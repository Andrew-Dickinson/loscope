//! Optional, opt-in background sampler that records the memory-budget system's *reserved* bytes
//! alongside the process's *actual* resident memory (RSS) at high frequency, so the two can be
//! plotted against each other after a run under real load: does what the budget thinks is
//! reserved actually track what the OS reports the process is using?
//!
//! Entirely inert unless `LOS_MEMORY_PROFILE_PATH` is set — `start_if_configured` is a no-op in
//! that case, so this has zero cost/behavior change in normal operation. To profile a run:
//!
//!   LOS_MEMORY_PROFILE_PATH=/tmp/memory_profile.csv \
//!   LOS_MEMORY_PROFILE_INTERVAL_MS=5 \
//!   cargo run
//!
//! then drive real traffic at the server and inspect the CSV (or chart it) once done.

use crate::analysis::memory_budget::MemoryBudget;
use crate::util::env::{LOS_MEMORY_PROFILE_INTERVAL_MS, LOS_MEMORY_PROFILE_PATH, get_env};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

/// Sampling interval used when `LOS_MEMORY_PROFILE_INTERVAL_MS` isn't set — comfortably under the
/// "10ms or less" this tool was built to satisfy, with a little headroom for jitter.
const DEFAULT_INTERVAL_MS: u64 = 5;

/// Starts the background sampling thread if `LOS_MEMORY_PROFILE_PATH` is set in the environment;
/// otherwise does nothing. `memory_budget` should be the same instance (or a `clone()` of it --
/// `MemoryBudget` shares its underlying counter across clones) managed by the running server, so
/// the sampled `reserved_bytes` reflects real, live reservations rather than an independent copy.
///
/// Runs on a dedicated OS thread rather than a tokio task deliberately: the entire point is to
/// observe behavior under real load, and a tokio task's scheduling can be delayed by exactly the
/// kind of load (many concurrent request-handling tasks) this is meant to measure. An OS thread
/// sampling via `std::thread::sleep` keeps the cadence independent of async executor contention.
pub fn start_if_configured(memory_budget: MemoryBudget) {
    let Some(path) = get_env(LOS_MEMORY_PROFILE_PATH) else {
        return;
    };
    let interval_ms: u64 = get_env(LOS_MEMORY_PROFILE_INTERVAL_MS)
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_INTERVAL_MS);

    let file = File::create(&path)
        .unwrap_or_else(|err| panic!("failed to create memory profile output file {path}: {err}"));
    let mut writer = BufWriter::new(file);
    writeln!(writer, "unix_ms,reserved_bytes,rss_bytes,limit_bytes")
        .expect("failed to write memory profile CSV header");
    writer.flush().expect("failed to flush memory profile CSV header");

    println!(
        "Memory profiling enabled: sampling every {interval_ms}ms into {path} \
         (set {LOS_MEMORY_PROFILE_INTERVAL_MS} to change the interval)"
    );

    std::thread::Builder::new()
        .name("memory-profiler".into())
        .spawn(move || run_sampling_loop(memory_budget, writer, Duration::from_millis(interval_ms)))
        .expect("failed to spawn memory-profiler thread");
}

fn run_sampling_loop(memory_budget: MemoryBudget, mut writer: BufWriter<File>, interval: Duration) {
    let pid = sysinfo::get_current_pid()
        .expect("failed to determine this process's own pid for memory profiling");
    let mut sys = System::new();
    let limit_bytes = memory_budget.limit_bytes();
    let refresh_kind = ProcessRefreshKind::nothing().with_memory();

    loop {
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(std::slice::from_ref(&pid)),
            false,
            refresh_kind,
        );
        let rss_bytes = sys.process(pid).map(|p| p.memory()).unwrap_or(0);
        let reserved_bytes = memory_budget.reserved_bytes();
        let unix_ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();

        // Flushed every row (not just buffered) so a killed-not-terminated process (this thread
        // has no shutdown hook -- the whole point is to run for the server's lifetime) loses at
        // most the in-flight row, not an arbitrarily large unflushed tail. At a 5-10ms cadence
        // this is a small, local file write; it doesn't meaningfully perturb the interval.
        if writeln!(writer, "{unix_ms},{reserved_bytes},{rss_bytes},{limit_bytes}").is_err()
            || writer.flush().is_err()
        {
            // Output path became unwritable (disk full, file removed, etc) -- stop sampling
            // rather than spin forever failing to write; the server itself is unaffected.
            break;
        }

        std::thread::sleep(interval);
    }
}
