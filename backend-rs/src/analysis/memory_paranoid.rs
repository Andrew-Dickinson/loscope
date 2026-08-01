//! Debugging aid for `memory_budget`: when `LOS_MEMORY_PARANOID_MODE` is set, every
//! reservation-guarded endpoint tracks the real size of each non-trivial allocation it makes
//! and panics -- after logging the full breakdown -- the instant the running total exceeds what
//! was reserved for it. This exists to *confirm* (or rule out) a suspected bug in the estimator
//! formulas in `memory_estimate.rs`: those formulas are supposed to be a conservative upper
//! bound on real allocation, but if one of them is wrong, `memory_budget`'s atomic counter alone
//! has no way to notice -- it only ever sees the (possibly-wrong) estimate, never the real bytes
//! allocated. This module closes that gap by comparing the two directly, at the call sites that
//! actually allocate.
//!
//! Deliberately *not* a global-allocator hook: this process is multi-threaded async (Rocket on
//! tokio), so a process-wide byte counter can't tell one request's allocations apart from
//! another's without task-scoped attribution -- and a global allocator hook would also be blind
//! to the FFI (`openjpeg-sys`) buffers this codebase allocates via C `malloc`. Instead, callers
//! call `check()` by hand immediately after each allocation identified in the memory-budget
//! audit; this module only supplies the plumbing that lets `check()` find the request's active
//! reservation without threading a parameter through every intermediate function signature.
//!
//! That plumbing is a `tokio::task_local!`, not a plain thread-local: a plain thread-local would
//! misattribute allocations whenever the executor moves this task's continuation to a different
//! worker thread (or polls a different task on this thread) across an `.await` point. Tokio
//! swaps task-locals in/out around each poll of the owning task specifically so they survive
//! that move correctly -- see `scope()` below.

use crate::util::env::{LOS_MEMORY_PARANOID_MODE, get_env};
use std::cell::RefCell;
use std::collections::HashSet;
use std::future::Future;
use std::sync::{Mutex, OnceLock};

pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| get_env(LOS_MEMORY_PARANOID_MODE).is_some())
}

struct ParanoidState {
    endpoint: String,
    reserved_bytes: u64,
    actual_bytes: u64,
    breakdown: Vec<(String, u64)>,
}

tokio::task_local! {
    static CURRENT: RefCell<Option<ParanoidState>>;
}

/// Runs `fut` with paranoid tracking bound to `endpoint`/`reserved_bytes` for its duration, so
/// any `check()` call made anywhere underneath it -- however deep the call chain -- can find it.
/// Wrap this around the reservation-guarded portion of a handler (from just after `try_reserve`
/// through wherever the reservation would next be shrunk or dropped). A pure pass-through (no
/// task-local write at all) when paranoid mode is off, so this costs nothing in normal
/// operation.
pub async fn scope<F: Future>(endpoint: &str, reserved_bytes: u64, fut: F) -> F::Output {
    if !enabled() {
        return fut.await;
    }
    CURRENT
        .scope(
            RefCell::new(Some(ParanoidState {
                endpoint: endpoint.to_string(),
                reserved_bytes,
                actual_bytes: 0,
                breakdown: Vec::new(),
            })),
            fut,
        )
        .await
}

/// Call immediately after allocating a non-trivial data structure that the enclosing `scope()`'s
/// reservation is meant to cover. `label` should identify the call site (e.g.
/// `"compute_fresnel_zone::values"`) -- it's what shows up in the panic message and the logged
/// breakdown. No-op if paranoid mode is off. If paranoid mode is on but this is called outside
/// any `scope()` -- most likely because the allocation happens on a path that isn't covered by a
/// reservation at all -- logs a one-time warning per distinct label instead of panicking, since
/// that's a coverage gap to investigate, not necessarily a reservation violation.
pub fn check(label: &str, actual_bytes: u64) {
    if !enabled() {
        return;
    }
    let found_scope = CURRENT
        .try_with(|cell| {
            let mut state = cell.borrow_mut();
            let Some(state) = state.as_mut() else {
                return false;
            };
            state.actual_bytes += actual_bytes;
            state.breakdown.push((label.to_string(), actual_bytes));
            if state.actual_bytes > state.reserved_bytes {
                eprintln!(
                    "LOS_MEMORY_PARANOID_MODE VIOLATION: endpoint={} reserved_bytes={} \
                     actual_bytes={} exceeded-by={} offending_allocation={}={}b breakdown={:?}",
                    state.endpoint,
                    state.reserved_bytes,
                    state.actual_bytes,
                    state.actual_bytes - state.reserved_bytes,
                    label,
                    actual_bytes,
                    state.breakdown,
                );
                panic!(
                    "LOS_MEMORY_PARANOID_MODE: reservation exceeded on endpoint={}: \
                     reserved_bytes={} actual_bytes={} (offending allocation: {}={} bytes)",
                    state.endpoint, state.reserved_bytes, state.actual_bytes, label, actual_bytes
                );
            }
            true
        })
        .unwrap_or(false);
    if !found_scope {
        warn_uncovered(label);
    }
}

fn warn_uncovered(label: &str) {
    static WARNED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let warned = WARNED.get_or_init(|| Mutex::new(HashSet::new()));
    let mut warned = warned.lock().unwrap();
    if warned.insert(label.to_string()) {
        eprintln!(
            "LOS_MEMORY_PARANOID_MODE: check(\"{label}\") called with no active memory_paranoid::scope() \
             -- this allocation isn't covered by any tracked reservation. Either the enclosing \
             endpoint doesn't reserve memory for it, or scope() isn't wrapping this code path."
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // enabled() is only ever exercised indirectly in these tests via LOS_MEMORY_PARANOID_MODE
    // not being set in the test environment, so scope()/check() are no-ops below. Real coverage
    // of the panic/tracking behavior lives in tests/memory_paranoid_mode.rs, which sets the env
    // var in a subprocess (env vars are process-global, so flipping it in-process would race
    // with every other #[test] in this binary).

    #[tokio::test]
    async fn scope_is_a_pure_passthrough_when_disabled() {
        let result = scope("test_endpoint", 100, async { 42 }).await;
        assert_eq!(result, 42);
    }

    #[test]
    fn check_does_not_panic_when_disabled() {
        check("some_allocation", u64::MAX);
    }
}
