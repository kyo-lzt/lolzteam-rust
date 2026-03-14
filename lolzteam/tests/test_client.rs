use lolzteam::generated::forum::ForumClient;
use lolzteam::generated::market::MarketClient;
use lolzteam::runtime::{
    ClientConfig, HttpError, LolzteamError, ProxyConfig, RateLimitConfig, RetryConfig,
};

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
        base_url: "https://api.lolz.live".to_string(),
        proxy: None,
        retry: RetryConfig {
            max_retries: 5,
            base_delay_ms: 2000,
            max_delay_ms: 60000,
        },
        rate_limit: Some(RateLimitConfig {
            requests_per_minute: 300,
        }),
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
        base_url: "https://api.lzt.market".to_string(),
        proxy: None,
        retry: RetryConfig {
            max_retries: 2,
            base_delay_ms: 500,
            max_delay_ms: 15000,
        },
        rate_limit: Some(RateLimitConfig {
            requests_per_minute: 120,
        }),
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
