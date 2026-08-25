# Structured Runtime Error Codes — Design Spec

**Issue**: [#277](https://github.com/project-minigraf/minigraf/issues/277)
**Status**: Approved, pending implementation plan
**Date**: 2026-08-25

## Background

`docs/ERROR_REFERENCE.md` documents 130 user-facing error codes across five
categories (PRS 79, STG 27, QRY 9, WAL 6, API 9), each with an exact "Error
text" string, but the codes are documentation-only — they never appear in
actual runtime output. All errors currently flow through `anyhow` with plain
string messages (21 files, ~286 `bail!`/`anyhow!` call sites).

This issue is deliberately sequenced last in the v1.3.0 milestone because it
depends on the final shape of every other fix's error paths. Its prerequisite
(#352, documenting #303/#305's new error messages) is merged. #309 (the other
blocking-adjacent issue) closed as by-design. #277 is unblocked.

## Goals

- Every error returned from Minigraf's public API carries a stable code
  (e.g. `PRS-001`) and a coarse category, matchable by library consumers.
- Runtime `Display` output includes the code: `[PRS-001] Unexpected end of input`.
- Codes are stable across versions independent of message text changes.
- `ERROR_REFERENCE.md` stays in sync with runtime codes, verified by a test.
- Internal call sites keep today's `anyhow`-based ergonomics (`?`, `.context()`)
  with minimal diff; only the public API boundary changes shape.

## Non-goals

- Replacing `anyhow` internally. It stays the workhorse for error propagation
  and context chaining inside the crate.
- A full enum variant per code (130+ variants). Explored and rejected — see
  Alternatives.
- Auto-generating the Rust code registry from `ERROR_REFERENCE.md` (or vice
  versa). A sync *test* is sufficient and avoids build-time markdown parsing.

## Architecture

### `src/error.rs` (new module)

```rust
pub enum ErrorCategory { Parser, Query, Storage, Wal, Api, Internal }

pub struct MinigrafError {
    category: ErrorCategory,
    code: &'static str,      // e.g. "PRS-001"
    message: String,
    source: anyhow::Error,
}
```

`MinigrafError` implements `Display` (`"[{code}] {message}"`) and
`std::error::Error` (via `source()` delegating to the wrapped `anyhow::Error`
chain). No `thiserror` dependency is added — the impl is a dozen lines by
hand, consistent with the self-contained/minimal-dependencies philosophy
principle (thiserror isn't a current dependency).

`ErrorCategory` is the "typed variant" consumers match on (6 variants,
matches the acceptance criterion). `.code()` returns the specific string —
stable and independent of any internal enum layout, which matters for a
project committing to decades-long compatibility.

### `pub(crate) enum ErrorCode` and the registry

```rust
pub(crate) enum ErrorCode { Prs001, Prs002, /* ... */ Int001, /* ... */ }

pub(crate) const REGISTRY: &[(ErrorCode, &str, ErrorCategory)] = &[
    (ErrorCode::Prs001, "PRS-001", ErrorCategory::Parser),
    // ...
];
```

`ErrorCode` is crate-internal — a compile-time-checked handle used only at
call sites via the macros below. Consumers never see it, only the resulting
code string, which keeps the enum free to be reorganized without breaking
downstream `match` statements.

A new sixth category, **INT** (Internal), covers `bail!`/`anyhow!` sites that
are invariant violations rather than user-actionable errors (e.g. "write
lock poisoned", "unexpected command variant in write path" — several
existing API-0xx entries already describe this kind of thing and may get
reclassified as INT during migration). Every `MinigrafError` always has a
`code`; there is no `Option`, so no downstream consumer has to special-case
"no code."

### Call-site macros

```rust
bail_coded!(ErrorCode::Prs001, "unexpected end of input");
bail_coded!(ErrorCode::Prs002, "unexpected character: {}", ch);
let e = err_coded!(ErrorCode::Api008, "function registry lock poisoned");
```

Both build a small internal `CodedError { code: ErrorCode, message: String }`
(implements `std::error::Error`) and wrap it as `anyhow::Error` — a drop-in
replacement for existing `bail!(...)` / `anyhow!(...)` call sites, minimal
diff per site, and a compile error if the `ErrorCode` variant doesn't exist.
Everything downstream of the call site is untouched: functions keep
returning `anyhow::Result<T>`, `?` and `.context()` keep working, and
`CodedError` rides along in the error chain through however many hops it
takes to reach the public API boundary.

### Public API boundary conversion

Only `db.rs`'s `pub fn` signatures (plus `WriteTransaction`, `PreparedQuery`,
`Repl`) change from `anyhow::Result<T>` to `Result<T, MinigrafError>`.
Conversion happens once, in a single `From<anyhow::Error> for MinigrafError`
(or equivalent helper):

1. Walk `.chain()` on the incoming `anyhow::Error`.
2. Find the first `CodedError` via `downcast_ref`.
3. Look up its code/category in `REGISTRY`.
4. Build `MinigrafError { category, code, message: CodedError's message, source: original anyhow::Error }`.
5. If no `CodedError` is found anywhere in the chain (a raw `io::Error` or
   any call site not yet migrated), fall back to `ErrorCategory::Internal` /
   a catch-all `INT-000` code, using the anyhow error's `Display` as the
   message.

Step 5 is what makes the foundation PR safe to land with zero call sites
migrated: the crate compiles, the public signature changes, and every
existing error still round-trips (as `INT-000`) while categorized errors
are added incrementally in the follow-up PRs.

### Registry ↔ doc sync test

A test (likely in `tests/` or a `#[cfg(test)]` module in `error.rs`) parses
`ERROR_REFERENCE.md`'s Quick Reference Table and asserts its code list
matches `REGISTRY` exactly in both directions — every documented code has a
registry entry and vice versa. This is the acceptance criterion "codes
remain in sync," verified mechanically rather than by convention. INT codes
also get their normal `ERROR_REFERENCE.md` entries (a new `## INT — Internal
Errors` section), so the test's coverage is uniform across all 6 categories.

## Public API changes

- `Minigraf::execute`, `open`, `open_with_options`, `in_memory`,
  `in_memory_with_options`, `begin_write`, `checkpoint`, `prepare`,
  `register_aggregate`, `register_predicate` — return type becomes
  `Result<T, MinigrafError>`.
- `WriteTransaction::commit`, `execute` — same.
- `Repl::execute` and internal read/write command dispatch — same, since the
  REPL surfaces the same errors to interactive users.
- `MinigrafError` implements `std::error::Error`, so `?` still works in
  consumer code using `Box<dyn Error>` or `anyhow` themselves (anyhow accepts
  any `std::error::Error`).
- This is a breaking change to the public `Result` alias used throughout
  `db.rs`. It bundles into the version bump already pending from the
  `OpenOptions` field landed unreleased on main (deferred until after #324,
  which is done) — no new semver decision is introduced by this issue, it
  just adds to the same pending bump.

## Migration phasing

Per repo convention (mirrors how #352 was spun out ahead of #277), #277
becomes the umbrella tracking issue and splits into:

1. **Foundation PR** (`toward #277`, its own branch/worktree) — `error.rs`
   module, `ErrorCategory`, `ErrorCode`/`REGISTRY`, `bail_coded!`/
   `err_coded!` macros, boundary conversion with `INT-000` fallback, sync
   test (passes trivially since `REGISTRY` starts near-empty and the doc
   table isn't touched yet — grows meaningfully once codes start landing),
   `db.rs` signature migration. Zero call sites migrated; everything falls
   through to `INT-000`. Crate compiles, full test suite green.
2. **PRS sub-issue/PR** — migrate all parser call sites to `bail_coded!`
   with their real PRS-0xx codes; add regression tests asserting the
   specific code comes back from `db.execute()` for each.
3. **STG sub-issue/PR** — storage/file-format call sites, STG-0xx.
4. **WAL sub-issue/PR** — WAL call sites, WAL-0xx.
5. **QRY sub-issue/PR** — query executor call sites, QRY-0xx.
6. **API + INT sub-issue/PR** — remaining `db.rs`/API-layer call sites
   (API-0xx), plus auditing every leftover uncoded `bail!`/`anyhow!` site
   crate-wide and assigning INT-0xx codes (with new `ERROR_REFERENCE.md`
   entries) so nothing permanently falls through to the generic `INT-000`
   catch-all.

Steps 2–6 touch disjoint modules and could be parallelized after the
foundation PR lands, but each PR should still land and go green
independently (own worktree per [[feedback_worktree_before_impl]], own CI
per [[feedback_pr_ownership]]).

**Closing keywords**: per [[feedback_no_closing_keywords_umbrella]], none of
PRs 1–6's bodies or commit messages may use a GitHub closing keyword
(`fixes`/`closes`/`resolves`) against **#277** — reference it as plain text
("part of #277", "toward #277") only. Each sub-issue PR (2–6) may close its
own sub-issue number normally. #277 itself is closed manually once all six
land.

## Testing strategy

- Foundation PR: unit tests for `MinigrafError::Display` format, boundary
  conversion (`CodedError` found vs. not found → `INT-000` fallback), and
  the registry↔doc sync test (passing vacuously until codes are added).
- Each category PR: for every migrated call site, a regression test that
  triggers the error condition through the public API and asserts
  `err.code() == "PRS-NNN"` (or equivalent) — not just that an error
  occurred. Follows [[feedback_no_uuid_in_asserts]] (plain string/code
  assertions only, no `{:?}` on `Fact`/`Value`/`Uuid`-bearing types).
- Full `cargo test` must stay green after every PR (existing tests that
  currently match on `anyhow::Error` message substrings, if any, need
  updating to the new `Result<T, MinigrafError>` shape — audit for these
  during the foundation PR).

## Documentation updates

- `docs/ERROR_REFERENCE.md`: remove the "codes are documentation-only... do
  not appear in runtime output today" caveat once the foundation PR lands
  (codes start appearing incrementally as each category PR merges — caveat
  should probably become per-category or just removed once all 6 PRs are
  in); add the new `## INT — Internal Errors` section (step 6).
- `CHANGELOG.md`: entry for the breaking `Result<T, MinigrafError>` change,
  grouped with the other pending breaking changes already noted there.
- Wiki `Architecture.md`: new `src/error.rs` module entry.
- Per [[decision_v2_file_format]]-style doc-sync discipline (CLAUDE.md
  reminder #8): sync all of CLAUDE.md/ROADMAP.md/README.md/
  TEST_COVERAGE.md/CHANGELOG.md only once the whole v1.3.0 milestone
  (including #277) is complete and ready to release — not after each
  individual sub-PR.

## Alternatives considered

**Full typed enum hierarchy** (one variant per code, `thiserror`-based):
rejected — 130+ variants (growing over the project's decades-long lifetime)
is a large ongoing maintenance surface for an embedded library that values
stability over exhaustive type safety, and the migration touches all 286
call sites with structured data instead of format strings, a much bigger
diff than macro-based tagging.

**Lightweight anyhow tag, no signature change**: rejected — keeps
`anyhow::Result<T>` in `db.rs`'s public signatures, non-breaking, smallest
diff, but doesn't satisfy "match on specific variants": consumers would
have to downcast to one wrapper struct and read a string, which is what the
chosen design already does *in addition to* exposing a real matchable
`ErrorCategory` enum — so the chosen design strictly improves on this
alternative for roughly the same internal-plumbing cost.

**Optional `code()` for uncoded errors**: rejected in favor of the INT
category — every `MinigrafError` always having a code means consumers never
handle a `None` case, and it forces every error path to eventually get a
real (if generic) code and doc entry rather than staying permanently
uncoded.
