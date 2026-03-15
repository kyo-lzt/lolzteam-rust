use tokio::sync::Mutex;
use tokio::time::Instant;

/// Token bucket rate limiter.
///
/// Permits up to `requests_per_minute` requests per minute, refilling
/// tokens continuously at a constant rate.
pub struct RateLimiter {
	state: Mutex<BucketState>,
	tokens_per_sec: f64,
	max_tokens: f64,
}

struct BucketState {
	tokens: f64,
	last_refill: Instant,
}

impl RateLimiter {
	/// Create a new rate limiter with the given capacity.
	#[must_use]
	pub fn new(requests_per_minute: u32) -> Self {
		let max_tokens = f64::from(requests_per_minute);
		let tokens_per_sec = max_tokens / 60.0;

		Self {
			state: Mutex::new(BucketState {
				tokens: max_tokens,
				last_refill: Instant::now(),
			}),
			tokens_per_sec,
			max_tokens,
		}
	}

	/// Acquire a single token, waiting if none are available.
	pub async fn acquire(&self) {
		loop {
			let sleep_duration = {
				let mut state = self.state.lock().await;
				let now = Instant::now();
				let elapsed = now.duration_since(state.last_refill).as_secs_f64();

				// Refill tokens based on elapsed time.
				state.tokens = (state.tokens + elapsed * self.tokens_per_sec).min(self.max_tokens);
				state.last_refill = now;

				if state.tokens >= 1.0 {
					state.tokens -= 1.0;
					return;
				}

				// Calculate how long until one token is available.
				let deficit = 1.0 - state.tokens;
				std::time::Duration::from_secs_f64(deficit / self.tokens_per_sec)
			};

			tokio::time::sleep(sleep_duration).await;
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn allows_burst_within_limit() {
		let limiter = RateLimiter::new(60); // 1 per sec, bucket starts full with 60
		let start = Instant::now();
		for _ in 0..5 {
			limiter.acquire().await;
		}
		let elapsed = start.elapsed();
		assert!(
			elapsed.as_millis() < 100,
			"burst should be near-instant, took {}ms",
			elapsed.as_millis()
		);
	}

	#[tokio::test]
	async fn delays_when_tokens_exhausted() {
		let limiter = RateLimiter::new(60); // 1 per sec
									  // Drain all 60 tokens
		for _ in 0..60 {
			limiter.acquire().await;
		}
		let start = Instant::now();
		limiter.acquire().await;
		let elapsed = start.elapsed();
		assert!(
			elapsed.as_millis() >= 500,
			"expected delay after exhaustion, got {}ms",
			elapsed.as_millis()
		);
	}

	#[test]
	fn new_sets_correct_rate() {
		let limiter = RateLimiter::new(120);
		assert!((limiter.tokens_per_sec - 2.0).abs() < f64::EPSILON);
		assert!((limiter.max_tokens - 120.0).abs() < f64::EPSILON);
	}
}
