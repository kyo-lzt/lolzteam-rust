use std::sync::Arc;

use lolzteam::runtime::{ClientConfig, HttpClient, RateLimitConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::Instant;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn mock_listener() -> (TcpListener, String) {
	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let addr = listener.local_addr().unwrap();
	let base_url = format!("http://{addr}");
	(listener, base_url)
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> String {
	let mut buf = vec![0u8; 8192];
	let n = stream.read(&mut buf).await.unwrap();
	String::from_utf8_lossy(&buf[..n]).to_string()
}

async fn write_response(stream: &mut tokio::net::TcpStream, body: &str) {
	let resp = format!(
		"HTTP/1.1 200 OK\r\n\
		 Content-Type: application/json\r\n\
		 Content-Length: {}\r\n\
		 \r\n\
		 {}",
		body.len(),
		body
	);
	stream.write_all(resp.as_bytes()).await.unwrap();
	stream.shutdown().await.ok();
}

fn client_with_rate_limit(base_url: &str, rpm: u32) -> HttpClient {
	HttpClient::new(ClientConfig {
		token: "test".to_string(),
		base_url: base_url.to_string(),
		proxy: None,
		retry: None,
		rate_limit: Some(RateLimitConfig {
			requests_per_minute: rpm,
		}),
		search_rate_limit: None,
		timeout_ms: None,
		on_retry: None,
	})
	.unwrap()
}

fn client_with_dual_rate_limit(base_url: &str, rpm: u32, search_rpm: u32) -> HttpClient {
	HttpClient::new(ClientConfig {
		token: "test".to_string(),
		base_url: base_url.to_string(),
		proxy: None,
		retry: None,
		rate_limit: Some(RateLimitConfig {
			requests_per_minute: rpm,
		}),
		search_rate_limit: Some(RateLimitConfig {
			requests_per_minute: search_rpm,
		}),
		timeout_ms: None,
		on_retry: None,
	})
	.unwrap()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Burst requests within the bucket capacity should not be throttled.
#[tokio::test]
async fn burst_within_limit_not_throttled() {
	let (listener, base_url) = mock_listener().await;
	// 600 RPM = 10/sec, bucket starts full with 600 tokens.
	let client = Arc::new(client_with_rate_limit(&base_url, 600));

	let count = 5;
	tokio::spawn(async move {
		for _ in 0..count {
			let (mut stream, _) = listener.accept().await.unwrap();
			let _ = read_request(&mut stream).await;
			write_response(&mut stream, r#"{"ok":true}"#).await;
		}
	});

	let start = Instant::now();
	for _ in 0..count {
		let _: serde_json::Value = client
			.request("GET", "/burst", None, None, false)
			.await
			.unwrap();
	}
	let elapsed = start.elapsed();

	assert!(
		elapsed.as_millis() < 500,
		"burst of {count} should be fast, took {}ms",
		elapsed.as_millis()
	);
}

/// After exhausting tokens, the next request should be delayed.
#[tokio::test]
async fn delays_after_tokens_exhausted() {
	let (listener, base_url) = mock_listener().await;
	// 60 RPM = 1/sec, bucket starts with 60 tokens.
	let client = Arc::new(client_with_rate_limit(&base_url, 60));

	// We need 61 responses total: 60 to drain, 1 more that should be delayed.
	let total = 61;
	tokio::spawn(async move {
		for _ in 0..total {
			let (mut stream, _) = listener.accept().await.unwrap();
			let _ = read_request(&mut stream).await;
			write_response(&mut stream, r#"{"ok":true}"#).await;
		}
	});

	// Drain all 60 tokens.
	for _ in 0..60 {
		let _: serde_json::Value = client
			.request("GET", "/drain", None, None, false)
			.await
			.unwrap();
	}

	// The 61st request should be delayed (waiting for token refill).
	let start = Instant::now();
	let _: serde_json::Value = client
		.request("GET", "/delayed", None, None, false)
		.await
		.unwrap();
	let elapsed = start.elapsed();

	assert!(
		elapsed.as_millis() >= 500,
		"expected delay after exhaustion, got {}ms",
		elapsed.as_millis()
	);
}

/// Search requests go through both standard and search rate limiters.
#[tokio::test]
async fn search_uses_both_limiters() {
	let (listener, base_url) = mock_listener().await;
	// Standard: 600 RPM (plenty), Search: 60 RPM (1/sec, starts with 60).
	let client = Arc::new(client_with_dual_rate_limit(&base_url, 600, 60));

	// 61 search requests: first 60 drain the search bucket, 61st delays.
	let total = 61;
	tokio::spawn(async move {
		for _ in 0..total {
			let (mut stream, _) = listener.accept().await.unwrap();
			let _ = read_request(&mut stream).await;
			write_response(&mut stream, r#"{"results":[]}"#).await;
		}
	});

	for _ in 0..60 {
		let _: serde_json::Value = client
			.request("GET", "/search", None, None, true)
			.await
			.unwrap();
	}

	let start = Instant::now();
	let _: serde_json::Value = client
		.request("GET", "/search", None, None, true)
		.await
		.unwrap();
	let elapsed = start.elapsed();

	assert!(
		elapsed.as_millis() >= 500,
		"search limiter should throttle, got {}ms",
		elapsed.as_millis()
	);
}

/// Non-search requests don't use the search rate limiter.
#[tokio::test]
async fn non_search_skips_search_limiter() {
	let (listener, base_url) = mock_listener().await;
	// Standard: 600 RPM, Search: 1 RPM (very restrictive).
	let client = Arc::new(client_with_dual_rate_limit(&base_url, 600, 1));

	let count = 5;
	tokio::spawn(async move {
		for _ in 0..count {
			let (mut stream, _) = listener.accept().await.unwrap();
			let _ = read_request(&mut stream).await;
			write_response(&mut stream, r#"{"ok":true}"#).await;
		}
	});

	let start = Instant::now();
	for _ in 0..count {
		// is_search = false, so search limiter (1 RPM) shouldn't apply.
		let _: serde_json::Value = client
			.request("GET", "/normal", None, None, false)
			.await
			.unwrap();
	}
	let elapsed = start.elapsed();

	assert!(
		elapsed.as_millis() < 500,
		"non-search should not use search limiter, took {}ms",
		elapsed.as_millis()
	);
}

/// No rate limit config means no throttling at all.
#[tokio::test]
async fn no_rate_limit_config_no_throttling() {
	let (listener, base_url) = mock_listener().await;
	let client = HttpClient::new(ClientConfig {
		token: "test".to_string(),
		base_url,
		proxy: None,
		retry: None,
		rate_limit: None,
		search_rate_limit: None,
		timeout_ms: None,
		on_retry: None,
	})
	.unwrap();

	let count = 5;
	tokio::spawn(async move {
		for _ in 0..count {
			let (mut stream, _) = listener.accept().await.unwrap();
			let _ = read_request(&mut stream).await;
			write_response(&mut stream, r#"{"ok":true}"#).await;
		}
	});

	let start = Instant::now();
	for _ in 0..count {
		let _: serde_json::Value = client
			.request("GET", "/fast", None, None, false)
			.await
			.unwrap();
	}
	let elapsed = start.elapsed();

	assert!(
		elapsed.as_millis() < 500,
		"no rate limit should be fast, took {}ms",
		elapsed.as_millis()
	);
}
