use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use lolzteam::runtime::{ClientConfig, HttpClient, LolzteamError, RetryConfig, RetryInfo};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn mock_listener() -> (TcpListener, String) {
	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let addr = listener.local_addr().unwrap();
	let base_url = format!("http://{addr}");
	(listener, base_url)
}

fn fast_retry_client(base_url: &str) -> HttpClient {
	HttpClient::new(ClientConfig {
		token: "test".to_string(),
		base_url: base_url.to_string(),
		proxy: None,
		retry: Some(RetryConfig {
			max_retries: 3,
			base_delay_ms: 1,
			max_delay_ms: 5,
		}),
		rate_limit: None,
		search_rate_limit: None,
		timeout_ms: None,
		on_retry: None,
	})
	.unwrap()
}

fn client_with_on_retry(base_url: &str, cb: Arc<dyn Fn(RetryInfo) + Send + Sync>) -> HttpClient {
	HttpClient::new(ClientConfig {
		token: "test".to_string(),
		base_url: base_url.to_string(),
		proxy: None,
		retry: Some(RetryConfig {
			max_retries: 2,
			base_delay_ms: 1,
			max_delay_ms: 5,
		}),
		rate_limit: None,
		search_rate_limit: None,
		timeout_ms: None,
		on_retry: Some(cb),
	})
	.unwrap()
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> String {
	let mut buf = vec![0u8; 8192];
	let n = stream.read(&mut buf).await.unwrap();
	String::from_utf8_lossy(&buf[..n]).to_string()
}

async fn write_response(
	stream: &mut tokio::net::TcpStream,
	status: u16,
	extra_headers: &str,
	body: &str,
) {
	let reason = match status {
		200 => "OK",
		429 => "Too Many Requests",
		502 => "Bad Gateway",
		503 => "Service Unavailable",
		504 => "Gateway Timeout",
		_ => "Error",
	};
	let resp = format!(
		"HTTP/1.1 {status} {reason}\r\n\
		 Content-Type: application/json\r\n\
		 Content-Length: {len}\r\n\
		 {extra_headers}\
		 \r\n\
		 {body}",
		len = body.len(),
	);
	stream.write_all(resp.as_bytes()).await.unwrap();
	stream.shutdown().await.ok();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Succeeds on first attempt — no retry.
#[tokio::test]
async fn succeeds_on_first_try() {
	let (listener, base_url) = mock_listener().await;
	let client = fast_retry_client(&base_url);

	tokio::spawn(async move {
		let (mut stream, _) = listener.accept().await.unwrap();
		let _ = read_request(&mut stream).await;
		write_response(&mut stream, 200, "", r#"{"ok":true}"#).await;
	});

	let result: serde_json::Value = client
		.request("GET", "/ok", None, None, false)
		.await
		.unwrap();
	assert_eq!(result["ok"], true);
}

/// 429 with Retry-After: 0 should retry almost immediately.
#[tokio::test]
async fn respects_retry_after_header() {
	let (listener, base_url) = mock_listener().await;
	let client = HttpClient::new(ClientConfig {
		token: "test".to_string(),
		base_url: base_url.clone(),
		proxy: None,
		retry: Some(RetryConfig {
			max_retries: 1,
			base_delay_ms: 5000, // high base delay
			max_delay_ms: 10000,
		}),
		rate_limit: None,
		search_rate_limit: None,
		timeout_ms: None,
		on_retry: None,
	})
	.unwrap();

	tokio::spawn(async move {
		let (mut stream, _) = listener.accept().await.unwrap();
		let _ = read_request(&mut stream).await;
		write_response(
			&mut stream,
			429,
			"Retry-After: 0\r\n",
			r#"{"error":"limited"}"#,
		)
		.await;

		let (mut stream, _) = listener.accept().await.unwrap();
		let _ = read_request(&mut stream).await;
		write_response(&mut stream, 200, "", r#"{"ok":true}"#).await;
	});

	let start = tokio::time::Instant::now();
	let result: serde_json::Value = client
		.request("GET", "/rl", None, None, false)
		.await
		.unwrap();
	let elapsed = start.elapsed();

	assert_eq!(result["ok"], true);
	// Retry-After: 0 means near-instant, not the 5000ms base delay.
	assert!(
		elapsed.as_millis() < 500,
		"should use Retry-After, not base delay; took {}ms",
		elapsed.as_millis()
	);
}

/// 401 is not retried — returns immediately.
#[tokio::test]
async fn auth_error_not_retried() {
	let (listener, base_url) = mock_listener().await;
	let call_count = Arc::new(AtomicUsize::new(0));
	let counter = Arc::clone(&call_count);
	let client = fast_retry_client(&base_url);

	tokio::spawn(async move {
		let (mut stream, _) = listener.accept().await.unwrap();
		let _ = read_request(&mut stream).await;
		counter.fetch_add(1, Ordering::SeqCst);
		write_response(&mut stream, 401, "", r#"{"error":"unauthorized"}"#).await;
	});

	let result: Result<serde_json::Value, LolzteamError> =
		client.request("GET", "/secret", None, None, false).await;

	assert!(matches!(result.unwrap_err(), LolzteamError::Http(e) if e.is_auth_error()));
	assert_eq!(call_count.load(Ordering::SeqCst), 1, "should not retry 401");
}

/// 404 is not retried.
#[tokio::test]
async fn not_found_not_retried() {
	let (listener, base_url) = mock_listener().await;
	let call_count = Arc::new(AtomicUsize::new(0));
	let counter = Arc::clone(&call_count);
	let client = fast_retry_client(&base_url);

	tokio::spawn(async move {
		let (mut stream, _) = listener.accept().await.unwrap();
		let _ = read_request(&mut stream).await;
		counter.fetch_add(1, Ordering::SeqCst);
		write_response(&mut stream, 404, "", r#"{"error":"not found"}"#).await;
	});

	let result: Result<serde_json::Value, LolzteamError> =
		client.request("GET", "/missing", None, None, false).await;

	assert!(matches!(result.unwrap_err(), LolzteamError::Http(e) if e.is_not_found()));
	assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

/// 500 is not retried (only 502/503/504 are).
#[tokio::test]
async fn server_500_not_retried() {
	let (listener, base_url) = mock_listener().await;
	let call_count = Arc::new(AtomicUsize::new(0));
	let counter = Arc::clone(&call_count);
	let client = fast_retry_client(&base_url);

	tokio::spawn(async move {
		let (mut stream, _) = listener.accept().await.unwrap();
		let _ = read_request(&mut stream).await;
		counter.fetch_add(1, Ordering::SeqCst);
		write_response(&mut stream, 500, "", r#"{"error":"internal"}"#).await;
	});

	let result: Result<serde_json::Value, LolzteamError> =
		client.request("GET", "/crash", None, None, false).await;

	assert!(matches!(result.unwrap_err(), LolzteamError::Http(e) if e.status() == 500));
	assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

/// 504 is retried (just like 502/503).
#[tokio::test]
async fn server_504_is_retried() {
	let (listener, base_url) = mock_listener().await;
	let call_count = Arc::new(AtomicUsize::new(0));
	let counter = Arc::clone(&call_count);
	let client = fast_retry_client(&base_url);

	tokio::spawn(async move {
		let (mut stream, _) = listener.accept().await.unwrap();
		let _ = read_request(&mut stream).await;
		counter.fetch_add(1, Ordering::SeqCst);
		write_response(&mut stream, 504, "", r#"{"error":"timeout"}"#).await;

		let (mut stream, _) = listener.accept().await.unwrap();
		let _ = read_request(&mut stream).await;
		counter.fetch_add(1, Ordering::SeqCst);
		write_response(&mut stream, 200, "", r#"{"ok":true}"#).await;
	});

	let result: serde_json::Value = client
		.request("GET", "/slow", None, None, false)
		.await
		.unwrap();
	assert_eq!(result["ok"], true);
	assert_eq!(call_count.load(Ordering::SeqCst), 2);
}

/// Exhausts all retries, wraps last error in RetryExhausted.
#[tokio::test]
async fn retry_exhausted_wraps_last_error() {
	let (listener, base_url) = mock_listener().await;
	let call_count = Arc::new(AtomicUsize::new(0));
	let counter = Arc::clone(&call_count);
	let client = HttpClient::new(ClientConfig {
		token: "test".to_string(),
		base_url,
		proxy: None,
		retry: Some(RetryConfig {
			max_retries: 2,
			base_delay_ms: 1,
			max_delay_ms: 1,
		}),
		rate_limit: None,
		search_rate_limit: None,
		timeout_ms: None,
		on_retry: None,
	})
	.unwrap();

	tokio::spawn(async move {
		for _ in 0..3 {
			let (mut stream, _) = listener.accept().await.unwrap();
			let _ = read_request(&mut stream).await;
			counter.fetch_add(1, Ordering::SeqCst);
			write_response(&mut stream, 503, "", r#"{"error":"unavailable"}"#).await;
		}
	});

	let result: Result<serde_json::Value, LolzteamError> =
		client.request("GET", "/down", None, None, false).await;

	let err = result.unwrap_err();
	match &err {
		LolzteamError::RetryExhausted {
			attempts,
			last_error,
		} => {
			assert_eq!(*attempts, 3); // 1 initial + 2 retries
			assert!(matches!(**last_error, LolzteamError::Http(_)));
		}
		other => panic!("expected RetryExhausted, got: {other}"),
	}
	assert_eq!(call_count.load(Ordering::SeqCst), 3);
}

/// on_retry callback is invoked for each retry attempt.
#[tokio::test]
async fn on_retry_callback_invoked() {
	let (listener, base_url) = mock_listener().await;
	let infos: Arc<std::sync::Mutex<Vec<RetryInfo>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
	let infos_clone = Arc::clone(&infos);

	let cb: Arc<dyn Fn(RetryInfo) + Send + Sync> = Arc::new(move |info| {
		infos_clone.lock().unwrap().push(info);
	});

	let client = client_with_on_retry(&base_url, cb);

	tokio::spawn(async move {
		// Two 429s, then 200.
		for _ in 0..2 {
			let (mut stream, _) = listener.accept().await.unwrap();
			let _ = read_request(&mut stream).await;
			write_response(&mut stream, 429, "", r#"{"error":"limited"}"#).await;
		}
		let (mut stream, _) = listener.accept().await.unwrap();
		let _ = read_request(&mut stream).await;
		write_response(&mut stream, 200, "", r#"{"ok":true}"#).await;
	});

	let result: serde_json::Value = client
		.request("GET", "/cb-test", None, None, false)
		.await
		.unwrap();
	assert_eq!(result["ok"], true);

	let collected = infos.lock().unwrap();
	assert_eq!(collected.len(), 2, "should have 2 retry callbacks");
	assert_eq!(collected[0].attempt, 0);
	assert_eq!(collected[1].attempt, 1);
	assert_eq!(collected[0].path, "/cb-test");
	assert_eq!(collected[0].method, "GET");
}

/// No retry config means no retries at all.
#[tokio::test]
async fn no_retry_config_returns_error_immediately() {
	let (listener, base_url) = mock_listener().await;
	let call_count = Arc::new(AtomicUsize::new(0));
	let counter = Arc::clone(&call_count);

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

	tokio::spawn(async move {
		let (mut stream, _) = listener.accept().await.unwrap();
		let _ = read_request(&mut stream).await;
		counter.fetch_add(1, Ordering::SeqCst);
		write_response(&mut stream, 503, "", r#"{"error":"down"}"#).await;
	});

	let result: Result<serde_json::Value, LolzteamError> =
		client.request("GET", "/no-retry", None, None, false).await;

	assert!(matches!(result.unwrap_err(), LolzteamError::Http(_)));
	assert_eq!(call_count.load(Ordering::SeqCst), 1);
}
