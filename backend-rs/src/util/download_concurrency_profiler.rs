//! Temporary diagnostic, opt-in background sampler that records the number of asset downloads
//! `CachingAssetProvider` currently has in flight (i.e. past a confirmed cache miss and lock
//! acquisition, actively fetching from upstream) at high frequency.
//!
//! Exists to confirm/measure a hypothesis: that per-request download concurrency limits
//! (`PER_LOAD_TILES_CALL_CONCURRENCY_LIMIT_*` in analysis/tiles.rs) aren't coordinated *across*
//! concurrent requests, so overlapping requests against a cold cache could pile up far more
//! simultaneous in-flight downloads than any single request's limits would suggest -- see
//! `providers::backends::fs_cache::in_flight_downloads`.
//!
//! Entirely inert unless `LOS_DOWNLOAD_CONCURRENCY_PROFILE_PATH` is set. To profile a run:
//!
//!   LOS_DOWNLOAD_CONCURRENCY_PROFILE_PATH=/tmp/download_concurrency.csv \
//!   LOS_DOWNLOAD_CONCURRENCY_PROFILE_INTERVAL_MS=5 \
//!   cargo run
//!
//! then drive real traffic (ideally against a cold cache) and inspect the CSV afterward.

use crate::providers::backends::fs_cache::in_flight_downloads;
use crate::util::env::{
    LOS_DOWNLOAD_CONCURRENCY_PROFILE_INTERVAL_MS, LOS_DOWNLOAD_CONCURRENCY_PROFILE_PATH, get_env,
};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Sampling interval used when `LOS_DOWNLOAD_CONCURRENCY_PROFILE_INTERVAL_MS` isn't set.
const DEFAULT_INTERVAL_MS: u64 = 5;

/// Starts the background sampling thread if `LOS_DOWNLOAD_CONCURRENCY_PROFILE_PATH` is set in
/// the environment; otherwise does nothing.
///
/// Runs on a dedicated OS thread (like `util::memory_profiler`) rather than a tokio task
/// deliberately: the whole point is to observe behavior under real request load, and a tokio
/// task's scheduling can be delayed by exactly the kind of load (many concurrent request-
/// handling tasks) this is meant to measure.
pub fn start_if_configured() {
    let Some(path) = get_env(LOS_DOWNLOAD_CONCURRENCY_PROFILE_PATH) else {
        return;
    };
    let interval_ms: u64 = get_env(LOS_DOWNLOAD_CONCURRENCY_PROFILE_INTERVAL_MS)
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_INTERVAL_MS);

    let file = File::create(&path).unwrap_or_else(|err| {
        panic!("failed to create download concurrency profile output file {path}: {err}")
    });
    let mut writer = BufWriter::new(file);
    writeln!(writer, "unix_ms,in_flight_downloads")
        .expect("failed to write download concurrency profile CSV header");
    writer.flush().expect("failed to flush download concurrency profile CSV header");

    println!(
        "Download concurrency profiling enabled: sampling every {interval_ms}ms into {path} \
         (set {LOS_DOWNLOAD_CONCURRENCY_PROFILE_INTERVAL_MS} to change the interval)"
    );

    std::thread::Builder::new()
        .name("download-concurrency-profiler".into())
        .spawn(move || run_sampling_loop(writer, Duration::from_millis(interval_ms)))
        .expect("failed to spawn download-concurrency-profiler thread");
}

fn run_sampling_loop(mut writer: BufWriter<File>, interval: Duration) {
    loop {
        let count = in_flight_downloads();
        let unix_ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();

        // Flushed every row for the same reason as util::memory_profiler: this thread has no
        // shutdown hook, so an unceremoniously-killed process loses at most the in-flight row.
        if writeln!(writer, "{unix_ms},{count}").is_err() || writer.flush().is_err() {
            break;
        }

        std::thread::sleep(interval);
    }
}
