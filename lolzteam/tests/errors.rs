use lolzteam::runtime::{ConfigError, HttpError, LolzteamError};

// ---------------------------------------------------------------------------
// HttpError classification
// ---------------------------------------------------------------------------

#[test]
fn rate_limit_429() {
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
fn retryable_statuses() {
	for status in [429, 502, 503, 504] {
		let err = HttpError {
			status,
			body: serde_json::Value::Null,
			retry_after: None,
		};
		assert!(err.is_retryable(), "{status} should be retryable");
	}
}

#[test]
fn non_retryable_statuses() {
	for status in [400, 401, 403, 404, 500, 418] {
		let err = HttpError {
			status,
			body: serde_json::Value::Null,
			retry_after: None,
		};
		assert!(!err.is_retryable(), "{status} should NOT be retryable");
	}
}

#[test]
fn auth_error_statuses() {
	for status in [401, 403] {
		let err = HttpError {
			status,
			body: serde_json::Value::Null,
			retry_after: None,
		};
		assert!(err.is_auth_error(), "{status} should be auth error");
	}
	// 400 is not an auth error
	let err = HttpError {
		status: 400,
		body: serde_json::Value::Null,
		retry_after: None,
	};
	assert!(!err.is_auth_error());
}

#[test]
fn not_found_404() {
	let err = HttpError {
		status: 404,
		body: serde_json::Value::Null,
		retry_after: None,
	};
	assert!(err.is_not_found());
	assert!(!err.is_retryable());
}

#[test]
fn server_error_5xx() {
	for status in [500, 501, 502, 503, 504] {
		let err = HttpError {
			status,
			body: serde_json::Value::Null,
			retry_after: None,
		};
		assert!(err.is_server_error(), "{status} should be server error");
	}
	let err = HttpError {
		status: 499,
		body: serde_json::Value::Null,
		retry_after: None,
	};
	assert!(!err.is_server_error());
}

#[test]
fn retry_after_none_when_absent() {
	let err = HttpError {
		status: 429,
		body: serde_json::Value::Null,
		retry_after: None,
	};
	assert_eq!(err.retry_after_secs(), None);
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

#[test]
fn http_error_display_contains_status_and_body() {
	let err = HttpError {
		status: 422,
		body: serde_json::json!({"message": "invalid field"}),
		retry_after: None,
	};
	let msg = format!("{err}");
	assert!(msg.contains("422"), "display should contain status");
	assert!(msg.contains("invalid field"), "display should contain body");
}

// ---------------------------------------------------------------------------
// LolzteamError conversions
// ---------------------------------------------------------------------------

#[test]
fn http_error_into_lolzteam_error() {
	let http_err = HttpError {
		status: 500,
		body: serde_json::Value::Null,
		retry_after: None,
	};
	let err: LolzteamError = http_err.into();
	assert!(matches!(err, LolzteamError::Http(_)));
}

#[test]
fn config_error_into_lolzteam_error() {
	let cfg_err = ConfigError("bad proxy".to_string());
	let err: LolzteamError = cfg_err.into();
	match &err {
		LolzteamError::Config(e) => {
			let msg = format!("{e}");
			assert!(msg.contains("bad proxy"));
		}
		other => panic!("expected Config, got: {other}"),
	}
}

// ---------------------------------------------------------------------------
// RetryExhausted
// ---------------------------------------------------------------------------

#[test]
fn retry_exhausted_display() {
	let inner = HttpError {
		status: 503,
		body: serde_json::Value::Null,
		retry_after: None,
	};
	let err = LolzteamError::RetryExhausted {
		attempts: 4,
		last_error: Box::new(LolzteamError::Http(inner)),
	};
	let msg = format!("{err}");
	assert!(msg.contains("4 attempts"), "display: {msg}");
}

#[test]
fn retry_exhausted_inner_error_accessible() {
	let inner = HttpError {
		status: 429,
		body: serde_json::Value::Null,
		retry_after: None,
	};
	let err = LolzteamError::RetryExhausted {
		attempts: 3,
		last_error: Box::new(LolzteamError::Http(inner)),
	};
	match &err {
		LolzteamError::RetryExhausted { last_error, .. } => {
			assert!(matches!(**last_error, LolzteamError::Http(_)));
		}
		_ => unreachable!(),
	}
}

// ---------------------------------------------------------------------------
// ConfigError
// ---------------------------------------------------------------------------

#[test]
fn config_error_display() {
	let err = ConfigError("invalid URL".to_string());
	let msg = format!("{err}");
	assert!(msg.contains("invalid URL"));
}

// ---------------------------------------------------------------------------
// LolzteamError variant matching
// ---------------------------------------------------------------------------

#[test]
fn config_error_is_not_retryable_variant() {
	let err: LolzteamError = ConfigError("bad".to_string()).into();
	// ConfigError should never match Http or Network.
	assert!(matches!(err, LolzteamError::Config(_)));
	assert!(!matches!(err, LolzteamError::Http(_)));
	assert!(!matches!(err, LolzteamError::Network(_)));
}

#[test]
fn lolzteam_error_implements_std_error() {
	let err: LolzteamError = ConfigError("test".to_string()).into();
	// Verify it implements std::error::Error via Display + Debug.
	let _debug = format!("{err:?}");
	let _display = format!("{err}");
}
