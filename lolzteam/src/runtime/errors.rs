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

	/// All retry attempts exhausted.
	#[error("request failed after {attempts} attempts: {last_error}")]
	RetryExhausted {
		attempts: u32,
		last_error: Box<LolzteamError>,
	},
}

/// Shared fields present on every HTTP error response.
#[derive(Debug, Clone)]
pub struct HttpErrorData {
	/// HTTP status code.
	pub status: u16,
	/// Parsed response body (may be an error object or empty object).
	pub body: serde_json::Value,
}

/// An HTTP error response from the API.
///
/// Each variant carries [`HttpErrorData`] with the status code and body.
/// Users can pattern-match on variants or use the convenience `is_*()` methods.
#[derive(Debug)]
pub enum HttpError {
	/// 401 Unauthorized.
	Auth(HttpErrorData),
	/// 403 Forbidden.
	Forbidden(HttpErrorData),
	/// 404 Not Found.
	NotFound(HttpErrorData),
	/// 429 Too Many Requests.
	RateLimit {
		data: HttpErrorData,
		/// Value of the `Retry-After` header, in seconds.
		retry_after: Option<u64>,
	},
	/// 5xx Server Error.
	Server(HttpErrorData),
	/// Any other non-2xx status code.
	Other(HttpErrorData),
}

impl HttpError {
	/// Create an `HttpError` from a status code, body, and optional retry-after.
	///
	/// Automatically selects the correct variant based on the status code.
	#[must_use]
	pub fn new(status: u16, body: serde_json::Value, retry_after: Option<u64>) -> Self {
		let data = HttpErrorData { status, body };
		match status {
			401 => Self::Auth(data),
			403 => Self::Forbidden(data),
			404 => Self::NotFound(data),
			429 => Self::RateLimit { data, retry_after },
			500..=599 => Self::Server(data),
			_ => Self::Other(data),
		}
	}

	/// Create from a reqwest response (consumes the response).
	pub async fn from_response(response: reqwest::Response) -> Self {
		let status = response.status().as_u16();

		let retry_after = response
			.headers()
			.get("retry-after")
			.and_then(|v| v.to_str().ok())
			.and_then(|v| {
				v.parse::<u64>().ok().or_else(|| {
					httpdate::parse_http_date(v).ok().and_then(|date| {
						let now = std::time::SystemTime::now();
						date.duration_since(now).ok().map(|d| d.as_secs())
					})
				})
			});

		let body = response
			.json::<serde_json::Value>()
			.await
			.unwrap_or(serde_json::Value::Null);

		let data = HttpErrorData { status, body };

		match status {
			401 => Self::Auth(data),
			403 => Self::Forbidden(data),
			404 => Self::NotFound(data),
			429 => Self::RateLimit { data, retry_after },
			500..=599 => Self::Server(data),
			_ => Self::Other(data),
		}
	}

	/// Access the shared error data regardless of variant.
	#[must_use]
	pub fn data(&self) -> &HttpErrorData {
		match self {
			Self::Auth(d)
			| Self::Forbidden(d)
			| Self::NotFound(d)
			| Self::Server(d)
			| Self::Other(d) => d,
			Self::RateLimit { data, .. } => data,
		}
	}

	/// HTTP status code.
	#[must_use]
	pub fn status(&self) -> u16 {
		self.data().status
	}

	/// Parsed response body.
	#[must_use]
	pub fn body(&self) -> &serde_json::Value {
		&self.data().body
	}

	/// Returns `true` if this is a 429 Too Many Requests response.
	#[must_use]
	pub fn is_rate_limit(&self) -> bool {
		matches!(self, Self::RateLimit { .. })
	}

	/// Returns `true` if this is a 5xx server error.
	#[must_use]
	pub fn is_server_error(&self) -> bool {
		matches!(self, Self::Server(_))
	}

	/// Returns `true` if this is a 401 authentication error.
	#[must_use]
	pub fn is_auth_error(&self) -> bool {
		matches!(self, Self::Auth(_))
	}

	/// Returns `true` if this is a 403 forbidden error.
	#[must_use]
	pub fn is_forbidden(&self) -> bool {
		matches!(self, Self::Forbidden(_))
	}

	/// Returns `true` if this is a 404 Not Found response.
	#[must_use]
	pub fn is_not_found(&self) -> bool {
		matches!(self, Self::NotFound(_))
	}

	/// Returns the Retry-After value in seconds, if present.
	#[must_use]
	pub fn retry_after_secs(&self) -> Option<u64> {
		match self {
			Self::RateLimit { retry_after, .. } => *retry_after,
			_ => None,
		}
	}

	/// Returns `true` if this error should be retried (429, 502, 503, 504).
	#[must_use]
	pub fn is_retryable(&self) -> bool {
		match self {
			Self::RateLimit { .. } => true,
			Self::Server(d) => matches!(d.status, 502..=504),
			_ => false,
		}
	}
}

impl fmt::Display for HttpError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let d = self.data();
		write!(f, "HTTP {}: {}", d.status, d.body)
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
