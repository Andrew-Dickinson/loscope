use crate::analysis::memory_budget::ReservationErr;
use crate::building::heightmap::HeightMapCreateErr;
use crate::types::errors::AssetErr;
use rocket::http::Status;
use rocket::response::Responder;
use rocket::{Request, Response};

/// Base delay suggested to clients via `Retry-After` when a request is throttled for lack of
/// memory headroom. Clients are expected to back off exponentially from here on repeated
/// throttling, not to treat this as a fixed retry interval.
const THROTTLE_RETRY_AFTER_SECONDS: u32 = 3;

/// Generic endpoint error response. Distinguishes plain status-code failures from throttling,
/// which additionally needs a `Retry-After` header so the frontend knows to back off and retry
/// rather than treat the request as permanently failed.
pub struct ApiError {
    status: Status,
    retry_after_seconds: Option<u32>,
}

impl ApiError {
    pub fn new(status: Status) -> Self {
        Self { status, retry_after_seconds: None }
    }

    fn throttled(retry_after_seconds: u32) -> Self {
        Self { status: Status::ServiceUnavailable, retry_after_seconds: Some(retry_after_seconds) }
    }
}

impl From<Status> for ApiError {
    fn from(status: Status) -> Self {
        ApiError::new(status)
    }
}

impl From<AssetErr> for ApiError {
    fn from(err: AssetErr) -> Self {
        ApiError::new(Status::from(err))
    }
}

impl From<HeightMapCreateErr> for ApiError {
    fn from(err: HeightMapCreateErr) -> Self {
        ApiError::new(Status::from(err))
    }
}

impl From<ReservationErr> for ApiError {
    fn from(err: ReservationErr) -> Self {
        match err {
            // This link/frequency combination can never fit in the configured budget alone —
            // retrying won't help, so this is a hard error, not a throttle.
            ReservationErr::ExceedsLimit { estimate_bytes, limit_bytes } => {
                eprintln!(
                    "analysis rejected: estimated {estimate_bytes} bytes exceeds memory budget of {limit_bytes} bytes"
                );
                ApiError::new(Status::PayloadTooLarge)
            }
            ReservationErr::Busy { estimate_bytes, available_bytes } => {
                eprintln!(
                    "analysis throttled: estimated {estimate_bytes} bytes, only {available_bytes} bytes available"
                );
                ApiError::throttled(THROTTLE_RETRY_AFTER_SECONDS)
            }
        }
    }
}

impl<'r> Responder<'r, 'static> for ApiError {
    fn respond_to(self, _req: &'r Request<'_>) -> rocket::response::Result<'static> {
        let mut builder = Response::build();
        builder.status(self.status);
        if let Some(secs) = self.retry_after_seconds {
            builder.raw_header("Retry-After", secs.to_string());
        }
        builder.ok()
    }
}
