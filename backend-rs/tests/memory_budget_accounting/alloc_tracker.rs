// A process-wide counting allocator used to measure *actual* peak heap growth of a call, so it
// can be checked against what `memory_estimate.rs` predicted for the same input. This is the
// core primitive the whole `memory_budget_accounting` suite is built on: every property test
// ultimately calls `measure`/`measure_async` around a real (not mocked-out) allocation-heavy
// function and compares the returned `delta_bytes` against the corresponding estimator.
//
// This has to be a `#[global_allocator]` (not e.g. a per-call wrapper) because Rust has no way to
// intercept allocations scoped to a closure otherwise. That means it counts *every* allocation in
// this test binary's process, including tokio runtime bookkeeping and proptest's own machinery.
// We deal with that by (a) reusing one lazily-built current-thread runtime instead of constructing
// a fresh one per measurement, and (b) taking a baseline snapshot of `CURRENT` immediately before
// the measured section and reporting only the growth above that baseline, so pre-existing resident
// memory (and slow, amortized-away background growth) doesn't get attributed to the call under
// measurement.
//
// Measurements are serialized by `MEASURE_LOCK` so that concurrent `#[test]` threads (or proptest
// cases within a single `#[test]`, though proptest itself already runs cases sequentially within
// one test function) never observe each other's allocations as spurious peaks. This makes the
// measurement correct regardless of `--test-threads`, though running with `--test-threads=1` is
// still recommended to avoid tests blocking on each other's lock and to keep failure output
// attributable to a single test at a time.

use std::alloc::{GlobalAlloc, Layout, System};
use std::future::Future;
use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

struct CountingAllocator;

static CURRENT_BYTES: AtomicIsize = AtomicIsize::new(0);
static PEAK_BYTES: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            track_alloc(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            track_alloc(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        track_dealloc(layout.size());
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            track_dealloc(layout.size());
            track_alloc(new_size);
        }
        new_ptr
    }
}

fn track_alloc(size: usize) {
    let prev = CURRENT_BYTES.fetch_add(size as isize, Ordering::SeqCst);
    let new_val = prev + size as isize;
    PEAK_BYTES.fetch_max(new_val.max(0) as usize, Ordering::SeqCst);
}

fn track_dealloc(size: usize) {
    CURRENT_BYTES.fetch_sub(size as isize, Ordering::SeqCst);
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

static MEASURE_LOCK: Mutex<()> = Mutex::new(());

/// What a single measurement window observed.
#[derive(Debug, Clone, Copy)]
pub struct Sample {
    /// Peak bytes allocated above the pre-call baseline during the measured window. This is the
    /// number to compare against a `memory_estimate.rs` estimator's output.
    pub delta_bytes: u64,
    /// `CURRENT_BYTES` immediately before the measured section started, for diagnostics.
    #[allow(dead_code)]
    pub baseline_bytes: u64,
}

fn reset_peak_to_baseline() -> u64 {
    let baseline = CURRENT_BYTES.load(Ordering::SeqCst).max(0) as usize;
    PEAK_BYTES.store(baseline, Ordering::SeqCst);
    baseline as u64
}

fn take_sample(baseline: u64) -> Sample {
    let peak = PEAK_BYTES.load(Ordering::SeqCst) as u64;
    Sample { delta_bytes: peak.saturating_sub(baseline), baseline_bytes: baseline }
}

/// Measures the peak heap growth (above baseline) of a synchronous closure.
pub fn measure<T>(f: impl FnOnce() -> T) -> (T, Sample) {
    let _guard = MEASURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let baseline = reset_peak_to_baseline();
    let result = f();
    (result, take_sample(baseline))
}

fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build measurement tokio runtime")
    })
}

/// Measures the peak heap growth (above baseline) of driving a future to completion on a shared,
/// lazily-initialized current-thread runtime (reused across calls so runtime construction itself
/// doesn't pollute the measurement).
pub fn measure_async<T>(fut: impl Future<Output = T>) -> (T, Sample) {
    let _guard = MEASURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let baseline = reset_peak_to_baseline();
    let result = runtime().block_on(fut);
    (result, take_sample(baseline))
}

/// Drives a future to completion on the same shared runtime `measure_async` uses, without taking
/// a measurement -- for setup/helper calls (e.g. computing an estimate before the real measured
/// section) where the allocation isn't the thing under test.
pub fn block_on<T>(fut: impl Future<Output = T>) -> T {
    runtime().block_on(fut)
}
