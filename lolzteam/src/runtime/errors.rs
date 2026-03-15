use std::fmt;

/// Top-level error type for the Lolzteam API client.
#[derive(Debug, thiserror::Error)]
pub enum LolzteamError {
	/// HTTP error — the server returned a non-2xx status code.
	#[error("{0}")]
	Http(#[from] HttpError),

	/// Network error — connection, DNS, timeout, or TLS failure.
	#[error("{0}")]
	Network(#[from] NetworkError),

	/// Configuration error — invalid client configuration.
	#[error("{0}")]
	Config(#[from] ConfigError),
}

/// An HTTP error response from the API.
#[derive(Debug)]
pub struct HttpError {
	/// HTTP status code.
	pub status: u16,
	/// Parsed response body (may be an error object or empty object).
	pub body: serde_json::Value,
	/// Value of the `Retry-After` header on 429 responses, in seconds.
	pub retry_after: Option<u64>,
}

impl HttpError {
	/// Create from a reqwest response (consumes the response).
	pub async fn from_response(response: reqwest::Response) -> Self {
		let status = response.status().as_u16();

		let retry_after = response
			.headers()
			.get("retry-after")
			.and_then(|v| v.to_str().ok())
			.and_then(|v| v.parse::<u64>().ok());

		let body = response
			.json::<serde_json::Value>()
			.await
			.unwrap_or(serde_json::Value::Null);

		Self {
			status,
			body,
			retry_after,
		}
	}

	/// Returns `true` if this is a 429 Too Many Requests response.
	#[must_use]
	pub fn is_rate_limit(&self) -> bool {
		self.status == 429
	}

	/// Returns `true` if this is a 5xx server error.
	#[must_use]
	pub fn is_server_error(&self) -> bool {
		self.status >= 500
	}

	/// Returns `true` if this is a 401 or 403 authentication/authorization error.
	#[must_use]
	pub fn is_auth_error(&self) -> bool {
		self.status == 401 || self.status == 403
	}

	/// Returns `true` if this is a 404 Not Found response.
	#[must_use]
	pub fn is_not_found(&self) -> bool {
		self.status == 404
	}

	/// Returns the Retry-After value in seconds, if present.
	#[must_use]
	pub fn retry_after_secs(&self) -> Option<u64> {
		self.retry_after
	}

	/// Returns `true` if this error should be retried (429, 502, 503, 504).
	#[must_use]
	pub fn is_retryable(&self) -> bool {
		self.status == 429 || self.status == 502 || self.status == 503 || self.status == 504
	}
}

impl fmt::Display for HttpError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "HTTP {}: {}", self.status, self.body)
	}
}

impl std::error::Error for HttpError {}

/// A network-level error (wraps reqwest::Error).
#[derive(Debug, thiserror::Error)]
#[error("network error: {0}")]
pub struct NetworkError(#[from] pub reqwest::Error);

impl NetworkError {
	/// Returns `true` if the underlying error is transient (timeout or connection failure).
	#[must_use]
	pub fn is_transient(&self) -> bool {
		self.0.is_timeout() || self.0.is_connect()
	}
}

/// A configuration error (e.g. invalid proxy URL).
#[derive(Debug, thiserror::Error)]
#[error("config error: {0}")]
pub struct ConfigError(pub String);
