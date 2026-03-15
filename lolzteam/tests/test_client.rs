use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use lolzteam::generated::forum::ForumClient;
use lolzteam::generated::market::MarketClient;
use lolzteam::runtime::{
	ClientConfig, HttpClient, HttpError, LolzteamError, ParamValue, ProxyConfig, RateLimitConfig,
	RetryConfig,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

// ---------------------------------------------------------------------------
// Mock HTTP server helpers
// ---------------------------------------------------------------------------

/// Start a TCP listener on a random port and return (listener, base_url).
async fn mock_listener() -> (TcpListener, String) {
	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let addr = listener.local_addr().unwrap();
	let base_url = format!("http://{addr}");
	(listener, base_url)
}

/// Build an HttpClient pointing at the given base_url with no rate limiting
/// and fast retries.
fn test_client(base_url: &str, token: &str) -> HttpClient {
	let config = ClientConfig {
		token: token.to_string(),
		base_url: base_url.to_string(),
		proxy: None,
		retry: RetryConfig {
			max_retries: 3,
			base_delay_ms: 10,
			max_delay_ms: 50,
		},
		rate_limit: None,
		search_rate_limit: None,
	};
	HttpClient::new(config).unwrap()
}

/// Read the full HTTP request from a TcpStream (up to 8KB).
async fn read_request(stream: &mut tokio::net::TcpStream) -> String {
	let mut buf = vec![0u8; 8192];
	let n = stream.read(&mut buf).await.unwrap();
	String::from_utf8_lossy(&buf[..n]).to_string()
}

/// Write an HTTP response with the given status, optional headers, and body.
async fn write_response(
	stream: &mut tokio::net::TcpStream,
	status: u16,
	extra_headers: &str,
	body: &str,
) {
	let reason = match status {
		200 => "OK",
		401 => "Unauthorized",
		404 => "Not Found",
		429 => "Too Many Requests",
		502 => "Bad Gateway",
		503 => "Service Unavailable",
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
// RetryConfig defaults
// ---------------------------------------------------------------------------

#[test]
fn retry_config_default_values() {
	let cfg = RetryConfig::default();
	assert_eq!(cfg.max_retries, 3);
	assert_eq!(cfg.base_delay_ms, 1000);
	assert_eq!(cfg.max_delay_ms, 30000);
}

#[test]
fn retry_config_custom_values() {
	let cfg = RetryConfig {
		max_retries: 5,
		base_delay_ms: 500,
		max_delay_ms: 10000,
	};
	assert_eq!(cfg.max_retries, 5);
	assert_eq!(cfg.base_delay_ms, 500);
	assert_eq!(cfg.max_delay_ms, 10000);
}

// ---------------------------------------------------------------------------
// Config types construction
// ---------------------------------------------------------------------------

#[test]
fn client_config_minimal() {
	let cfg = ClientConfig {
		token: "test-token".to_string(),
		base_url: "https://example.com".to_string(),
		proxy: None,
		retry: RetryConfig::default(),
		rate_limit: None,
		search_rate_limit: None,
	};
	assert_eq!(cfg.token, "test-token");
	assert_eq!(cfg.base_url, "https://example.com");
	assert!(cfg.proxy.is_none());
	assert!(cfg.rate_limit.is_none());
}

#[test]
fn client_config_with_proxy() {
	let cfg = ClientConfig {
		token: "t".to_string(),
		base_url: "https://example.com".to_string(),
		proxy: Some(ProxyConfig {
			url: "socks5://127.0.0.1:1080".to_string(),
		}),
		retry: RetryConfig::default(),
		rate_limit: None,
		search_rate_limit: None,
	};
	let proxy = cfg.proxy.as_ref().unwrap();
	assert_eq!(proxy.url, "socks5://127.0.0.1:1080");
}

#[test]
fn client_config_with_rate_limit() {
	let cfg = ClientConfig {
		token: "t".to_string(),
		base_url: "https://example.com".to_string(),
		proxy: None,
		retry: RetryConfig::default(),
		rate_limit: Some(RateLimitConfig {
			requests_per_minute: 60,
		}),
		search_rate_limit: None,
	};
	let rl = cfg.rate_limit.as_ref().unwrap();
	assert_eq!(rl.requests_per_minute, 60);
}

#[test]
fn config_types_are_clone_and_debug() {
	let cfg = ClientConfig {
		token: "t".to_string(),
		base_url: "https://example.com".to_string(),
		proxy: Some(ProxyConfig {
			url: "http://proxy".to_string(),
		}),
		retry: RetryConfig::default(),
		rate_limit: Some(RateLimitConfig {
			requests_per_minute: 100,
		}),
		search_rate_limit: None,
	};
	let cloned = cfg.clone();
	assert_eq!(format!("{:?}", cloned), format!("{:?}", cfg));
}

// ---------------------------------------------------------------------------
// HttpError
// ---------------------------------------------------------------------------

#[test]
fn http_error_is_rate_limit() {
	let err = HttpError {
		status: 429,
		body: serde_json::json!({"error": "rate limited"}),
		retry_after: Some(5),
	};
	assert!(err.is_rate_limit());
	assert!(err.is_retryable());
	assert!(!err.is_server_error());
	assert!(!err.is_auth_error());
	assert!(!err.is_not_found());
	assert_eq!(err.retry_after_secs(), Some(5));
}

#[test]
fn http_error_server_errors() {
	for status in [500, 502, 503, 504] {
		let err = HttpError {
			status,
			body: serde_json::Value::Null,
			retry_after: None,
		};
		assert!(
			err.is_server_error(),
			"status {status} should be server error"
		);
	}
}

#[test]
fn http_error_retryable_statuses() {
	// 429, 502, 503 are retryable; 500, 504, 400 are not
	let retryable = [429, 502, 503];
	let not_retryable = [400, 401, 403, 404, 500, 504];

	for status in retryable {
		let err = HttpError {
			status,
			body: serde_json::Value::Null,
			retry_after: None,
		};
		assert!(err.is_retryable(), "status {status} should be retryable");
	}
	for status in not_retryable {
		let err = HttpError {
			status,
			body: serde_json::Value::Null,
			retry_after: None,
		};
		assert!(
			!err.is_retryable(),
			"status {status} should NOT be retryable"
		);
	}
}

#[test]
fn http_error_auth_error() {
	for status in [401, 403] {
		let err = HttpError {
			status,
			body: serde_json::Value::Null,
			retry_after: None,
		};
		assert!(err.is_auth_error(), "status {status} should be auth error");
	}
}

#[test]
fn http_error_not_found() {
	let err = HttpError {
		status: 404,
		body: serde_json::Value::Null,
		retry_after: None,
	};
	assert!(err.is_not_found());
}

#[test]
fn http_error_display() {
	let err = HttpError {
		status: 422,
		body: serde_json::json!({"error": "invalid"}),
		retry_after: None,
	};
	let display = format!("{err}");
	assert!(display.contains("422"));
}

#[test]
fn http_error_converts_to_lolzteam_error() {
	let http_err = HttpError {
		status: 500,
		body: serde_json::Value::Null,
		retry_after: None,
	};
	let err: LolzteamError = http_err.into();
	assert!(matches!(err, LolzteamError::Http(_)));
}

// ---------------------------------------------------------------------------
// ForumClient construction
// ---------------------------------------------------------------------------

#[test]
fn forum_client_new_succeeds() {
	let client = ForumClient::new("test-token");
	assert!(client.is_ok());
}

#[test]
fn forum_client_with_config_succeeds() {
	let config = ClientConfig {
		token: "test-token".to_string(),
		base_url: "https://prod-api.lolz.live".to_string(),
		proxy: None,
		retry: RetryConfig {
			max_retries: 5,
			base_delay_ms: 2000,
			max_delay_ms: 60000,
		},
		rate_limit: Some(RateLimitConfig {
			requests_per_minute: 300,
		}),
		search_rate_limit: None,
	};
	let client = ForumClient::with_config(config);
	assert!(client.is_ok());
}

#[test]
fn forum_client_accepts_string_token() {
	let token = String::from("my-token");
	let client = ForumClient::new(token);
	assert!(client.is_ok());
}

/// Verify all 18 API group accessors compile and return references.
#[test]
fn forum_client_all_api_group_accessors() {
	let client = ForumClient::new("token").unwrap();

	// Each accessor must compile and return a reference.
	let _ = client.assets();
	let _ = client.batch();
	let _ = client.categories();
	let _ = client.chatbox();
	let _ = client.conversations();
	let _ = client.forms();
	let _ = client.forums();
	let _ = client.links();
	let _ = client.navigation();
	let _ = client.notifications();
	let _ = client.o_auth();
	let _ = client.pages();
	let _ = client.posts();
	let _ = client.profile_posts();
	let _ = client.search();
	let _ = client.tags();
	let _ = client.threads();
	let _ = client.users();
}

// ---------------------------------------------------------------------------
// MarketClient construction
// ---------------------------------------------------------------------------

#[test]
fn market_client_new_succeeds() {
	let client = MarketClient::new("test-token");
	assert!(client.is_ok());
}

#[test]
fn market_client_with_config_succeeds() {
	let config = ClientConfig {
		token: "test-token".to_string(),
		base_url: "https://prod-api.lzt.market".to_string(),
		proxy: None,
		retry: RetryConfig {
			max_retries: 2,
			base_delay_ms: 500,
			max_delay_ms: 15000,
		},
		rate_limit: Some(RateLimitConfig {
			requests_per_minute: 120,
		}),
		search_rate_limit: None,
	};
	let client = MarketClient::with_config(config);
	assert!(client.is_ok());
}

#[test]
fn market_client_accepts_string_token() {
	let token = String::from("my-token");
	let client = MarketClient::new(token);
	assert!(client.is_ok());
}

/// Verify all 13 API group accessors compile and return references.
#[test]
fn market_client_all_api_group_accessors() {
	let client = MarketClient::new("token").unwrap();

	let _ = client.auto_payments();
	let _ = client.batch();
	let _ = client.cart();
	let _ = client.category();
	let _ = client.custom_discounts();
	let _ = client.imap();
	let _ = client.list();
	let _ = client.managing();
	let _ = client.payments();
	let _ = client.profile();
	let _ = client.proxy();
	let _ = client.publishing();
	let _ = client.purchasing();
}

// ---------------------------------------------------------------------------
// Proxy URL validation
// ---------------------------------------------------------------------------

#[test]
fn proxy_rejects_unsupported_scheme() {
	let cfg = ClientConfig {
		token: "t".to_string(),
		base_url: "https://example.com".to_string(),
		proxy: Some(ProxyConfig {
			url: "ftp://proxy:8080".to_string(),
		}),
		retry: RetryConfig::default(),
		rate_limit: None,
		search_rate_limit: None,
	};
	let result = ForumClient::with_config(cfg);
	assert!(result.is_err());
	assert!(matches!(result, Err(LolzteamError::Config(_))));
}

#[test]
fn proxy_rejects_invalid_url() {
	let cfg = ClientConfig {
		token: "t".to_string(),
		base_url: "https://example.com".to_string(),
		proxy: Some(ProxyConfig {
			url: "not a url".to_string(),
		}),
		retry: RetryConfig::default(),
		rate_limit: None,
		search_rate_limit: None,
	};
	let result = ForumClient::with_config(cfg);
	assert!(result.is_err());
	assert!(matches!(result, Err(LolzteamError::Config(_))));
}

#[test]
fn proxy_rejects_no_host() {
	let cfg = ClientConfig {
		token: "t".to_string(),
		base_url: "https://example.com".to_string(),
		proxy: Some(ProxyConfig {
			url: "http://".to_string(),
		}),
		retry: RetryConfig::default(),
		rate_limit: None,
		search_rate_limit: None,
	};
	let result = ForumClient::with_config(cfg);
	assert!(result.is_err());
	assert!(matches!(result, Err(LolzteamError::Config(_))));
}

#[test]
fn proxy_accepts_valid_http() {
	let cfg = ClientConfig {
		token: "t".to_string(),
		base_url: "https://example.com".to_string(),
		proxy: Some(ProxyConfig {
			url: "http://proxy:8080".to_string(),
		}),
		retry: RetryConfig::default(),
		rate_limit: None,
		search_rate_limit: None,
	};
	let result = ForumClient::with_config(cfg);
	assert!(result.is_ok());
}

#[test]
fn proxy_accepts_valid_socks5() {
	let cfg = ClientConfig {
		token: "t".to_string(),
		base_url: "https://example.com".to_string(),
		proxy: Some(ProxyConfig {
			url: "socks5://127.0.0.1:1080".to_string(),
		}),
		retry: RetryConfig::default(),
		rate_limit: None,
		search_rate_limit: None,
	};
	let result = ForumClient::with_config(cfg);
	assert!(result.is_ok());
}

// ---------------------------------------------------------------------------
// LolzteamError variant matching
// ---------------------------------------------------------------------------

#[test]
fn network_error_converts_to_lolzteam_error() {
	// We can't easily construct a reqwest::Error, but we can test the enum pattern
	let http_err = HttpError {
		status: 503,
		body: serde_json::Value::Null,
		retry_after: None,
	};
	let err: LolzteamError = http_err.into();
	match &err {
		LolzteamError::Http(h) => {
			assert!(h.is_server_error());
			assert!(h.is_retryable());
		}
		_ => panic!("expected Http variant"),
	}
}

#[test]
fn http_error_429_retry_after_none() {
	let err = HttpError {
		status: 429,
		body: serde_json::Value::Null,
		retry_after: None,
	};
	assert!(err.is_rate_limit());
	assert!(err.is_retryable());
	assert_eq!(err.retry_after_secs(), None);
}

#[test]
fn http_error_display_contains_body() {
	let err = HttpError {
		status: 400,
		body: serde_json::json!({"message": "bad request"}),
		retry_after: None,
	};
	let display = format!("{err}");
	assert!(display.contains("400"));
	assert!(display.contains("bad request"));
}

// ---------------------------------------------------------------------------
// Both clients can coexist
// ---------------------------------------------------------------------------

#[test]
fn both_clients_can_coexist() {
	let forum = ForumClient::new("forum-token").unwrap();
	let market = MarketClient::new("market-token").unwrap();

	// Smoke-check that both are usable simultaneously.
	let _ = forum.threads();
	let _ = market.list();
}

// ---------------------------------------------------------------------------
// HTTP-level mock tests
// ---------------------------------------------------------------------------

/// 1. Successful request — mock returns 200 JSON, verify client parses it.
#[tokio::test]
async fn http_mock_successful_request() {
	let (listener, base_url) = mock_listener().await;
	let client = test_client(&base_url, "test-token");

	tokio::spawn(async move {
		let (mut stream, _) = listener.accept().await.unwrap();
		let _ = read_request(&mut stream).await;
		write_response(&mut stream, 200, "", r#"{"ok":true,"value":42}"#).await;
	});

	let result: serde_json::Value = client.request("GET", "/test", None, None, false).await.unwrap();
	assert_eq!(result["ok"], true);
	assert_eq!(result["value"], 42);
}

/// 2. Auth header — verify Bearer token is sent in Authorization header.
#[tokio::test]
async fn http_mock_auth_header_sent() {
	let (listener, base_url) = mock_listener().await;
	let client = test_client(&base_url, "secret-token-123");

	let handle = tokio::spawn(async move {
		let (mut stream, _) = listener.accept().await.unwrap();
		let req = read_request(&mut stream).await;
		write_response(&mut stream, 200, "", r#"{"ok":true}"#).await;
		req
	});

	let _: serde_json::Value = client
		.request("GET", "/auth-check", None, None, false)
		.await
		.unwrap();
	let req = handle.await.unwrap();
	let req_lower = req.to_lowercase();
	assert!(
		req_lower.contains("authorization: bearer secret-token-123"),
		"request should contain Bearer token, got: {req}"
	);
}

/// 3. 401 → auth error.
#[tokio::test]
async fn http_mock_401_auth_error() {
	let (listener, base_url) = mock_listener().await;
	let client = test_client(&base_url, "bad-token");

	tokio::spawn(async move {
		let (mut stream, _) = listener.accept().await.unwrap();
		let _ = read_request(&mut stream).await;
		write_response(&mut stream, 401, "", r#"{"error":"unauthorized"}"#).await;
	});

	let result: Result<serde_json::Value, LolzteamError> =
		client.request("GET", "/secret", None, None, false).await;
	let err = result.unwrap_err();
	match &err {
		LolzteamError::Http(http_err) => {
			assert!(http_err.is_auth_error());
			assert_eq!(http_err.status, 401);
		}
		other => panic!("expected Http error, got: {other:?}"),
	}
}

/// 4. 404 → not found.
#[tokio::test]
async fn http_mock_404_not_found() {
	let (listener, base_url) = mock_listener().await;
	let client = test_client(&base_url, "token");

	tokio::spawn(async move {
		let (mut stream, _) = listener.accept().await.unwrap();
		let _ = read_request(&mut stream).await;
		write_response(&mut stream, 404, "", r#"{"error":"not found"}"#).await;
	});

	let result: Result<serde_json::Value, LolzteamError> =
		client.request("GET", "/missing", None, None, false).await;
	let err = result.unwrap_err();
	match &err {
		LolzteamError::Http(http_err) => {
			assert!(http_err.is_not_found());
			assert_eq!(http_err.status, 404);
		}
		other => panic!("expected Http error, got: {other:?}"),
	}
}

/// 5. 429 retry then success — mock returns 429 first, then 200.
#[tokio::test]
async fn http_mock_429_retry_then_success() {
	let (listener, base_url) = mock_listener().await;
	let client = test_client(&base_url, "token");

	let call_count = Arc::new(AtomicUsize::new(0));
	let counter = Arc::clone(&call_count);

	tokio::spawn(async move {
		// First request: 429
		let (mut stream, _) = listener.accept().await.unwrap();
		let _ = read_request(&mut stream).await;
		counter.fetch_add(1, Ordering::SeqCst);
		write_response(
			&mut stream,
			429,
			"Retry-After: 0\r\n",
			r#"{"error":"rate limited"}"#,
		)
		.await;

		// Second request: 200
		let (mut stream, _) = listener.accept().await.unwrap();
		let _ = read_request(&mut stream).await;
		counter.fetch_add(1, Ordering::SeqCst);
		write_response(&mut stream, 200, "", r#"{"retried":true}"#).await;
	});

	let result: serde_json::Value = client
		.request("GET", "/rate-limited", None, None, false)
		.await
		.unwrap();
	assert_eq!(result["retried"], true);
	assert_eq!(call_count.load(Ordering::SeqCst), 2);
}

/// 6. 502/503 retry — mock returns 502 first, then 200.
#[tokio::test]
async fn http_mock_502_retry_then_success() {
	let (listener, base_url) = mock_listener().await;
	let client = test_client(&base_url, "token");

	let call_count = Arc::new(AtomicUsize::new(0));
	let counter = Arc::clone(&call_count);

	tokio::spawn(async move {
		// First request: 502
		let (mut stream, _) = listener.accept().await.unwrap();
		let _ = read_request(&mut stream).await;
		counter.fetch_add(1, Ordering::SeqCst);
		write_response(&mut stream, 502, "", r#"{"error":"bad gateway"}"#).await;

		// Second request: 200
		let (mut stream, _) = listener.accept().await.unwrap();
		let _ = read_request(&mut stream).await;
		counter.fetch_add(1, Ordering::SeqCst);
		write_response(&mut stream, 200, "", r#"{"recovered":true}"#).await;
	});

	let result: serde_json::Value = client
		.request("GET", "/unstable", None, None, false)
		.await
		.unwrap();
	assert_eq!(result["recovered"], true);
	assert_eq!(call_count.load(Ordering::SeqCst), 2);
}

/// 7. Path params — verify correct URL path is requested.
#[tokio::test]
async fn http_mock_path_params() {
	let (listener, base_url) = mock_listener().await;
	let client = test_client(&base_url, "token");

	let handle = tokio::spawn(async move {
		let (mut stream, _) = listener.accept().await.unwrap();
		let req = read_request(&mut stream).await;
		write_response(&mut stream, 200, "", r#"{"thread_id":123}"#).await;
		req
	});

	// Simulate what threads().get(123, None) does: builds path "/threads/123"
	let path = format!("/threads/{}", 123);
	let _: serde_json::Value = client.request("GET", &path, None, None, false).await.unwrap();
	let req = handle.await.unwrap();
	assert!(
		req.contains("GET /threads/123"),
		"request should contain path /threads/123, got: {req}"
	);
}

/// 8. Query params — verify query string is correctly appended.
#[tokio::test]
async fn http_mock_query_params() {
	let (listener, base_url) = mock_listener().await;
	let client = test_client(&base_url, "token");

	let handle = tokio::spawn(async move {
		let (mut stream, _) = listener.accept().await.unwrap();
		let req = read_request(&mut stream).await;
		write_response(&mut stream, 200, "", r#"{"threads":[]}"#).await;
		req
	});

	let query = vec![
		("forum_id".to_string(), ParamValue::Integer(42)),
		("limit".to_string(), ParamValue::Integer(10)),
		("sticky".to_string(), ParamValue::Bool(true)),
	];
	let _: serde_json::Value = client
		.request("GET", "/threads", Some(query.as_slice()), None, false)
		.await
		.unwrap();
	let req = handle.await.unwrap();
	assert!(
		req.contains("forum_id=42"),
		"query should contain forum_id=42, got: {req}"
	);
	assert!(
		req.contains("limit=10"),
		"query should contain limit=10, got: {req}"
	);
	assert!(
		req.contains("sticky=true"),
		"query should contain sticky=true, got: {req}"
	);
}
