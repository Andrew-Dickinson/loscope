//! Integration test for `analysis::memory_paranoid`'s panic behavior. Runs as a real subprocess
//! rather than calling scope()/check() in-process, because `enabled()` caches
//! `LOS_MEMORY_PARANOID_MODE`'s value in a process-wide `OnceLock` on first read -- flipping the
//! env var mid-process would race with (and be unreliable across) every other test in this
//! binary. Instead each outer test re-execs itself (`std::env::current_exe()`) as a child
//! process with the env var set, selecting just one inner test function via `--exact`; the
//! child's exit status tells us whether that inner test's `check()` call panicked.

use loscope::analysis::memory_paranoid;
use loscope::util::env::LOS_MEMORY_PARANOID_MODE;
use std::env;
use std::process::Command;

const INNER_MARKER_ENV: &str = "LOS_MEMORY_PARANOID_MODE_TEST_INNER";

fn run_as_subprocess(inner_test_name: &str) -> std::process::Output {
    Command::new(env::current_exe().unwrap())
        .arg(inner_test_name)
        .arg("--exact")
        .arg("--nocapture")
        .env(LOS_MEMORY_PARANOID_MODE, "1")
        .env(INNER_MARKER_ENV, "1")
        .output()
        .expect("failed to spawn self as subprocess")
}

fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(fut)
}

#[test]
fn panics_when_actual_bytes_exceed_reservation() {
    if env::var(INNER_MARKER_ENV).is_ok() {
        block_on(memory_paranoid::scope("test_endpoint_over_budget", 100, async {
            memory_paranoid::check("first_allocation", 60);
            memory_paranoid::check("second_allocation", 90); // 150 > 100 reserved -- must panic
        }));
        return;
    }

    let output = run_as_subprocess("panics_when_actual_bytes_exceed_reservation");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.status.success(),
        "expected the subprocess to fail (panic inside check()), but it exited successfully.\n{combined}"
    );
    assert!(
        combined.contains("LOS_MEMORY_PARANOID_MODE"),
        "expected panic output to mention LOS_MEMORY_PARANOID_MODE, got:\n{combined}"
    );
}

#[test]
fn does_not_panic_when_actual_bytes_stay_within_reservation() {
    if env::var(INNER_MARKER_ENV).is_ok() {
        block_on(memory_paranoid::scope("test_endpoint_within_budget", 100, async {
            memory_paranoid::check("first_allocation", 40);
            memory_paranoid::check("second_allocation", 50); // 90 <= 100 reserved -- must not panic
        }));
        return;
    }

    let output = run_as_subprocess("does_not_panic_when_actual_bytes_stay_within_reservation");
    assert!(
        output.status.success(),
        "expected the subprocess to exit cleanly.\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn check_outside_any_scope_warns_but_does_not_panic() {
    if env::var(INNER_MARKER_ENV).is_ok() {
        // No memory_paranoid::scope() wraps this -- a coverage gap, not a violation.
        memory_paranoid::check("unscoped_allocation", u64::MAX);
        return;
    }

    let output = run_as_subprocess("check_outside_any_scope_warns_but_does_not_panic");
    assert!(
        output.status.success(),
        "expected the subprocess to exit cleanly.\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("no active memory_paranoid::scope()"),
        "expected a coverage-gap warning, got:\n{combined}"
    );
}
