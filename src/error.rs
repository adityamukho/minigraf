//! Structured runtime error codes surfaced through `MinigrafError`.
//!
//! See `docs/superpowers/specs/2026-08-25-structured-error-codes-design.md`
//! and `docs/ERROR_REFERENCE.md` for the design and the full code registry.

use std::fmt;

/// Coarse-grained error category — the type library consumers match on.
///
/// `#[non_exhaustive]`: a future category can be added without that being a
/// breaking change. Downstream `match` expressions must include a wildcard
/// `_` arm. Has no effect within this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorCategory {
    /// Datalog/EDN syntax errors (`PRS-` codes).
    Parser,
    /// Query planning/execution errors (`QRY-` codes).
    Query,
    /// On-disk storage/file-format errors (`STG-` codes).
    Storage,
    /// Write-ahead log errors (`WAL-` codes).
    Wal,
    /// Public `Minigraf` API misuse or internal sequencing errors (`API-` codes).
    Api,
    /// Unclassified internal invariant violations (`INT-` codes).
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
    let entry = REGISTRY.iter().find(|(c, ..)| *c == code);
    // This is NOT exhaustively guaranteed: `registry_is_a_subset_of_error_reference_doc`
    // iterates over `REGISTRY`, not over `ErrorCode` variants, so it cannot catch a new
    // `ErrorCode` variant added without a matching `REGISTRY` row. The `debug_assert!`
    // below catches that mistake in debug/test builds only — nothing catches it in a
    // release build, where it silently falls back to the always-present INT-000 entry
    // below instead of panicking, since `unwrap`/`expect`/`panic!` are denied crate-wide.
    // `error::tests::every_error_code_variant_has_a_registry_entry` is the real
    // exhaustiveness guarantee: its `match` over `ErrorCode` fails to compile if a new
    // variant is added without also being handled there.
    debug_assert!(
        entry.is_some(),
        "every ErrorCode variant must have a REGISTRY entry"
    );
    match entry {
        Some((_, code_str, template, category)) => (*code_str, *template, *category),
        None => (
            "INT-000",
            "unclassified internal error: {}",
            ErrorCategory::Internal,
        ),
    }
}

/// Fill a message template's positional `{}` placeholders from `args`, in order.
///
/// Not `std::fmt`-based: the template is runtime data pulled from [`REGISTRY`],
/// and `format!` requires a compile-time string literal.
#[allow(dead_code)] // unused until the PRS category PR wires up real call sites
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
    #[allow(dead_code)] // unused until the PRS category PR wires up real call sites
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
#[allow(unused_macros)] // unused until the PRS category PR wires up real call sites
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
#[allow(unused_macros)] // unused until the PRS category PR wires up real call sites
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

#[allow(unused_imports)] // unused until the PRS category PR wires up real call sites
pub(crate) use bail_coded;
#[allow(unused_imports)] // unused until the PRS category PR wires up real call sites
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
        let message_text = err.to_string();
        let (code_str, template, category) = registry_entry(ErrorCode::Int000);
        let message = format_template(template, &[&message_text as &dyn fmt::Display]);
        MinigrafError {
            category,
            code: code_str,
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
        assert_eq!(
            format_template("no placeholders here", &[]),
            "no placeholders here"
        );
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

    /// Exhaustively verifies every `ErrorCode` variant resolves to a real
    /// `REGISTRY` entry, not `registry_entry`'s INT-000 fallback. The `match`
    /// below must list every `ErrorCode` variant explicitly (no `_` arm) so
    /// that adding a new variant without also handling it here is a compile
    /// error — the real exhaustiveness guarantee `registry_entry`'s
    /// `debug_assert!` cannot provide on its own (it's compiled out in
    /// release builds, and `registry_is_a_subset_of_error_reference_doc`
    /// only walks `REGISTRY`, not `ErrorCode`). Necessarily small today
    /// since `ErrorCode` has just one variant, but the structure is what
    /// matters for the PRS category PR that adds ~79 more.
    #[test]
    fn every_error_code_variant_has_a_registry_entry() {
        fn assert_has_registry_entry(code: ErrorCode) {
            let (code_str, _, _) = registry_entry(code);
            assert_ne!(
                code_str, "",
                "ErrorCode variant resolved to an empty registry code"
            );
            assert!(
                REGISTRY.iter().any(|(c, ..)| *c == code),
                "ErrorCode variant has no REGISTRY entry"
            );
        }

        match ErrorCode::Int000 {
            ErrorCode::Int000 => assert_has_registry_entry(ErrorCode::Int000),
        }
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
        let coded = err
            .downcast_ref::<CodedError>()
            .expect("expected a CodedError");
        assert_eq!(coded.code(), ErrorCode::Int000);
        assert_eq!(coded.to_string(), "unclassified internal error: boom");
    }

    #[test]
    fn err_coded_builds_anyhow_error_without_returning() {
        let err = err_coded!(ErrorCode::Int000, "boom");
        let coded = err
            .downcast_ref::<CodedError>()
            .expect("expected a CodedError");
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
    fn minigraf_error_finds_coded_error_through_two_context_hops() {
        let anyhow_err = err_coded!(ErrorCode::Int000, "boom")
            .context("first context")
            .context("second context");
        let e: MinigrafError = anyhow_err.into();
        assert_eq!(e.code(), "INT-000");
        assert_eq!(e.to_string(), "[INT-000] unclassified internal error: boom");
    }

    // A genuine arg-count mismatch trips the `debug_assert_eq!` in
    // `format_template` — active under `cargo test`'s debug build, which is
    // what this test exercises. In a release build (`debug_assert!` compiled
    // out) the same call does not panic: the loop below the assert simply
    // drops the text of any `{}` placeholder with no matching arg.
    #[test]
    #[should_panic(expected = "different placeholder count")]
    fn format_template_arg_count_mismatch_debug_asserts() {
        format_template("value: {}", &[]);
    }

    #[test]
    fn minigraf_error_falls_back_to_int000_for_uncoded_anyhow_error() {
        let anyhow_err = anyhow::anyhow!("plain uncoded error");
        let e: MinigrafError = anyhow_err.into();
        assert_eq!(e.code(), "INT-000");
        assert_eq!(e.category(), ErrorCategory::Internal);
        assert_eq!(
            e.to_string(),
            "[INT-000] unclassified internal error: plain uncoded error"
        );
    }

    #[test]
    fn minigraf_error_implements_std_error_source() {
        let anyhow_err = err_coded!(ErrorCode::Int000, "boom");
        let e: MinigrafError = anyhow_err.into();
        let err: &dyn std::error::Error = &e;
        assert!(err.source().is_some());
    }

    /// `docs/ERROR_REFERENCE.md` already documents all 130 codes (from #192)
    /// before #277 starts touching runtime codes — the doc isn't built up
    /// incrementally alongside `REGISTRY`, it's already complete. So this
    /// test can only check `REGISTRY` is a content-matched subset of the
    /// doc (every registry entry's code+template appears in the doc
    /// verbatim) — not that every documented code has a registry entry yet.
    /// A documented code with no registry entry just means that call site
    /// hasn't been migrated. The final category PR (once every remaining
    /// call site is audited and coded) should upgrade this to a full
    /// bidirectional equality check.
    #[test]
    fn registry_is_a_subset_of_error_reference_doc() {
        let doc = include_str!("../docs/ERROR_REFERENCE.md");
        let mut doc_entries: std::collections::HashMap<String, String> = Default::default();
        let mut lines = doc.lines().peekable();
        while let Some(line) = lines.next() {
            let Some(rest) = line.strip_prefix("### ") else {
                continue;
            };
            let code = rest.split_whitespace().next().unwrap_or("").to_string();
            if !is_error_reference_code(&code) {
                continue;
            }
            let mut error_text = None;
            while let Some(&next_line) = lines.peek() {
                if next_line.starts_with("### ") || next_line.starts_with("## ") {
                    break;
                }
                if let Some(text) = next_line.strip_prefix("**Error text**: `") {
                    error_text = text.strip_suffix('`').map(str::to_string);
                    lines.next();
                    break;
                }
                lines.next();
            }
            if let Some(text) = error_text {
                doc_entries.insert(code, text);
            }
        }

        for (_, code_str, template, _) in REGISTRY {
            match doc_entries.get(*code_str) {
                Some(doc_text) => assert_eq!(
                    doc_text, template,
                    "REGISTRY template for a documented code doesn't match its ERROR_REFERENCE.md Error text"
                ),
                None => panic!("REGISTRY has a code with no ERROR_REFERENCE.md entry at all"),
            }
        }
    }

    fn is_error_reference_code(s: &str) -> bool {
        let bytes = s.as_bytes();
        bytes.len() == 7
            && bytes[..3].iter().all(u8::is_ascii_uppercase)
            && bytes[3] == b'-'
            && bytes[4..].iter().all(u8::is_ascii_digit)
    }
}
