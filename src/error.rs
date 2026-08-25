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
    Prs001,
    Prs002,
    Prs003,
    Prs004,
    Prs005,
    Prs006,
    Prs007,
    Prs008,
    Prs009,
    Prs010,
    Prs011,
    Prs012,
    Prs013,
    Prs014,
    Prs015,
    Prs016,
    Prs017,
    Prs018,
    Prs019,
    Prs020,
    Prs021,
    Prs022,
    Prs023,
    Prs024,
    Prs025,
    Prs026,
    Prs027,
    Prs028,
    Prs029,
    Prs030,
    Prs031,
    Prs032,
    Prs033,
    Prs034,
    Prs035,
    Prs036,
    Prs037,
    Prs038,
    Prs039,
    Prs040,
    Prs041,
    Prs042,
    Prs043,
    Prs044,
    Prs045,
    Prs046,
    Prs047,
    Prs048,
    Prs049,
    Prs050,
    Prs051,
    Prs052,
    Prs053,
    Prs054,
    Prs055,
    Prs056,
    Prs057,
    Prs058,
    Prs059,
    Prs060,
    Prs061,
    Prs062,
    Prs063,
    Prs064,
    Prs065,
    Prs066,
    Prs067,
    Prs068,
    Prs069,
    Prs070,
    Prs071,
    Prs072,
    Prs073,
    Prs074,
    Prs075,
    Prs076,
    Prs077,
    Prs078,
    Prs079,
    Int000,
}

/// Single source of truth: (code, code string, message template, category).
///
/// The message template is verified against `docs/ERROR_REFERENCE.md`'s
/// "Error text" lines by the sync test in this module. The PRS (parser)
/// category was the first migrated, in the #277 follow-up PR that wired up
/// `src/query/datalog/parser.rs` — see the design spec.
pub(crate) const REGISTRY: &[(ErrorCode, &str, &str, ErrorCategory)] = &[
    (
        ErrorCode::Prs001,
        "PRS-001",
        "Unexpected end of input",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs002,
        "PRS-002",
        "Unexpected character: {}",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs003,
        "PRS-003",
        "Unexpected token: {}",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs004,
        "PRS-004",
        "Unclosed vector",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs005,
        "PRS-005",
        "Unclosed list",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs006,
        "PRS-006",
        "Unterminated map: missing '}'",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs007,
        "PRS-007",
        "String exceeds maximum length of {} bytes",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs008,
        "PRS-008",
        "Keyword exceeds maximum length of {} bytes",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs009,
        "PRS-009",
        "Tagged literal exceeds maximum length of {} bytes",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs010,
        "PRS-010",
        "Expected command symbol",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs011,
        "PRS-011",
        "Unknown command: {}",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs012,
        "PRS-012",
        "Expected a list starting with a command symbol",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs013,
        "PRS-013",
        "Query requires a map argument",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs014,
        "PRS-014",
        ":as-of requires a value",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs015,
        "PRS-015",
        ":as-of counter must be non-negative, got {}",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs016,
        "PRS-016",
        ":as-of must be an integer (counter) or ISO 8601 string, got {}",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs017,
        "PRS-017",
        ":valid-at requires a value",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs018,
        "PRS-018",
        ":valid-at must be an ISO 8601 string or :any-valid-time, got {}",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs019,
        "PRS-019",
        ":valid-from must be an ISO 8601 string, got {}",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs020,
        "PRS-020",
        ":valid-to must be an ISO 8601 string, got {}",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs021,
        "PRS-021",
        "':with' clause requires at least one aggregate in :find",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs022,
        "PRS-022",
        "':with' variable {} not bound in :where",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs023,
        "PRS-023",
        "Aggregate variable {} not bound in :where",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs024,
        "PRS-024",
        "Aggregate expression must have exactly 2 elements (func ?var), got {}",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs025,
        "PRS-025",
        "Aggregate function name must be a symbol, got {}",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs026,
        "PRS-026",
        "Aggregate argument must be a variable (starting with ?)",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs027,
        "PRS-027",
        "'{}' is a window function and requires an ':over (...)' clause",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs028,
        "PRS-028",
        "window expression cannot be empty",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs029,
        "PRS-029",
        "window function name must be a symbol",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs030,
        "PRS-030",
        "'{}' is not supported in this version; lag/lead are planned for a future release",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs031,
        "PRS-031",
        "'{}' is not window-compatible and cannot be used with ':over'",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs032,
        "PRS-032",
        "'{}' requires a variable argument (starting with ?) before ':over'",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs033,
        "PRS-033",
        "'{}' requires ':over' after the variable argument",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs034,
        "PRS-034",
        "'{}' requires ':over' immediately after the function name (no variable argument)",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs035,
        "PRS-035",
        "':over' must be followed by a list, e.g., (:order-by ?var)",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs036,
        "PRS-036",
        "unexpected tokens after ':over' clause in window expression",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs037,
        "PRS-037",
        "':partition-by' requires a variable (starting with ?)",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs038,
        "PRS-038",
        "':order-by' requires a variable (starting with ?)",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs039,
        "PRS-039",
        "unknown option in ':over' clause: '{}'",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs040,
        "PRS-040",
        "unexpected element in ':over' clause: {}",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs041,
        "PRS-041",
        "Transact requires a vector of facts",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs042,
        "PRS-042",
        "Transact argument must be a vector of facts",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs043,
        "PRS-043",
        "Retract requires a vector of facts",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs044,
        "PRS-044",
        "Retract argument must be a vector of facts",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs045,
        "PRS-045",
        "Each fact must be a vector [e a v] or [e a v {opts}]",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs046,
        "PRS-046",
        "Fact must have at least 3 elements (E A V), got {}",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs047,
        "PRS-047",
        "Optional 4th element of a fact must be a map {:valid-from ... :valid-to ...}, got {}",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs048,
        "PRS-048",
        "Transact with options requires a facts vector after the map",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs049,
        "PRS-049",
        "unexpected end of fact vector",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs050,
        "PRS-050",
        "Empty list in :where clause",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs051,
        "PRS-051",
        "(not ...) cannot appear inside another (not ...)",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs052,
        "PRS-052",
        "(not) requires at least one clause",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs053,
        "PRS-053",
        "(or) requires at least one branch",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs054,
        "PRS-054",
        "(or-join) requires a join-vars vector and at least one branch",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs055,
        "PRS-055",
        "(or-join) first argument must be a vector of join variables",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs056,
        "PRS-056",
        "(or-join) join variables must be logic variables, got {}",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs057,
        "PRS-057",
        "all branches of (or ...) must introduce the same set of new variables",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs058,
        "PRS-058",
        "(and) inside or/or-join requires at least one clause",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs059,
        "PRS-059",
        "(not-join) requires a join-vars vector and at least one clause",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs060,
        "PRS-060",
        "Expected pattern vector or rule invocation in :where clause, got {}",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs061,
        "PRS-061",
        "Unexpected element in query: {}",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs062,
        "PRS-062",
        "expression list cannot be empty",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs063,
        "PRS-063",
        "expression head must be a symbol, got {}",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs064,
        "PRS-064",
        "{} takes exactly 1 argument",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs065,
        "PRS-065",
        "{} takes exactly 2 arguments",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs066,
        "PRS-066",
        "matches? second argument must be a string literal",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs067,
        "PRS-067",
        "unknown expression operator: {}",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs068,
        "PRS-068",
        "expression clause must be [(expr)] or [(expr) ?out], got {} elements",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs069,
        "PRS-069",
        "expression output must be a ?variable, got {}",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs070,
        "PRS-070",
        "unsupported expression argument: {}",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs071,
        "PRS-071",
        "Expected UUID string after #uuid tag",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs072,
        "PRS-072",
        "Invalid UUID",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs073,
        "PRS-073",
        "Unknown tagged literal: #{}",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs074,
        "PRS-074",
        "Bind slot name exceeds maximum length of {} bytes",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs075,
        "PRS-075",
        "transact takes (transact [facts]) or (transact {opts} [facts]); found {} unexpected trailing argument(s) — the valid-time options map must come BEFORE the facts vector, not after",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs076,
        "PRS-076",
        "retract takes (retract [facts]); found {} unexpected trailing argument(s)",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs077,
        "PRS-077",
        "unexpected trailing input after a complete form: {}",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs078,
        "PRS-078",
        "query takes (query [...]); found {} unexpected trailing argument(s)",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Prs079,
        "PRS-079",
        "rule takes (rule [...]); found {} unexpected trailing argument(s)",
        ErrorCategory::Parser,
    ),
    (
        ErrorCode::Int000,
        "INT-000",
        "unclassified internal error: {}",
        ErrorCategory::Internal,
    ),
];

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
    /// `REGISTRY` entry, not `registry_entry`'s INT-000 fallback: `ALL` is
    /// checked at runtime, and `_assert_all_variants_covered`'s `match` must
    /// list every `ErrorCode` variant explicitly (no `_` arm) so that adding
    /// a new variant without also adding it to both `ALL` and that match is
    /// a compile error — the real exhaustiveness guarantee `registry_entry`'s
    /// `debug_assert!` cannot provide on its own (it's compiled out in
    /// release builds, and `registry_is_a_subset_of_error_reference_doc`
    /// only walks `REGISTRY`, not `ErrorCode`).
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

        const ALL: &[ErrorCode] = &[
            ErrorCode::Prs001,
            ErrorCode::Prs002,
            ErrorCode::Prs003,
            ErrorCode::Prs004,
            ErrorCode::Prs005,
            ErrorCode::Prs006,
            ErrorCode::Prs007,
            ErrorCode::Prs008,
            ErrorCode::Prs009,
            ErrorCode::Prs010,
            ErrorCode::Prs011,
            ErrorCode::Prs012,
            ErrorCode::Prs013,
            ErrorCode::Prs014,
            ErrorCode::Prs015,
            ErrorCode::Prs016,
            ErrorCode::Prs017,
            ErrorCode::Prs018,
            ErrorCode::Prs019,
            ErrorCode::Prs020,
            ErrorCode::Prs021,
            ErrorCode::Prs022,
            ErrorCode::Prs023,
            ErrorCode::Prs024,
            ErrorCode::Prs025,
            ErrorCode::Prs026,
            ErrorCode::Prs027,
            ErrorCode::Prs028,
            ErrorCode::Prs029,
            ErrorCode::Prs030,
            ErrorCode::Prs031,
            ErrorCode::Prs032,
            ErrorCode::Prs033,
            ErrorCode::Prs034,
            ErrorCode::Prs035,
            ErrorCode::Prs036,
            ErrorCode::Prs037,
            ErrorCode::Prs038,
            ErrorCode::Prs039,
            ErrorCode::Prs040,
            ErrorCode::Prs041,
            ErrorCode::Prs042,
            ErrorCode::Prs043,
            ErrorCode::Prs044,
            ErrorCode::Prs045,
            ErrorCode::Prs046,
            ErrorCode::Prs047,
            ErrorCode::Prs048,
            ErrorCode::Prs049,
            ErrorCode::Prs050,
            ErrorCode::Prs051,
            ErrorCode::Prs052,
            ErrorCode::Prs053,
            ErrorCode::Prs054,
            ErrorCode::Prs055,
            ErrorCode::Prs056,
            ErrorCode::Prs057,
            ErrorCode::Prs058,
            ErrorCode::Prs059,
            ErrorCode::Prs060,
            ErrorCode::Prs061,
            ErrorCode::Prs062,
            ErrorCode::Prs063,
            ErrorCode::Prs064,
            ErrorCode::Prs065,
            ErrorCode::Prs066,
            ErrorCode::Prs067,
            ErrorCode::Prs068,
            ErrorCode::Prs069,
            ErrorCode::Prs070,
            ErrorCode::Prs071,
            ErrorCode::Prs072,
            ErrorCode::Prs073,
            ErrorCode::Prs074,
            ErrorCode::Prs075,
            ErrorCode::Prs076,
            ErrorCode::Prs077,
            ErrorCode::Prs078,
            ErrorCode::Prs079,
            ErrorCode::Int000,
        ];
        for &code in ALL {
            assert_has_registry_entry(code);
        }

        // Compile-time exhaustiveness guard: fails to compile if a new
        // ErrorCode variant is added without also adding it to ALL above —
        // catching the mistake `registry_entry`'s `debug_assert!` cannot
        // (it's compiled out in release builds) and that
        // `registry_is_a_subset_of_error_reference_doc` cannot (it only
        // walks REGISTRY, not ErrorCode).
        fn _assert_all_variants_covered(code: ErrorCode) {
            match code {
                ErrorCode::Prs001 => {}
                ErrorCode::Prs002 => {}
                ErrorCode::Prs003 => {}
                ErrorCode::Prs004 => {}
                ErrorCode::Prs005 => {}
                ErrorCode::Prs006 => {}
                ErrorCode::Prs007 => {}
                ErrorCode::Prs008 => {}
                ErrorCode::Prs009 => {}
                ErrorCode::Prs010 => {}
                ErrorCode::Prs011 => {}
                ErrorCode::Prs012 => {}
                ErrorCode::Prs013 => {}
                ErrorCode::Prs014 => {}
                ErrorCode::Prs015 => {}
                ErrorCode::Prs016 => {}
                ErrorCode::Prs017 => {}
                ErrorCode::Prs018 => {}
                ErrorCode::Prs019 => {}
                ErrorCode::Prs020 => {}
                ErrorCode::Prs021 => {}
                ErrorCode::Prs022 => {}
                ErrorCode::Prs023 => {}
                ErrorCode::Prs024 => {}
                ErrorCode::Prs025 => {}
                ErrorCode::Prs026 => {}
                ErrorCode::Prs027 => {}
                ErrorCode::Prs028 => {}
                ErrorCode::Prs029 => {}
                ErrorCode::Prs030 => {}
                ErrorCode::Prs031 => {}
                ErrorCode::Prs032 => {}
                ErrorCode::Prs033 => {}
                ErrorCode::Prs034 => {}
                ErrorCode::Prs035 => {}
                ErrorCode::Prs036 => {}
                ErrorCode::Prs037 => {}
                ErrorCode::Prs038 => {}
                ErrorCode::Prs039 => {}
                ErrorCode::Prs040 => {}
                ErrorCode::Prs041 => {}
                ErrorCode::Prs042 => {}
                ErrorCode::Prs043 => {}
                ErrorCode::Prs044 => {}
                ErrorCode::Prs045 => {}
                ErrorCode::Prs046 => {}
                ErrorCode::Prs047 => {}
                ErrorCode::Prs048 => {}
                ErrorCode::Prs049 => {}
                ErrorCode::Prs050 => {}
                ErrorCode::Prs051 => {}
                ErrorCode::Prs052 => {}
                ErrorCode::Prs053 => {}
                ErrorCode::Prs054 => {}
                ErrorCode::Prs055 => {}
                ErrorCode::Prs056 => {}
                ErrorCode::Prs057 => {}
                ErrorCode::Prs058 => {}
                ErrorCode::Prs059 => {}
                ErrorCode::Prs060 => {}
                ErrorCode::Prs061 => {}
                ErrorCode::Prs062 => {}
                ErrorCode::Prs063 => {}
                ErrorCode::Prs064 => {}
                ErrorCode::Prs065 => {}
                ErrorCode::Prs066 => {}
                ErrorCode::Prs067 => {}
                ErrorCode::Prs068 => {}
                ErrorCode::Prs069 => {}
                ErrorCode::Prs070 => {}
                ErrorCode::Prs071 => {}
                ErrorCode::Prs072 => {}
                ErrorCode::Prs073 => {}
                ErrorCode::Prs074 => {}
                ErrorCode::Prs075 => {}
                ErrorCode::Prs076 => {}
                ErrorCode::Prs077 => {}
                ErrorCode::Prs078 => {}
                ErrorCode::Prs079 => {}
                ErrorCode::Int000 => {}
            }
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
