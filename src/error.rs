//! Structured runtime error codes surfaced through `MinigrafError`.
//!
//! See `docs/superpowers/specs/2026-08-25-structured-error-codes-design.md`
//! and `docs/ERROR_REFERENCE.md` for the design and the full code registry.

use std::fmt;

/// Coarse-grained error category — the type library consumers match on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    Parser,
    Query,
    Storage,
    Wal,
    Api,
    Internal,
}

/// Crate-internal handle for one documented `docs/ERROR_REFERENCE.md` code.
///
/// Never exposed to library consumers — only the `&'static str` code string
/// it resolves to via [`REGISTRY`] is public (through [`MinigrafError::code`]).
/// Kept internal so this enum is free to be reorganized without breaking any
/// downstream `match`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ErrorCode {
    Int000,
}

/// Single source of truth: (code, code string, message template, category).
///
/// The message template is verified against `docs/ERROR_REFERENCE.md`'s
/// "Error text" lines by the sync test in this module (added once real
/// codes exist in a follow-up category PR — see the design spec).
pub(crate) const REGISTRY: &[(ErrorCode, &str, &str, ErrorCategory)] = &[(
    ErrorCode::Int000,
    "INT-000",
    "unclassified internal error: {}",
    ErrorCategory::Internal,
)];

pub(crate) fn registry_entry(code: ErrorCode) -> (&'static str, &'static str, ErrorCategory) {
    REGISTRY
        .iter()
        .find(|(c, ..)| *c == code)
        .map(|(_, code_str, template, category)| (*code_str, *template, *category))
        .expect("every ErrorCode variant must have a REGISTRY entry")
}

/// Fill a message template's positional `{}` placeholders from `args`, in order.
///
/// Not `std::fmt`-based: the template is runtime data pulled from [`REGISTRY`],
/// and `format!` requires a compile-time string literal.
pub(crate) fn format_template(template: &str, args: &[&dyn fmt::Display]) -> String {
    let placeholder_count = template.matches("{}").count();
    debug_assert_eq!(
        placeholder_count,
        args.len(),
        "format_template: template has a different placeholder count than args supplied"
    );
    let mut out = String::with_capacity(template.len());
    let mut args = args.iter();
    let mut rest = template;
    while let Some(pos) = rest.find("{}") {
        out.push_str(&rest[..pos]);
        if let Some(arg) = args.next() {
            out.push_str(&arg.to_string());
        }
        rest = &rest[pos + 2..];
    }
    out.push_str(rest);
    out
}

/// An `anyhow`-compatible error carrying a compile-time-checked [`ErrorCode`].
///
/// Built exclusively through [`bail_coded!`]/[`err_coded!`] and wrapped as an
/// `anyhow::Error`, so it rides through the crate's existing `anyhow::Result`
/// plumbing (`?`, `.context()`) unchanged. Recovered at the public API
/// boundary by [`MinigrafError::from`]`(anyhow::Error)` walking the error
/// chain for the first `CodedError` via `downcast_ref`.
#[derive(Debug)]
pub(crate) struct CodedError {
    code: ErrorCode,
    message: String,
}

impl CodedError {
    pub(crate) fn new(code: ErrorCode, args: &[&dyn fmt::Display]) -> Self {
        let (_, template, _) = registry_entry(code);
        CodedError {
            code,
            message: format_template(template, args),
        }
    }

    pub(crate) fn code(&self) -> ErrorCode {
        self.code
    }
}

impl fmt::Display for CodedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CodedError {}

/// Return early with a coded, `anyhow`-compatible error.
///
/// `bail_coded!(ErrorCode::Prs001)` for a static message, or
/// `bail_coded!(ErrorCode::Prs002, ch)` to fill the template's `{}`
/// placeholders positionally. A drop-in replacement for `anyhow::bail!`.
macro_rules! bail_coded {
    ($code:expr $(,)?) => {
        return Err(::anyhow::Error::new($crate::error::CodedError::new($code, &[])))
    };
    ($code:expr, $($arg:expr),+ $(,)?) => {
        return Err(::anyhow::Error::new($crate::error::CodedError::new(
            $code,
            &[$(&$arg as &dyn ::std::fmt::Display),+],
        )))
    };
}

/// Build (without returning) a coded, `anyhow`-compatible error.
///
/// For use where an `anyhow::Error` value is needed directly, e.g.
/// `.ok_or_else(|| err_coded!(ErrorCode::Api009))`.
macro_rules! err_coded {
    ($code:expr $(,)?) => {
        ::anyhow::Error::new($crate::error::CodedError::new($code, &[]))
    };
    ($code:expr, $($arg:expr),+ $(,)?) => {
        ::anyhow::Error::new($crate::error::CodedError::new(
            $code,
            &[$(&$arg as &dyn ::std::fmt::Display),+],
        ))
    };
}

pub(crate) use bail_coded;
pub(crate) use err_coded;

/// The error type returned from Minigraf's public API.
///
/// Carries a matchable [`category`](MinigrafError::category) and a stable
/// [`code`](MinigrafError::code) (e.g. `"PRS-001"`) documented in
/// `docs/ERROR_REFERENCE.md`. `Display` renders as `[CODE] message`.
#[derive(Debug)]
pub struct MinigrafError {
    category: ErrorCategory,
    code: &'static str,
    message: String,
    source: anyhow::Error,
}

impl MinigrafError {
    /// The coarse-grained category — the type to `match` on.
    pub fn category(&self) -> ErrorCategory {
        self.category
    }

    /// The stable reference code, e.g. `"PRS-001"`. See `docs/ERROR_REFERENCE.md`.
    pub fn code(&self) -> &'static str {
        self.code
    }
}

impl fmt::Display for MinigrafError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for MinigrafError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&*self.source)
    }
}

impl From<anyhow::Error> for MinigrafError {
    fn from(err: anyhow::Error) -> Self {
        for cause in err.chain() {
            if let Some(coded) = cause.downcast_ref::<CodedError>() {
                let (code_str, _, category) = registry_entry(coded.code());
                return MinigrafError {
                    category,
                    code: code_str,
                    message: coded.to_string(),
                    source: err,
                };
            }
        }
        let message = err.to_string();
        MinigrafError {
            category: ErrorCategory::Internal,
            code: "INT-000",
            message,
            source: err,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_template_no_placeholders() {
        assert_eq!(format_template("no placeholders here", &[]), "no placeholders here");
    }

    #[test]
    fn format_template_one_placeholder() {
        let arg = 42;
        assert_eq!(
            format_template("value: {}", &[&arg as &dyn std::fmt::Display]),
            "value: 42"
        );
    }

    #[test]
    fn format_template_multiple_placeholders() {
        let a = "x";
        let b = 7;
        assert_eq!(
            format_template(
                "{} then {}",
                &[&a as &dyn std::fmt::Display, &b as &dyn std::fmt::Display]
            ),
            "x then 7"
        );
    }

    #[test]
    fn registry_entry_resolves_int000() {
        let (code_str, template, category) = registry_entry(ErrorCode::Int000);
        assert_eq!(code_str, "INT-000");
        assert_eq!(template, "unclassified internal error: {}");
        assert_eq!(category, ErrorCategory::Internal);
    }

    #[test]
    fn coded_error_display_uses_template() {
        let e = CodedError::new(ErrorCode::Int000, &[&"boom" as &dyn std::fmt::Display]);
        assert_eq!(e.to_string(), "unclassified internal error: boom");
    }

    #[test]
    fn bail_coded_returns_anyhow_error_wrapping_coded_error() {
        fn inner() -> anyhow::Result<()> {
            bail_coded!(ErrorCode::Int000, "boom");
        }
        let err = inner().unwrap_err();
        let coded = err.downcast_ref::<CodedError>().expect("expected a CodedError");
        assert_eq!(coded.code(), ErrorCode::Int000);
        assert_eq!(coded.to_string(), "unclassified internal error: boom");
    }

    #[test]
    fn err_coded_builds_anyhow_error_without_returning() {
        let err = err_coded!(ErrorCode::Int000, "boom");
        let coded = err.downcast_ref::<CodedError>().expect("expected a CodedError");
        assert_eq!(coded.code(), ErrorCode::Int000);
    }

    #[test]
    fn minigraf_error_display_format() {
        let anyhow_err = err_coded!(ErrorCode::Int000, "boom");
        let e: MinigrafError = anyhow_err.into();
        assert_eq!(e.to_string(), "[INT-000] unclassified internal error: boom");
        assert_eq!(e.code(), "INT-000");
        assert_eq!(e.category(), ErrorCategory::Internal);
    }

    #[test]
    fn minigraf_error_finds_coded_error_through_context_chain() {
        let anyhow_err = err_coded!(ErrorCode::Int000, "boom").context("extra context");
        let e: MinigrafError = anyhow_err.into();
        assert_eq!(e.code(), "INT-000");
        assert_eq!(e.to_string(), "[INT-000] unclassified internal error: boom");
    }

    #[test]
    fn minigraf_error_falls_back_to_int000_for_uncoded_anyhow_error() {
        let anyhow_err = anyhow::anyhow!("plain uncoded error");
        let e: MinigrafError = anyhow_err.into();
        assert_eq!(e.code(), "INT-000");
        assert_eq!(e.category(), ErrorCategory::Internal);
        assert_eq!(e.to_string(), "[INT-000] plain uncoded error");
    }

    #[test]
    fn minigraf_error_implements_std_error_source() {
        let anyhow_err = err_coded!(ErrorCode::Int000, "boom");
        let e: MinigrafError = anyhow_err.into();
        let err: &dyn std::error::Error = &e;
        assert!(err.source().is_some());
    }
}
