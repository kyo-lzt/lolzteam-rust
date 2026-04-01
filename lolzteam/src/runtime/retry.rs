use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use crate::runtime::types::RetryInfo;
use crate::runtime::{LolzteamError, RetryConfig};

/// Execute an async closure with retry on transient errors.
///
/// Retries on 429 (rate limit), 502/503/504 (server error), and transient
/// network errors (timeout, connection failure) with exponential backoff
/// and jitter. Respects `Retry-After` header on 429 responses.
pub async fn with_retry<T, F, Fut>(
	config: &RetryConfig,
	on_retry: Option<&Arc<dyn Fn(RetryInfo) + Send + Sync>>,
	method: &str,
	path: &str,
	f: F,
) -> Result<T, LolzteamError>
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
					LolzteamError::Network(net_err) => net_err.is_transient(),
					LolzteamError::Config(_) | LolzteamError::RetryExhausted { .. } => false,
				};

				if !should_retry {
					return Err(err);
				}
				if attempt >= config.max_retries {
					return if attempt > 0 {
						Err(LolzteamError::RetryExhausted {
							attempts: attempt + 1,
							last_error: Box::new(err),
						})
					} else {
						Err(err)
					};
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

				if let Some(cb) = on_retry {
					cb(RetryInfo {
						attempt,
						delay_ms: delay.as_millis() as u64,
						method: method.to_string(),
						path: path.to_string(),
					});
				}

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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn compute_backoff_attempt_0() {
		let config = RetryConfig {
			max_retries: 3,
			base_delay_ms: 1000,
			max_delay_ms: 30000,
		};
		let delay = compute_backoff(&config, 0);
		// base_delay * 2^0 + jitter = 1000 + [0, 1000) -> [1000, 2000)
		assert!(delay.as_millis() >= 1000);
		assert!(delay.as_millis() < 2000);
	}

	#[test]
	fn compute_backoff_attempt_1() {
		let config = RetryConfig {
			max_retries: 3,
			base_delay_ms: 1000,
			max_delay_ms: 30000,
		};
		let delay = compute_backoff(&config, 1);
		// base_delay * 2^1 + jitter = 2000 + [0, 1000) -> [2000, 3000)
		assert!(delay.as_millis() >= 2000);
		assert!(delay.as_millis() < 3000);
	}

	#[test]
	fn compute_backoff_capped_by_max_delay() {
		let config = RetryConfig {
			max_retries: 10,
			base_delay_ms: 1000,
			max_delay_ms: 5000,
		};
		let delay = compute_backoff(&config, 8);
		assert!(delay.as_millis() <= 5000);
	}

	#[test]
	fn random_jitter_zero_base() {
		assert_eq!(random_jitter(0), 0);
	}

	#[test]
	fn random_jitter_within_range() {
		for _ in 0..100 {
			let j = random_jitter(1000);
			assert!(j < 1000);
		}
	}

	#[tokio::test]
	async fn retry_exhausted_after_max_retries() {
		use crate::runtime::errors::{HttpError, HttpErrorData};
		use std::sync::atomic::{AtomicU32, Ordering};

		let config = RetryConfig {
			max_retries: 1,
			base_delay_ms: 1, // minimal delay for fast test
			max_delay_ms: 1,
		};

		let call_count = AtomicU32::new(0);

		let result: Result<(), LolzteamError> = with_retry(&config, None, "GET", "/test", || {
			call_count.fetch_add(1, Ordering::SeqCst);
			async {
				Err(LolzteamError::Http(HttpError::RateLimit {
					data: HttpErrorData {
						status: 429,
						body: serde_json::Value::Null,
					},
					retry_after: None,
				}))
			}
		})
		.await;

		let err = result.unwrap_err();
		match &err {
			LolzteamError::RetryExhausted {
				attempts,
				last_error,
			} => {
				assert_eq!(*attempts, 2);
				assert!(matches!(**last_error, LolzteamError::Http(_)));
			}
			other => panic!("expected RetryExhausted, got: {other}"),
		}
		assert_eq!(call_count.load(Ordering::SeqCst), 2);
	}

	#[tokio::test]
	async fn no_retry_exhausted_when_max_retries_zero() {
		use crate::runtime::errors::{HttpError, HttpErrorData};

		let config = RetryConfig {
			max_retries: 0,
			base_delay_ms: 1,
			max_delay_ms: 1,
		};

		let result: Result<(), LolzteamError> =
			with_retry(&config, None, "GET", "/test", || async {
				Err(LolzteamError::Http(HttpError::RateLimit {
					data: HttpErrorData {
						status: 429,
						body: serde_json::Value::Null,
					},
					retry_after: None,
				}))
			})
			.await;

		let err = result.unwrap_err();
		// With max_retries=0, attempt=0 so attempt > 0 is false → original error returned
		assert!(
			matches!(err, LolzteamError::Http(_)),
			"expected Http error, got: {err}"
		);
	}
}
