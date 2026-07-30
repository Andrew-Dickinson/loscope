use crate::util::env::{LOS_MAX_ANALYSIS_MEMORY_BYTES, get_env};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

// Aligned with the `backend` service's container memory limit in docker-compose.yml (currently
// a 1280m hard limit / 819m reservation). This budget only covers analysis requests, so it's
// kept well under that hard limit to leave headroom for the process's baseline footprint,
// asset caches, and other concurrently-running memory-heavy endpoints (rooftop/tileview) that
// this budget doesn't track. If you change the container's memory limit, update
// LOS_MAX_ANALYSIS_MEMORY_BYTES in docker-compose.yml (and this default) to match.
const DEFAULT_MAX_ANALYSIS_MEMORY_BYTES: u64 = 512 * 1024 * 1024; // 512 MiB

/// Tracks estimated bytes reserved by in-flight requests against a configured ceiling, so we
/// can throttle new requests instead of letting the process OOM. One instance is shared
/// (Rocket-managed) across the whole server.
///
/// `reserved_bytes` is an `Arc` (not a plain field) so `Reservation` can hold an owned handle
/// to it rather than borrowing `&MemoryBudget`. Streaming endpoints (`TextStream!`) need their
/// reservation to outlive the handler function itself — it has to stay alive until the response
/// stream finishes — and Rocket's stream types can't capture a borrow of `&State<MemoryBudget>`
/// into that opaque future. An owned `Arc` clone sidesteps the borrow entirely.
#[derive(Debug)]
pub struct MemoryBudget {
    limit_bytes: u64,
    reserved_bytes: Arc<AtomicU64>,
}

#[derive(Debug)]
pub enum ReservationErr {
    /// This request's estimate alone exceeds the configured limit — no amount of waiting for
    /// other requests to finish will make it fit. Callers should surface this as a permanent
    /// failure, not something worth retrying.
    ExceedsLimit { estimate_bytes: u64, limit_bytes: u64 },
    /// The server doesn't have enough headroom right now because of other in-flight requests,
    /// but this request could succeed once some of them finish.
    Busy { estimate_bytes: u64, available_bytes: u64 },
}

/// RAII handle for a reservation; releases the reserved bytes back to the budget on drop. Owned
/// (no lifetime parameter) so it can be moved into async blocks / streams freely.
#[derive(Debug)]
pub struct Reservation {
    reserved_bytes: Arc<AtomicU64>,
    bytes: u64,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.reserved_bytes.fetch_sub(self.bytes, Ordering::SeqCst);
    }
}

impl Reservation {
    /// Releases the difference between this reservation's current size and `new_bytes` back to
    /// the budget immediately, shrinking the reservation in place rather than waiting for it to
    /// be dropped. Lets a caller reserve an upfront worst-case estimate before doing expensive
    /// work, then hand back whatever turned out to be unnecessary as soon as the real size is
    /// known, so other concurrent requests see that headroom sooner instead of waiting for this
    /// request to finish entirely.
    ///
    /// One-way release valve, not a general resize: a `new_bytes` at or above the currently-held
    /// amount is a no-op rather than growing the reservation (this type intentionally has no way
    /// to reserve *more* after the fact — callers that might need more should reserve for the
    /// worst case up front instead).
    pub fn shrink_to(&mut self, new_bytes: u64) {
        if new_bytes >= self.bytes {
            return;
        }
        let released = self.bytes - new_bytes;
        self.reserved_bytes.fetch_sub(released, Ordering::SeqCst);
        self.bytes = new_bytes;
    }
}

impl MemoryBudget {
    pub fn new_from_env() -> Self {
        let limit_bytes = get_env(LOS_MAX_ANALYSIS_MEMORY_BYTES)
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| {
                println!(
                    "WARN: {} not set, defaulting analysis memory budget to {} bytes",
                    LOS_MAX_ANALYSIS_MEMORY_BYTES, DEFAULT_MAX_ANALYSIS_MEMORY_BYTES
                );
                DEFAULT_MAX_ANALYSIS_MEMORY_BYTES
            });
        Self::new(limit_bytes)
    }

    pub fn new(limit_bytes: u64) -> Self {
        Self { limit_bytes, reserved_bytes: Arc::new(AtomicU64::new(0)) }
    }

    pub fn try_reserve(&self, estimate_bytes: u64) -> Result<Reservation, ReservationErr> {
        if estimate_bytes > self.limit_bytes {
            return Err(ReservationErr::ExceedsLimit { estimate_bytes, limit_bytes: self.limit_bytes });
        }
        loop {
            let current = self.reserved_bytes.load(Ordering::SeqCst);
            let available = self.limit_bytes.saturating_sub(current);
            if estimate_bytes > available {
                return Err(ReservationErr::Busy { estimate_bytes, available_bytes: available });
            }
            let new_val = current + estimate_bytes;
            if self
                .reserved_bytes
                .compare_exchange(current, new_val, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return Ok(Reservation { reserved_bytes: Arc::clone(&self.reserved_bytes), bytes: estimate_bytes });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserves_and_releases_on_drop() {
        let budget = MemoryBudget::new(1000);
        {
            let _r = budget.try_reserve(400).unwrap();
            assert_eq!(budget.reserved_bytes.load(Ordering::SeqCst), 400);
        }
        assert_eq!(budget.reserved_bytes.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn single_request_over_limit_is_permanent() {
        let budget = MemoryBudget::new(1000);
        match budget.try_reserve(1001) {
            Err(ReservationErr::ExceedsLimit { .. }) => {}
            other => panic!("expected ExceedsLimit, got {other:?}"),
        }
    }

    #[test]
    fn concurrent_requests_that_together_exceed_limit_are_throttled() {
        let budget = MemoryBudget::new(1000);
        let _first = budget.try_reserve(700).unwrap();
        match budget.try_reserve(400) {
            Err(ReservationErr::Busy { available_bytes, .. }) => assert_eq!(available_bytes, 300),
            other => panic!("expected Busy, got {other:?}"),
        }
    }

    #[test]
    fn releasing_a_reservation_frees_headroom_for_the_next_request() {
        let budget = MemoryBudget::new(1000);
        {
            let _first = budget.try_reserve(700).unwrap();
        }
        assert!(budget.try_reserve(400).is_ok());
    }

    #[test]
    fn reservation_can_be_moved_into_an_owned_context() {
        // Regression guard for the streaming-endpoint use case: a Reservation must be movable
        // into a 'static / owned context (e.g. an async stream) without borrowing MemoryBudget.
        fn takes_owned(_r: Reservation) {}
        let budget = MemoryBudget::new(1000);
        let r = budget.try_reserve(100).unwrap();
        takes_owned(r);
        assert_eq!(budget.reserved_bytes.load(Ordering::SeqCst), 0);
    }
}
