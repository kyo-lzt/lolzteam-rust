use std::future::Future;
use std::time::Duration;

use crate::runtime::{LolzteamError, RetryConfig};

/// Execute an async closure with retry on transient errors.
///
/// Retries on 429 (rate limit) and 502/503 (server error) with exponential
/// backoff and jitter. Respects `Retry-After` header on 429 responses.
pub async fn with_retry<T, F, Fut>(config: &RetryConfig, f: F) -> Result<T, LolzteamError>
where
	F: Fn() -> Fut,
	Fut: Future<Output = Result<T, LolzteamError>>,
{
	let mut attempt: u32 = 0;

	loop {
		match f().await {
			Ok(value) => return Ok(value),
			Err(err) => {
				let should_retry = match &err {
					LolzteamError::Http(http_err) => http_err.is_retryable(),
					LolzteamError::Network(_) | LolzteamError::Config(_) => false,
				};

				if !should_retry || attempt >= config.max_retries {
					return Err(err);
				}

				let delay = match &err {
					LolzteamError::Http(http_err) if http_err.is_rate_limit() => {
						// Use Retry-After if available, otherwise fall back to backoff.
						match http_err.retry_after_secs() {
							Some(secs) => Duration::from_secs(secs),
							None => compute_backoff(config, attempt),
						}
					}
					_ => compute_backoff(config, attempt),
				};

				tokio::time::sleep(delay).await;
				attempt += 1;
			}
		}
	}
}

/// Compute backoff delay: `min(base_delay * 2^attempt + jitter, max_delay)`.
fn compute_backoff(config: &RetryConfig, attempt: u32) -> Duration {
	let base = config
		.base_delay_ms
		.saturating_mul(1u64.checked_shl(attempt).unwrap_or(u64::MAX));
	let jitter = random_jitter(config.base_delay_ms);
	let delay_ms = base.saturating_add(jitter).min(config.max_delay_ms);
	Duration::from_millis(delay_ms)
}

/// Random jitter in range `[0, base_delay_ms)`.
fn random_jitter(base_delay_ms: u64) -> u64 {
	if base_delay_ms == 0 {
		return 0;
	}
	fastrand::u64(0..base_delay_ms)
}
