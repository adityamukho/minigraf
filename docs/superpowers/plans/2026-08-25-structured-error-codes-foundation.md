# Structured Error Codes — Foundation PR Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the foundational error-code machinery (types, macros, boundary conversion, public API signature change) for issue #277 with zero call sites migrated yet — every existing error still round-trips through the new public API as a generic `INT-000` code, so the crate compiles and the full test suite stays green.

**Architecture:** New `src/error.rs` module defines `ErrorCategory` (public, 6-variant, matchable), `ErrorCode` (crate-internal, one variant per documented code — starts with just `Int000`), a `REGISTRY` const table mapping code → (code string, message template, category), a hand-rolled `format_template` runtime formatter, `CodedError` (wraps as `anyhow::Error` at `bail_coded!`/`err_coded!` call sites), and `MinigrafError` (the new public error type: `Display` as `[CODE] message`, `std::error::Error`, built from any `anyhow::Error` via chain-walking downcast with an `INT-000` fallback). `db.rs`'s public functions (plus `WriteTransaction` and `PreparedQuery`) get a thin `pub` wrapper added in front of their existing body, renamed to a private `_inner` twin — the wrapper is the only new logic; existing bodies are untouched except for the rename.

**Tech Stack:** Rust 2024 edition, `anyhow` 1.0 (existing dependency, no new dependencies added — no `thiserror`).

**Spec:** `docs/superpowers/specs/2026-08-25-structured-error-codes-design.md`

## Global Constraints

- No new dependencies (spec Non-goals: no `thiserror`, no codegen/build-script markdown parsing).
- Internal call sites keep `anyhow::Result<T>` / `?` / `.context()` unchanged — only `db.rs`'s `pub fn` signatures (and `WriteTransaction`, `PreparedQuery`) change shape.
- `ErrorCode` and `REGISTRY` are `pub(crate)` — never exposed to library consumers directly.
- Every `MinigrafError` always has a code; no `Option<code>`.
- Test assertion messages: never `{:?}`-format a `Result`/`Fact`/`Value`/`EdnValue`/`Uuid`-bearing type into an `assert!`/`assert_eq!` message string — use plain literals, `.unwrap()`/`.expect()`, or count/bool/string-only assertions (see CLAUDE.md Testing Conventions).
- No GitHub closing keyword (`fixes`/`closes`/`resolves`) referencing **#277** in any commit message or the eventual PR body — reference it as plain text ("part of #277", "toward #277") only.
- Verification commands (must all pass before the final commit): `cargo fmt -- --check`, `cargo clippy --all-features -- -D warnings`, `cargo test`.

---

## Task 1: `src/error.rs` — categories, codes, registry, template formatter

**Files:**
- Create: `src/error.rs`

**Interfaces:**
- Produces: `pub enum ErrorCategory { Parser, Query, Storage, Wal, Api, Internal }` (derives `Debug, Clone, Copy, PartialEq, Eq`); `pub(crate) enum ErrorCode { Int000 }` (derives `Debug, Clone, Copy, PartialEq, Eq`); `pub(crate) const REGISTRY: &[(ErrorCode, &str, &str, ErrorCategory)]`; `pub(crate) fn registry_entry(code: ErrorCode) -> (&'static str, &'static str, ErrorCategory)`; `pub(crate) fn format_template(template: &str, args: &[&dyn std::fmt::Display]) -> String`.

- [ ] **Step 1: Write the failing tests**

Create `src/error.rs` with just the test module first:

```rust
//! Structured runtime error codes surfaced through `MinigrafError`.
//!
//! See `docs/superpowers/specs/2026-08-25-structured-error-codes-design.md`
//! and `docs/ERROR_REFERENCE.md` for the design and the full code registry.

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
}
```

- [ ] **Step 2: Run tests to verify they fail to compile**

Run: `cargo test --lib error:: -- --nocapture`
Expected: compile error — `format_template`, `registry_entry`, `ErrorCategory`, `ErrorCode` not yet defined.

- [ ] **Step 3: Implement the module**

Add above the test module in `src/error.rs`:

```rust
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib error:: -- --nocapture`
Expected: all 4 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/error.rs
git commit -m "$(cat <<'EOF'
feat: add ErrorCategory/ErrorCode/REGISTRY/format_template for #277

Foundation types for structured runtime error codes. No call sites
wired up yet — this is pure scaffolding. Part of #277.
EOF
)"
```

---

## Task 2: `CodedError` + `bail_coded!`/`err_coded!` macros

**Files:**
- Modify: `src/error.rs`

**Interfaces:**
- Consumes: `ErrorCode`, `registry_entry`, `format_template` (Task 1).
- Produces: `pub(crate) struct CodedError { code: ErrorCode, message: String }` with `impl fmt::Display`, `impl std::error::Error`, `pub(crate) fn new(code: ErrorCode, args: &[&dyn fmt::Display]) -> Self`, and a `pub(crate) fn code(&self) -> ErrorCode`; macros `bail_coded!` and `err_coded!`, both re-exported crate-wide via `pub(crate) use`.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src/error.rs`:

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail to compile**

Run: `cargo test --lib error:: -- --nocapture`
Expected: compile error — `CodedError`, `bail_coded!`, `err_coded!` not yet defined.

- [ ] **Step 3: Implement `CodedError` and the macros**

Add to `src/error.rs`, after the `format_template` function and before the `tests` module:

```rust
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib error:: -- --nocapture`
Expected: all tests PASS (7 total so far).

- [ ] **Step 5: Commit**

```bash
git add src/error.rs
git commit -m "$(cat <<'EOF'
feat: add CodedError and bail_coded!/err_coded! macros for #277

Call-site macros that tag an anyhow::Error with a compile-time-checked
ErrorCode, formatted from the code's REGISTRY template. Not yet used
by any real call site. Part of #277.
EOF
)"
```

---

## Task 3: `MinigrafError` + boundary conversion (`From<anyhow::Error>`)

**Files:**
- Modify: `src/error.rs`

**Interfaces:**
- Consumes: `ErrorCategory`, `ErrorCode`, `registry_entry`, `CodedError` (Tasks 1–2).
- Produces: `pub struct MinigrafError` (derives `Debug`) with `pub fn category(&self) -> ErrorCategory`, `pub fn code(&self) -> &'static str`, `impl fmt::Display for MinigrafError` (`"[{code}] {message}"`), `impl std::error::Error for MinigrafError`, `impl From<anyhow::Error> for MinigrafError`.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src/error.rs`:

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail to compile**

Run: `cargo test --lib error:: -- --nocapture`
Expected: compile error — `MinigrafError` not yet defined.

- [ ] **Step 3: Implement `MinigrafError`**

Add to `src/error.rs`, after the macro definitions and before the `tests` module:

```rust
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib error:: -- --nocapture`
Expected: all tests PASS (11 total so far).

- [ ] **Step 5: Commit**

```bash
git add src/error.rs
git commit -m "$(cat <<'EOF'
feat: add MinigrafError and anyhow::Error boundary conversion for #277

MinigrafError is the new public error type: category + stable code +
Display as "[CODE] message" + std::error::Error. From<anyhow::Error>
walks the error chain for a CodedError, falling back to INT-000 for
any error not yet migrated to a specific code. Part of #277.
EOF
)"
```

---

## Task 4: Wire `error` module into `lib.rs`; add INT-000 to `ERROR_REFERENCE.md`; registry↔doc sync test

**Files:**
- Modify: `src/lib.rs`
- Modify: `docs/ERROR_REFERENCE.md`
- Modify: `src/error.rs` (sync test)

**Interfaces:**
- Consumes: `MinigrafError`, `ErrorCategory`, `REGISTRY` (Tasks 1–3).
- Produces: `minigraf::MinigrafError`, `minigraf::ErrorCategory` at the crate's public root; `docs/ERROR_REFERENCE.md` documents `INT-000`; a test enforcing `REGISTRY` and the doc never drift apart.

- [ ] **Step 1: Export the new types from `lib.rs`**

In `src/lib.rs`, add `pub mod error;` next to the other `pub mod` declarations (near line 77's `pub mod db;`), and add to the `pub use` block (near line 91-92):

```rust
pub mod error;
```

```rust
pub use error::{ErrorCategory, MinigrafError};
```

- [ ] **Step 2: Add the `INT — Internal Errors` section to `docs/ERROR_REFERENCE.md`**

Insert immediately before the existing `## Appendix: Internal Errors` heading (currently the last section in the file):

```markdown
## INT — Internal Errors

Internal errors are invariant violations that should not be reachable through
normal use of the public API. `INT-000` is also the generic catch-all every
runtime error falls back to until its call site is migrated to a specific
structured code — see [#277](https://github.com/project-minigraf/minigraf/issues/277).
If you see one of these in practice, it likely indicates a bug in Minigraf
itself; please [file an issue](https://github.com/project-minigraf/minigraf/issues).

### INT-000 Unclassified internal error

**Error text**: `unclassified internal error: {}`

**Cause**: A `bail!`/`anyhow!` call site has not yet been migrated to a specific structured error code, or the error genuinely is an unreachable internal invariant violation.

**Resolution**:
- Read the wrapped message text (the `{}` above) for the underlying cause.
- Match on message content rather than this code if you need long-term programmatic stability — `INT-000` is expected to cover fewer cases over time as call sites gain specific codes.

**Scenario**: Any error not yet covered by a migrated `PRS-`/`STG-`/`WAL-`/`QRY-`/`API-` code.

---

```

Also update the Quick Reference Table: add a row after the `API-009` row (immediately before the `---` that follows the table):

```markdown
| INT-000 | Unclassified internal error | Internal |
```

And replace the existing caveat paragraph near the top of the file (the one starting "**Reference codes** (e.g. `PRS-001`) are documentation-only identifiers...") with:

```markdown
**Reference codes** (e.g. `PRS-001`) appear in runtime error output as
`[CODE] message`, and programmatically via `MinigrafError::code()` /
`MinigrafError::category()`. As of the [#277](https://github.com/project-minigraf/minigraf/issues/277)
foundation PR, every runtime error carries a code — but until each
category's follow-up migration PR lands, most still surface generically as
`INT-000` rather than their documented code below.
```

- [ ] **Step 3: Write the failing sync test**

Add to the `tests` module in `src/error.rs`:

```rust
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
```

- [ ] **Step 4: Run the test**

Run: `cargo test --lib error:: -- --nocapture`
Expected: initially FAILs if the `INT-000` doc entry added in Step 2 isn't exactly byte-for-byte consistent with `REGISTRY`'s single entry (code string `"INT-000"`, template `"unclassified internal error: {}"`) — adjust the doc's "**Error text**" line until it PASSes. Then all tests in `src/error.rs` PASS (12 total).

- [ ] **Step 5: Run the full test suite to confirm `lib.rs` changes compile cleanly**

Run: `cargo test`
Expected: PASS, no new warnings.

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/error.rs docs/ERROR_REFERENCE.md
git commit -m "$(cat <<'EOF'
feat: export MinigrafError/ErrorCategory; document INT-000; sync test

Wires the new error module into the public API surface, documents the
INT-000 catch-all code in ERROR_REFERENCE.md, and adds a test that
fails if REGISTRY and the doc's code list/message templates ever
diverge. Part of #277.
EOF
)"
```

---

## Task 5: Migrate `Minigraf::open`/`open_with_options`/`in_memory`/`in_memory_with_options` and the `OpenOptions`/`OpenOptionsWithPath` builders

**Files:**
- Modify: `src/db.rs`
- Test: `tests/error_codes_foundation_test.rs` (new)

**Interfaces:**
- Consumes: `MinigrafError` (Task 3).
- Produces: `Minigraf::open`, `Minigraf::open_with_options`, `Minigraf::in_memory`, `Minigraf::in_memory_with_options`, `OpenOptions::open_memory`, `OpenOptionsWithPath::open` all return `Result<Minigraf, MinigrafError>` (previously `anyhow::Result<Minigraf>`). Private `_inner` twins (`Minigraf::open_with_options_inner`, `Minigraf::in_memory_with_options_inner`) keep the exact original bodies, returning `anyhow::Result<Minigraf>`, and are what later category PRs (e.g. STG) will migrate to `bail_coded!`.

- [ ] **Step 1: Write the failing test**

Create `tests/error_codes_foundation_test.rs`:

```rust
use minigraf::{ErrorCategory, Minigraf};

#[test]
fn in_memory_open_returns_ok() {
    let db = Minigraf::in_memory();
    assert!(db.is_ok(), "in_memory() should succeed");
}

#[test]
fn open_nonexistent_directory_returns_coded_error() {
    let bad_path = "/nonexistent-dir-for-minigraf-test-277/db.graph";
    let result = Minigraf::open(bad_path);
    let err = result.expect_err("opening a file in a nonexistent directory should fail");
    assert_eq!(err.category(), ErrorCategory::Internal);
    assert_eq!(err.code(), "INT-000");
}
```

- [ ] **Step 2: Run test to verify it fails to compile**

Run: `cargo test --test error_codes_foundation_test -- --nocapture`
Expected: compile error — `Minigraf::open`/`in_memory` still return `anyhow::Result<Minigraf>`, so `.category()`/`.code()` don't exist on the error type, and `minigraf::ErrorCategory` doesn't resolve to anything usable here yet (type mismatch).

- [ ] **Step 3: Migrate the four `Minigraf` associate functions and the two builder methods**

In `src/db.rs`, add `use crate::error::MinigrafError;` to the top-level `use` block (next to the existing `use anyhow::{Result, bail};`).

Change (in `impl OpenOptions`):

```rust
    pub fn open_memory(self) -> Result<Minigraf> {
        Minigraf::in_memory_with_options(self)
    }
```

to:

```rust
    pub fn open_memory(self) -> Result<Minigraf, MinigrafError> {
        Minigraf::in_memory_with_options(self)
    }
```

Change (in `impl OpenOptionsWithPath`):

```rust
    pub fn open(self) -> Result<Minigraf> {
        Minigraf::open_with_options(self.path, self.opts)
    }
```

to:

```rust
    pub fn open(self) -> Result<Minigraf, MinigrafError> {
        Minigraf::open_with_options(self.path, self.opts)
    }
```

Change (in `impl Minigraf`):

```rust
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_options(path, OpenOptions::default())
    }
```

to:

```rust
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open(path: impl AsRef<Path>) -> Result<Self, MinigrafError> {
        Self::open_with_options(path, OpenOptions::default())
    }
```

Change the `open_with_options` signature line only (keep the entire body below it — from `let db_path = path.as_ref().to_path_buf();` through the closing `Ok(Minigraf { ... })` and its final `}` — completely unchanged):

```rust
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_with_options(path: impl AsRef<Path>, opts: OpenOptions) -> Result<Self> {
```

to:

```rust
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_with_options(path: impl AsRef<Path>, opts: OpenOptions) -> Result<Self, MinigrafError> {
        Self::open_with_options_inner(path, opts).map_err(MinigrafError::from)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open_with_options_inner(path: impl AsRef<Path>, opts: OpenOptions) -> Result<Self> {
```

(This inserts a new thin wrapper and renames the original function to `open_with_options_inner`, whose unchanged body — and its closing `}` — now terminates the renamed function instead.)

Change:

```rust
    pub fn in_memory() -> Result<Self> {
        Self::in_memory_with_options(OpenOptions::default())
    }
```

to:

```rust
    pub fn in_memory() -> Result<Self, MinigrafError> {
        Self::in_memory_with_options(OpenOptions::default())
    }
```

Change the `in_memory_with_options` signature line only (body from `let backend = MemoryBackend::new();` through `Ok(Minigraf { ... })` stays unchanged):

```rust
    pub fn in_memory_with_options(opts: OpenOptions) -> Result<Self> {
```

to:

```rust
    pub fn in_memory_with_options(opts: OpenOptions) -> Result<Self, MinigrafError> {
        Self::in_memory_with_options_inner(opts).map_err(MinigrafError::from)
    }

    fn in_memory_with_options_inner(opts: OpenOptions) -> Result<Self> {
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --test error_codes_foundation_test -- --nocapture`
Expected: both tests PASS.

- [ ] **Step 5: Run the full test suite**

Run: `cargo test`
Expected: PASS. If any existing test fails to compile because it relied on `anyhow`-specific behavior from these four functions' errors (e.g. `.context()` chaining on the returned error), fix that call site to use `MinigrafError`'s `Display`/`.code()`/`.category()` instead — do not add `.context()` support to `MinigrafError`.

- [ ] **Step 6: Commit**

```bash
git add src/db.rs tests/error_codes_foundation_test.rs
git commit -m "$(cat <<'EOF'
feat: migrate Minigraf::open/in_memory family to MinigrafError

open, open_with_options, in_memory, in_memory_with_options, and the
OpenOptions/OpenOptionsWithPath builders now return
Result<Minigraf, MinigrafError>. Bodies are unchanged (renamed to
_inner twins); every error still surfaces as INT-000 until a later
category PR wires up real codes. Part of #277.
EOF
)"
```

---

## Task 6: Migrate `Minigraf::execute`

**Files:**
- Modify: `src/db.rs`
- Modify: `tests/error_codes_foundation_test.rs`

**Interfaces:**
- Consumes: `MinigrafError` (Task 3).
- Produces: `Minigraf::execute` returns `Result<QueryResult, MinigrafError>`. Private `Minigraf::execute_inner` keeps the exact original body, returning `anyhow::Result<QueryResult>`.

- [ ] **Step 1: Write the failing test**

Add to `tests/error_codes_foundation_test.rs`:

```rust
#[test]
fn execute_parse_error_returns_coded_error() {
    let db = Minigraf::in_memory().unwrap();
    let result = db.execute("(this is not valid datalog");
    let err = result.expect_err("malformed input should fail to parse");
    assert_eq!(err.category(), ErrorCategory::Internal);
    assert_eq!(err.code(), "INT-000");
}

#[test]
fn execute_valid_transact_returns_ok() {
    let db = Minigraf::in_memory().unwrap();
    let result = db.execute(r#"(transact [[#uuid "550e8400-e29b-41d4-a716-446655440000" :name "alice"]])"#);
    assert!(result.is_ok(), "valid transact should succeed");
}
```

- [ ] **Step 2: Run test to verify it fails to compile**

Run: `cargo test --test error_codes_foundation_test -- --nocapture`
Expected: compile error — `Minigraf::execute` still returns `anyhow::Result<QueryResult>`.

- [ ] **Step 3: Migrate `execute`**

In `src/db.rs`, change the signature line only (the doc comment above it, and the entire body below — from the `if is_write_tx_active() { bail!(...) }` check through the final `else` branch's `executor.execute(cmd)` and closing `}` — stay unchanged):

```rust
    pub fn execute(&self, input: &str) -> Result<QueryResult> {
```

to:

```rust
    pub fn execute(&self, input: &str) -> Result<QueryResult, MinigrafError> {
        self.execute_inner(input).map_err(MinigrafError::from)
    }

    fn execute_inner(&self, input: &str) -> Result<QueryResult> {
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --test error_codes_foundation_test -- --nocapture`
Expected: all tests PASS.

- [ ] **Step 5: Run the full test suite**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/db.rs tests/error_codes_foundation_test.rs
git commit -m "$(cat <<'EOF'
feat: migrate Minigraf::execute to MinigrafError

Signature-only change; body renamed to execute_inner, unchanged.
Part of #277.
EOF
)"
```

---

## Task 7: Migrate `Minigraf::begin_write` and `Minigraf::checkpoint`

**Files:**
- Modify: `src/db.rs`
- Modify: `tests/error_codes_foundation_test.rs`

**Interfaces:**
- Consumes: `MinigrafError` (Task 3).
- Produces: `Minigraf::begin_write` returns `Result<WriteTransaction<'_>, MinigrafError>`; `Minigraf::checkpoint` returns `Result<(), MinigrafError>`. Private `_inner` twins keep the original bodies.

- [ ] **Step 1: Write the failing test**

Add to `tests/error_codes_foundation_test.rs`:

```rust
#[test]
fn begin_write_then_checkpoint_returns_ok() {
    let db = Minigraf::in_memory().unwrap();
    let tx = db.begin_write();
    assert!(tx.is_ok(), "begin_write on a fresh db should succeed");
    tx.unwrap().rollback();
    let checkpoint = db.checkpoint();
    assert!(checkpoint.is_ok(), "checkpoint on an in-memory db is a no-op success");
}
```

- [ ] **Step 2: Run test to verify it fails to compile**

Run: `cargo test --test error_codes_foundation_test -- --nocapture`
Expected: compile error — both functions still return `anyhow::Result<_>`.

- [ ] **Step 3: Migrate `begin_write` and `checkpoint`**

Change the signature line only (body — the `is_write_tx_active()` check through the final `Ok(WriteTransaction { ... })` and closing `}` — stays unchanged):

```rust
    pub fn begin_write(&self) -> Result<WriteTransaction<'_>> {
```

to:

```rust
    pub fn begin_write(&self) -> Result<WriteTransaction<'_>, MinigrafError> {
        self.begin_write_inner().map_err(MinigrafError::from)
    }

    fn begin_write_inner(&self) -> Result<WriteTransaction<'_>> {
```

Change the signature line only (body — acquiring the lock and calling `Self::do_checkpoint` — stays unchanged):

```rust
    pub fn checkpoint(&self) -> Result<()> {
```

to:

```rust
    pub fn checkpoint(&self) -> Result<(), MinigrafError> {
        self.checkpoint_inner().map_err(MinigrafError::from)
    }

    fn checkpoint_inner(&self) -> Result<()> {
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --test error_codes_foundation_test -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Run the full test suite**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/db.rs tests/error_codes_foundation_test.rs
git commit -m "$(cat <<'EOF'
feat: migrate Minigraf::begin_write/checkpoint to MinigrafError

Signature-only changes; bodies renamed to _inner twins, unchanged.
Part of #277.
EOF
)"
```

---

## Task 8: Migrate `Minigraf::prepare`, `register_aggregate`, `register_predicate`

**Files:**
- Modify: `src/db.rs`
- Modify: `tests/error_codes_foundation_test.rs`

**Interfaces:**
- Consumes: `MinigrafError` (Task 3).
- Produces: `Minigraf::prepare` returns `Result<PreparedQuery, MinigrafError>`; `register_aggregate`/`register_predicate` return `Result<(), MinigrafError>`. Private `_inner` twins keep the original bodies (including `register_aggregate`'s generic parameter and `where` clause).

- [ ] **Step 1: Write the failing test**

Add to `tests/error_codes_foundation_test.rs`:

```rust
#[test]
fn prepare_and_register_predicate_return_ok() {
    use minigraf::Value;

    let db = Minigraf::in_memory().unwrap();
    let prepared = db.prepare("(query [:find ?e :where [?e :name $name]])");
    assert!(prepared.is_ok(), "preparing a valid query should succeed");

    let registered = db.register_predicate("even277?", |v: &Value| {
        matches!(v, Value::Integer(i) if i % 2 == 0)
    });
    assert!(registered.is_ok(), "registering a new predicate name should succeed");
}
```

- [ ] **Step 2: Run test to verify it fails to compile**

Run: `cargo test --test error_codes_foundation_test -- --nocapture`
Expected: compile error — `prepare`/`register_predicate` still return `anyhow::Result<_>`.

- [ ] **Step 3: Migrate `prepare`**

Change the signature line only (body — parsing `query_str`, matching on `cmd`, calling `prepare_query` — stays unchanged):

```rust
    pub fn prepare(
        &self,
        query_str: &str,
    ) -> Result<crate::query::datalog::prepared::PreparedQuery> {
```

to:

```rust
    pub fn prepare(
        &self,
        query_str: &str,
    ) -> Result<crate::query::datalog::prepared::PreparedQuery, MinigrafError> {
        self.prepare_inner(query_str).map_err(MinigrafError::from)
    }

    fn prepare_inner(
        &self,
        query_str: &str,
    ) -> Result<crate::query::datalog::prepared::PreparedQuery> {
```

- [ ] **Step 4: Migrate `register_aggregate`**

Change the signature block only (body — building `init_boxed`/`step_boxed`/`finalise_boxed` and calling `register_aggregate_desc` — stays unchanged):

```rust
    pub fn register_aggregate<Acc>(
        &self,
        name: &str,
        init: impl Fn() -> Acc + Send + Sync + 'static,
        step: impl Fn(&mut Acc, &Value) + Send + Sync + 'static,
        finalise: impl Fn(&Acc, usize) -> Value + Send + Sync + 'static,
    ) -> Result<()>
    where
        Acc: Any + Send + 'static,
    {
```

to:

```rust
    pub fn register_aggregate<Acc>(
        &self,
        name: &str,
        init: impl Fn() -> Acc + Send + Sync + 'static,
        step: impl Fn(&mut Acc, &Value) + Send + Sync + 'static,
        finalise: impl Fn(&Acc, usize) -> Value + Send + Sync + 'static,
    ) -> Result<(), MinigrafError>
    where
        Acc: Any + Send + 'static,
    {
        self.register_aggregate_inner(name, init, step, finalise)
            .map_err(MinigrafError::from)
    }

    fn register_aggregate_inner<Acc>(
        &self,
        name: &str,
        init: impl Fn() -> Acc + Send + Sync + 'static,
        step: impl Fn(&mut Acc, &Value) + Send + Sync + 'static,
        finalise: impl Fn(&Acc, usize) -> Value + Send + Sync + 'static,
    ) -> Result<()>
    where
        Acc: Any + Send + 'static,
    {
```

- [ ] **Step 5: Migrate `register_predicate`**

Change the signature line only (body — building `desc` and calling `register_predicate_desc` — stays unchanged):

```rust
    pub fn register_predicate(
        &self,
        name: &str,
        f: impl Fn(&Value) -> bool + Send + Sync + 'static,
    ) -> Result<()> {
```

to:

```rust
    pub fn register_predicate(
        &self,
        name: &str,
        f: impl Fn(&Value) -> bool + Send + Sync + 'static,
    ) -> Result<(), MinigrafError> {
        self.register_predicate_inner(name, f).map_err(MinigrafError::from)
    }

    fn register_predicate_inner(
        &self,
        name: &str,
        f: impl Fn(&Value) -> bool + Send + Sync + 'static,
    ) -> Result<()> {
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test --test error_codes_foundation_test -- --nocapture`
Expected: PASS.

- [ ] **Step 7: Run the full test suite, including doctests**

Run: `cargo test`
Expected: PASS. `register_aggregate`'s and `register_predicate`'s doc-comment examples both call `.unwrap()` on the result — these keep compiling unchanged since `MinigrafError: Debug` (required by `Result::unwrap`).

- [ ] **Step 8: Commit**

```bash
git add src/db.rs tests/error_codes_foundation_test.rs
git commit -m "$(cat <<'EOF'
feat: migrate Minigraf::prepare/register_aggregate/register_predicate

Signature-only changes; bodies renamed to _inner twins, unchanged.
Part of #277.
EOF
)"
```

---

## Task 9: Migrate `WriteTransaction::execute` and `WriteTransaction::commit`

**Files:**
- Modify: `src/db.rs`
- Modify: `tests/error_codes_foundation_test.rs`

**Interfaces:**
- Consumes: `MinigrafError` (Task 3).
- Produces: `WriteTransaction::execute` returns `Result<QueryResult, MinigrafError>`; `WriteTransaction::commit` returns `Result<(), MinigrafError>`. Private `_inner` twins keep the original bodies (still calling the unchanged private `execute_read_command`/`execute_rule_command`/`wal_write_stamped_batch` helpers).

- [ ] **Step 1: Write the failing test**

Add to `tests/error_codes_foundation_test.rs`:

```rust
#[test]
fn write_transaction_execute_and_commit_return_ok() {
    let db = Minigraf::in_memory().unwrap();
    let mut tx = db.begin_write().unwrap();
    let exec = tx.execute(r#"(transact [[#uuid "550e8400-e29b-41d4-a716-446655440001" :name "bob"]])"#);
    assert!(exec.is_ok(), "staging a valid transact in a tx should succeed");
    let commit = tx.commit();
    assert!(commit.is_ok(), "committing a valid transaction should succeed");
}
```

- [ ] **Step 2: Run test to verify it fails to compile**

Run: `cargo test --test error_codes_foundation_test -- --nocapture`
Expected: compile error — both methods still return `anyhow::Result<_>`.

- [ ] **Step 3: Migrate `execute`**

Change the signature line only (body — parsing, matching on `cmd`, `stage_pending_facts`, `execute_read_command`/`execute_rule_command` calls — stays unchanged):

```rust
    pub fn execute(&mut self, input: &str) -> Result<QueryResult> {
```

to:

```rust
    pub fn execute(&mut self, input: &str) -> Result<QueryResult, MinigrafError> {
        self.execute_inner(input).map_err(MinigrafError::from)
    }

    fn execute_inner(&mut self, input: &str) -> Result<QueryResult> {
```

- [ ] **Step 4: Migrate `commit`**

Change the signature line only (body — staging, WAL write, applying facts, checkpoint trigger — stays unchanged):

```rust
    pub fn commit(mut self) -> Result<()> {
```

to:

```rust
    pub fn commit(mut self) -> Result<(), MinigrafError> {
        self.commit_inner().map_err(MinigrafError::from)
    }

    fn commit_inner(mut self) -> Result<()> {
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test --test error_codes_foundation_test -- --nocapture`
Expected: PASS.

- [ ] **Step 6: Run the full test suite**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/db.rs tests/error_codes_foundation_test.rs
git commit -m "$(cat <<'EOF'
feat: migrate WriteTransaction::execute/commit to MinigrafError

Signature-only changes; bodies renamed to _inner twins, unchanged.
Part of #277.
EOF
)"
```

---

## Task 10: Migrate `PreparedQuery::execute`

**Files:**
- Modify: `src/query/datalog/prepared.rs`
- Modify: `tests/error_codes_foundation_test.rs`

**Interfaces:**
- Consumes: `MinigrafError` (Task 3).
- Produces: `PreparedQuery::execute` returns `Result<QueryResult, MinigrafError>`. Private `PreparedQuery::execute_inner` keeps the original body, returning `anyhow::Result<QueryResult>`.

- [ ] **Step 1: Write the failing test**

Add to `tests/error_codes_foundation_test.rs`:

```rust
use minigraf::{BindValue, Value};

#[test]
fn prepared_query_execute_returns_ok() {
    let db = Minigraf::in_memory().unwrap();
    db.execute(r#"(transact [[:carol :person/name "Carol"]])"#).unwrap();
    let prepared = db
        .prepare("(query [:find ?e :where [?e :person/name $name]])")
        .unwrap();
    let result = prepared.execute(&[("name", BindValue::Val(Value::String("Carol".to_string())))]);
    assert!(result.is_ok(), "executing a prepared query with a valid binding should succeed");
}
```

(Matches the exact pattern of `tests/prepared_statements_test.rs`'s `prepare_and_execute_value_slot` test: vector-form `(query [:find ... :where ...])`, a keyword ident entity, and `BindValue::Val(Value::String(...))` for the bind slot — there is no `From<&str>` on `BindValue`.)

- [ ] **Step 2: Run test to verify it fails to compile**

Run: `cargo test --test error_codes_foundation_test -- --nocapture`
Expected: compile error — `PreparedQuery::execute` still returns `anyhow::Result<QueryResult>`.

- [ ] **Step 3: Migrate `execute`**

In `src/query/datalog/prepared.rs`, add `use crate::error::MinigrafError;` to the top-level `use` block (next to the existing `use anyhow::Result;`).

Change the signature line only (body stays unchanged):

```rust
    pub fn execute(&self, bindings: &[(&str, BindValue)]) -> Result<QueryResult> {
```

to:

```rust
    pub fn execute(&self, bindings: &[(&str, BindValue)]) -> Result<QueryResult, MinigrafError> {
        self.execute_inner(bindings).map_err(MinigrafError::from)
    }

    fn execute_inner(&self, bindings: &[(&str, BindValue)]) -> Result<QueryResult> {
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --test error_codes_foundation_test -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Run the full test suite**

Run: `cargo test`
Expected: PASS. Check `tests/prepared_statements_test.rs` specifically — it's the file most likely to exercise `PreparedQuery::execute`'s error path.

- [ ] **Step 6: Commit**

```bash
git add src/query/datalog/prepared.rs tests/error_codes_foundation_test.rs
git commit -m "$(cat <<'EOF'
feat: migrate PreparedQuery::execute to MinigrafError

Signature-only change; body renamed to execute_inner, unchanged.
This completes the public API signature migration for #277's
foundation PR. Part of #277.
EOF
)"
```

---

## Task 11: Full verification pass

**Files:** none (verification only; fixes applied wherever needed if something breaks)

**Interfaces:**
- Consumes: everything from Tasks 1–10.
- Produces: a crate that passes every check the pre-push hook and CI run.

- [ ] **Step 1: Format check**

Run: `cargo fmt -- --check`
Expected: no diff. If there is one, run `cargo fmt` (no `-- --check`) to apply it, then re-run the check.

- [ ] **Step 2: Clippy, matching CI exactly**

Run: `cargo clippy --all-features -- -D warnings`
Expected: no warnings. Fix anything flagged (likely candidates: an unused `Result` import if a file's only remaining anyhow-typed usage was removed, or an `#[allow(...)]` needed on an `_inner` fn if clippy flags the rename pattern — investigate and fix root causes, do not silence with a blanket `#[allow]`).

- [ ] **Step 3: Full test suite**

Run: `cargo test`
Expected: PASS, same or greater test count than before this plan started (`cargo test 2>&1 | tail -5` — compare against the pre-change baseline of 1023 tests noted in `CLAUDE.md`; the 8 new tests in `error_codes_foundation_test.rs` plus 12 in `src/error.rs`'s own module bring the count up by 20).

- [ ] **Step 4: WASM target build check (best-effort)**

Run: `rustup target list --installed | grep wasm32-unknown-unknown`
If installed, run: `cargo build --target wasm32-unknown-unknown --no-default-features --features wasm` (check `Cargo.toml`'s `[features]` section first for the exact wasm feature name if this fails) and confirm it compiles. If the target isn't installed, skip this step and note it in the PR description as untested locally — CI's WASM job will catch any regression.

- [ ] **Step 5: Grep for any remaining direct `anyhow::Result` usage on the functions this plan migrated, to confirm nothing was missed**

Run: `grep -n "pub fn open\|pub fn in_memory\|pub fn execute\|pub fn begin_write\|pub fn checkpoint\|pub fn prepare\|pub fn register_aggregate\|pub fn register_predicate\|pub fn commit" src/db.rs src/query/datalog/prepared.rs`
Expected: every line shown returns `Result<_, MinigrafError>`, not bare `Result<_>`.

- [ ] **Step 6: Commit any fixes from Steps 1–3** (only if something needed fixing; skip if everything was already clean)

```bash
git add -A
git commit -m "$(cat <<'EOF'
fix: address fmt/clippy/test fallout from the MinigrafError migration

Part of #277.
EOF
)"
```

---

## Task 12: CHANGELOG entry

**Files:**
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: nothing new.
- Produces: an `## [Unreleased]` (or equivalent pending-release heading — match whatever heading the file currently uses for not-yet-released changes) entry documenting the breaking change.

- [ ] **Step 1: Read the current `CHANGELOG.md` header structure**

Run: `head -40 CHANGELOG.md`
Confirm the exact heading text used for unreleased/pending changes and the existing entry format (bullet style, whether "Breaking" changes get their own subsection) before writing the new entry, so it matches surrounding entries exactly rather than guessing.

- [ ] **Step 2: Add the entry**

Add a bullet under the appropriate pending-release/breaking-changes heading, following the file's existing format for a breaking-change entry (e.g. how the `OpenOptions` field addition or the kernel-locking change were recorded):

```markdown
- **Breaking**: `Minigraf`'s and `WriteTransaction`'s and `PreparedQuery`'s public
  methods now return `Result<T, MinigrafError>` instead of `anyhow::Result<T>`.
  `MinigrafError` implements `std::error::Error` (so `?` still works against
  `anyhow::Result`/`Box<dyn Error>` callers) and adds `.category() -> ErrorCategory`
  and `.code() -> &str` for structured matching. This lands the foundation for
  [#277](https://github.com/project-minigraf/minigraf/issues/277); every error
  currently surfaces as the generic `INT-000` code until each error category's
  follow-up PR wires up its real `docs/ERROR_REFERENCE.md` codes.
```

- [ ] **Step 3: Verify the doc builds/renders sanely**

Run: `cargo test` (doctests in `CHANGELOG.md` are not run, but this re-confirms nothing else broke from the `CHANGELOG.md`-adjacent edit — e.g. if any test reads `CHANGELOG.md`'s content, such as a version-sync test)
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add CHANGELOG.md
git commit -m "$(cat <<'EOF'
docs: add CHANGELOG entry for the MinigrafError breaking change

Part of #277.
EOF
)"
```

- [ ] **Step 5: Report completion, ready for PR**

This is the last task of the Foundation PR. At this point the branch is ready for `git push` and PR creation, per [[feedback_pr_ownership]] (own the PR until CI is green) and [[feedback_no_closing_keywords_umbrella]] (the PR title/body/commits must say "part of #277" or "toward #277" — never a closing keyword against #277, since #277 is the umbrella issue for all 6 planned PRs and must stay open until the other 5 land).
