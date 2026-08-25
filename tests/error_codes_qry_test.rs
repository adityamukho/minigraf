//! Regression tests for issue #361 (part of #277): QRY-0xx error codes.
//!
//! Drives the full public `Minigraf::execute()` API and asserts the specific
//! `QRY-00N` code documented in `docs/ERROR_REFERENCE.md` comes back through
//! `MinigrafError::code()`.
//!
//! Coverage note: only QRY-007 (unknown predicate) and QRY-008/QRY-009
//! (function/rule registry lock poisoned) are reachable through
//! `Minigraf::execute()` for a *query* — that path constructs a
//! `DatalogExecutor` directly (see `src/db.rs` `execute_inner`'s read-only
//! branch). QRY-001..006 (transact/retract validation, wired in
//! `DatalogExecutor::execute_transact`/`execute_retract`) are NOT reachable
//! through `Minigraf::execute()` today: `execute_inner`'s write path routes
//! `transact`/`retract` commands through `Minigraf::materialize_transaction`/
//! `materialize_retraction` (src/db.rs) instead, for WAL-first-ordering
//! reasons, and never calls those `DatalogExecutor` methods. `db.rs`'s
//! near-duplicate messages there are already earmarked as API-003/API-004 in
//! ERROR_REFERENCE.md for the API+INT category PR (#277 step 6); QRY-001/
//! 004/005/006 currently have no `db.rs`-side equivalent at all. QRY-001..006
//! are still migrated and covered by unit tests in
//! `src/query/datalog/executor.rs` (constructing `DatalogExecutor` directly,
//! the same convention already used there for other parser-unreachable
//! branches) — see that file's "Stream 3: branches unreachable via the
//! parser" test group.
//!
//! This file also locks in current (INT-000) behavior for two error families
//! that live in this issue's in-scope files but have no documented QRY code:
//! the recursive-rule iteration/derived-fact/result limit family in
//! `src/query/datalog/evaluator.rs` (the "limit/depth-exceeded" and
//! "recursion-error" paths), and the runtime aggregate type-mismatch family,
//! which is raised in `src/query/datalog/functions.rs` — not one of this
//! issue's five in-scope files, so it's out of scope for migration here.

use minigraf::Minigraf;

// ─── QRY-007: unknown predicate ───────────────────────────────────────────

#[test]
fn query_unknown_predicate_returns_qry_007() {
    let db = Minigraf::in_memory().unwrap();
    db.execute(r#"(transact [[:a :x "hello"]])"#).unwrap();
    let result = db.execute(r#"(query [:find ?e :where [?e :x ?v] [(nosuchpred361? ?v)]])"#);
    let err = result.expect_err("unknown predicate must fail");
    assert_eq!(err.code(), "QRY-007");
}

#[test]
fn query_with_rules_unknown_predicate_returns_qry_007() {
    // Same check, but through the rules-aware execute_query_with_rules path
    // (a distinct call site from the plain execute_query path above).
    let db = Minigraf::in_memory().unwrap();
    db.execute(r#"(transact [[:a :x "hello"]])"#).unwrap();
    db.execute(r#"(rule [(p ?e) [?e :x ?v]])"#).unwrap();
    let result =
        db.execute(r#"(query [:find ?e :where (p ?e) [?e :x ?v] [(nosuchpred361b? ?v)]])"#);
    let err = result.expect_err("unknown predicate must fail");
    assert_eq!(err.code(), "QRY-007");
}

// ─── Limit/recursion-error family: no documented QRY code (INT-000) ───────

/// A recursive rule with an artificially tiny per-query `max_derived_facts`
/// hits `RecursiveEvaluator::evaluate_recursive_rules`'s "Max derived facts
/// per iteration exceeded" branch (src/query/datalog/evaluator.rs). No
/// QRY-0xx code is documented for this in ERROR_REFERENCE.md — see the
/// module doc comment above for why. This test locks in that it currently
/// (and, for the scope of this PR, deliberately) surfaces as INT-000 rather
/// than silently succeeding or panicking.
#[test]
fn recursive_rule_derived_facts_limit_returns_int000() {
    let db = Minigraf::in_memory().unwrap();
    db.execute(r#"(transact [[:a :connected :b] [:b :connected :c] [:c :connected :d]])"#)
        .unwrap();
    db.execute(r#"(rule [(reachable ?x ?y) [?x :connected ?y]])"#)
        .unwrap();
    db.execute(r#"(rule [(reachable ?x ?y) [?x :connected ?z] (reachable ?z ?y)])"#)
        .unwrap();
    let result =
        db.execute(r#"(query [:find ?x ?y :where (reachable ?x ?y) :max-derived-facts 1])"#);
    match result {
        Ok(_) => {
            // If per-query :max-derived-facts syntax isn't recognised (or
            // the limit isn't tight enough to trip on this tiny dataset),
            // this test isn't exercising the intended branch — investigate
            // rather than treat as a pass.
            panic!("expected the derived-facts limit to be exceeded");
        }
        Err(err) => assert_eq!(err.code(), "INT-000"),
    }
}

// ─── Runtime type-mismatch family: out of this issue's file scope ─────────

/// `sum` over a non-numeric attribute fails at query execution time. The
/// error originates in `apply_builtin_aggregate`
/// (src/query/datalog/functions.rs), which is NOT one of this issue's five
/// in-scope files (executor.rs, evaluator.rs, optimizer.rs, matcher.rs,
/// magic_sets.rs) — so it is intentionally left uncoded by this PR and
/// currently surfaces as INT-000. Locked in here as a known, documented gap
/// rather than left to silently regress.
#[test]
fn aggregate_type_mismatch_returns_int000() {
    let db = Minigraf::in_memory().unwrap();
    db.execute(r#"(transact [[:a :score "high"] [:b :score "low"]])"#)
        .unwrap();
    let result = db.execute(r#"(query [:find (sum ?s) :where [?e :score ?s]])"#);
    let err = result.expect_err("sum of strings must fail at runtime");
    assert_eq!(err.code(), "INT-000");
}
