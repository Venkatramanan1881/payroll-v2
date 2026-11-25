//! Sign-in rate limits (TSD §15, API_CONTRACTS A1): per client IP on every sign-in route, and per
//! account where a route names one. Over the limit → 429 `rate_limited` with Retry-After.
// shortcut: counters live in this server's memory, so each Cloud Run instance counts on its own and a
// restart forgets them; move to a shared store (Memorystore) or Cloud Armor per-IP rules if that's too weak.

use crate::error::ApiError;
use crate::AppState;
use axum::extract::{Request, State};
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use shared::AppError;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const MINUTE: Duration = Duration::from_secs(60);
/// Per IP, per sign-in route.
pub const IP_LIMIT: (u32, Duration) = (30, MINUTE);
/// Per account (organisation + employee code, vendor email, one MFA step token). Above the 5-try lock
/// so a locked employee still gets account_locked; this only stops floods.
pub const ACCOUNT_LIMIT: (u32, Duration) = (20, Duration::from_secs(15 * 60));
/// One-time codes by email: few, so nobody can flood an inbox.
pub const OTP_SEND_LIMIT: (u32, Duration) = (3, Duration::from_secs(10 * 60));
const LONGEST: Duration = Duration::from_secs(15 * 60);
const PRUNE_AT: usize = 50_000;

/// Fixed-window counters.
#[derive(Default)]
pub struct Limiter {
    hits: Mutex<HashMap<String, (Instant, u32)>>,
}

impl Limiter {
    /// Counts one attempt; Err(seconds to wait) once over `limit` within `window`.
    pub fn hit(&self, key: &str, (limit, window): (u32, Duration)) -> Result<(), u64> {
        let now = Instant::now();
        let mut hits = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        if hits.len() > PRUNE_AT {
            hits.retain(|_, (start, _)| now.duration_since(*start) < LONGEST);
        }
        let entry = hits.entry(key.to_string()).or_insert((now, 0));
        if now.duration_since(entry.0) >= window {
            *entry = (now, 0);
        }
        entry.1 += 1;
        if entry.1 > limit {
            return Err((window.saturating_sub(now.duration_since(entry.0))).as_secs().max(1));
        }
        Ok(())
    }

    /// `hit`, as the API error.
    pub fn check(&self, key: &str, limit: (u32, Duration)) -> Result<(), ApiError> {
        self.hit(key, limit).map_err(limited)
    }
}

pub fn limited(retry_after: u64) -> ApiError {
    AppError::new(429, "rate_limited", "Too many attempts", "Wait a little and try again")
        .with_meta(serde_json::json!({ "retry_after": retry_after }))
        .into()
}

/// The caller's address: the last X-Forwarded-For entry, which Cloud Run's front end appends.
// shortcut: right for Cloud Run without a load balancer in front; behind a Google load balancer use the second-to-last entry.
pub(crate) fn client_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit(',').next())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

/// Layer on the sign-in routes.
pub async fn per_ip(State(s): State<AppState>, req: Request, next: Next) -> Response {
    let key = format!("ip:{}:{}", req.uri().path(), client_ip(req.headers()));
    match s.limiter.check(&key, IP_LIMIT) {
        Ok(()) => next.run(req).await,
        Err(e) => e.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_per_key_and_resets_after_the_window() {
        let l = Limiter::default();
        let short = (2, Duration::from_millis(50));
        assert!(l.hit("a", short).is_ok() && l.hit("a", short).is_ok());
        assert!(l.hit("a", short).unwrap_err() >= 1);
        assert!(l.hit("b", short).is_ok(), "other keys are counted apart");
        std::thread::sleep(Duration::from_millis(60));
        assert!(l.hit("a", short).is_ok(), "a new window starts");
    }

    #[test]
    fn client_ip_is_the_last_forwarded_entry() {
        let mut h = HeaderMap::new();
        assert_eq!(client_ip(&h), "unknown");
        h.insert("x-forwarded-for", "1.2.3.4, 9.9.9.9".parse().unwrap());
        assert_eq!(client_ip(&h), "9.9.9.9");
    }
}
