use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::runtime::errors::{ConfigError, HttpError, NetworkError};
use crate::runtime::rate_limiter::RateLimiter;
use crate::runtime::retry::with_retry;
use crate::runtime::types::{ClientConfig, MultipartPart, ParamValue, RetryConfig, RetryInfo};
use crate::runtime::LolzteamError;

/// HTTP client for the Lolzteam API.
///
/// Handles authentication, rate limiting, retries, and proxy configuration.
/// Designed to be wrapped in `Arc` and shared across API resource structs.
pub struct HttpClient {
	client: reqwest::Client,
	base_url: String,
	token: String,
	rate_limiter: Option<RateLimiter>,
	search_rate_limiter: Option<RateLimiter>,
	retry_config: Option<RetryConfig>,
	on_retry: Option<Arc<dyn Fn(RetryInfo) + Send + Sync>>,
}

impl HttpClient {
	/// Build a new HTTP client from the given configuration.
	///
	/// # Errors
	///
	/// Returns `LolzteamError::Network` if the underlying reqwest client
	/// cannot be constructed (e.g. invalid proxy URL).
	pub fn new(config: ClientConfig) -> Result<Self, LolzteamError> {
		let mut builder = reqwest::Client::builder();

		if let Some(proxy_cfg) = &config.proxy {
			let url = reqwest::Url::parse(&proxy_cfg.url)
				.map_err(|e| ConfigError(format!("invalid proxy URL: {e}")))?;
			match url.scheme() {
				"http" | "https" | "socks5" => {}
				other => {
					return Err(ConfigError(format!("unsupported proxy scheme: {other}")).into())
				}
			}
			if url.host().is_none() {
				return Err(ConfigError("proxy URL has no host".to_string()).into());
			}
			let proxy = reqwest::Proxy::all(&proxy_cfg.url).map_err(NetworkError)?;
			builder = builder.proxy(proxy);
		}

		if let Some(timeout_ms) = config.timeout_ms {
			builder = builder.timeout(std::time::Duration::from_millis(timeout_ms));
		}

		let client = builder.build().map_err(NetworkError)?;

		let rate_limiter = config
			.rate_limit
			.map(|rl| RateLimiter::new(rl.requests_per_minute));

		let search_rate_limiter = config
			.search_rate_limit
			.map(|rl| RateLimiter::new(rl.requests_per_minute));

		Ok(Self {
			client,
			base_url: config.base_url,
			token: config.token,
			rate_limiter,
			search_rate_limiter,
			retry_config: config.retry,
			on_retry: config.on_retry,
		})
	}

	/// Send an API request with rate limiting and retry.
	///
	/// # Arguments
	///
	/// * `method` — HTTP method (GET, POST, PUT, DELETE, etc.)
	/// * `path` — URL path appended to `base_url` (e.g. `/threads/123`)
	/// * `query` — Optional query string parameters
	/// * `body` — Optional form-encoded body parameters
	///
	/// # Errors
	///
	/// Returns `LolzteamError::Http` on non-2xx responses or
	/// `LolzteamError::Network` on connection failures.
	pub async fn request<T: DeserializeOwned>(
		&self,
		method: &str,
		path: &str,
		query: Option<&[(String, ParamValue)]>,
		body: Option<&[(String, ParamValue)]>,
		is_search: bool,
	) -> Result<T, LolzteamError> {
		// Rate limit before making the request.
		if let Some(ref limiter) = self.rate_limiter {
			limiter.acquire().await;
		}
		if is_search {
			if let Some(ref limiter) = self.search_rate_limiter {
				limiter.acquire().await;
			}
		}

		let url = format!("{}{}", self.base_url, path);
		let http_method = method
			.parse::<reqwest::Method>()
			.unwrap_or(reqwest::Method::GET);

		match &self.retry_config {
			Some(cfg) => {
				with_retry(cfg, self.on_retry.as_ref(), method, path, || {
					self.execute_request::<T>(http_method.clone(), &url, query, body)
				})
				.await
			}
			None => {
				self.execute_request::<T>(http_method, &url, query, body)
					.await
			}
		}
	}

	/// Send a request with JSON body.
	///
	/// # Arguments
	///
	/// * `method` — HTTP method (POST, PUT, etc.)
	/// * `path` — URL path appended to `base_url`
	/// * `query` — Optional query string parameters
	/// * `body` — Optional JSON-serializable body
	///
	/// # Errors
	///
	/// Returns `LolzteamError::Http` on non-2xx responses or
	/// `LolzteamError::Network` on connection failures.
	pub async fn request_json<T: DeserializeOwned, B: Serialize + Sync>(
		&self,
		method: &str,
		path: &str,
		query: Option<&[(String, ParamValue)]>,
		body: Option<&B>,
		is_search: bool,
	) -> Result<T, LolzteamError> {
		if let Some(ref limiter) = self.rate_limiter {
			limiter.acquire().await;
		}
		if is_search {
			if let Some(ref limiter) = self.search_rate_limiter {
				limiter.acquire().await;
			}
		}

		let url = format!("{}{}", self.base_url, path);
		let http_method = method
			.parse::<reqwest::Method>()
			.unwrap_or(reqwest::Method::POST);

		match &self.retry_config {
			Some(cfg) => {
				with_retry(cfg, self.on_retry.as_ref(), method, path, || {
					self.execute_json_request::<T, B>(http_method.clone(), &url, query, body)
				})
				.await
			}
			None => {
				self.execute_json_request::<T, B>(http_method, &url, query, body)
					.await
			}
		}
	}

	/// Send a request expecting a text response (not JSON).
	///
	/// # Arguments
	///
	/// * `method` — HTTP method
	/// * `path` — URL path appended to `base_url`
	/// * `query` — Optional query string parameters
	///
	/// # Errors
	///
	/// Returns `LolzteamError::Http` on non-2xx responses or
	/// `LolzteamError::Network` on connection failures.
	pub async fn request_text(
		&self,
		method: &str,
		path: &str,
		query: Option<&[(String, ParamValue)]>,
		is_search: bool,
	) -> Result<String, LolzteamError> {
		if let Some(ref limiter) = self.rate_limiter {
			limiter.acquire().await;
		}
		if is_search {
			if let Some(ref limiter) = self.search_rate_limiter {
				limiter.acquire().await;
			}
		}

		let url = format!("{}{}", self.base_url, path);
		let http_method = method
			.parse::<reqwest::Method>()
			.unwrap_or(reqwest::Method::GET);

		match &self.retry_config {
			Some(cfg) => {
				with_retry(cfg, self.on_retry.as_ref(), method, path, || {
					self.execute_text_request(http_method.clone(), &url, query)
				})
				.await
			}
			None => self.execute_text_request(http_method, &url, query).await,
		}
	}

	/// Send an API request with multipart/form-data body.
	///
	/// # Arguments
	///
	/// * `method` — HTTP method (POST, PUT, etc.)
	/// * `path` — URL path appended to `base_url`
	/// * `parts` — Multipart form parts (text fields and/or binary files)
	///
	/// # Errors
	///
	/// Returns `LolzteamError::Http` on non-2xx responses or
	/// `LolzteamError::Network` on connection failures.
	pub async fn request_multipart<T: DeserializeOwned>(
		&self,
		method: &str,
		path: &str,
		parts: Vec<MultipartPart>,
		is_search: bool,
	) -> Result<T, LolzteamError> {
		if let Some(ref limiter) = self.rate_limiter {
			limiter.acquire().await;
		}
		if is_search {
			if let Some(ref limiter) = self.search_rate_limiter {
				limiter.acquire().await;
			}
		}

		let url = format!("{}{}", self.base_url, path);
		let http_method = method
			.parse::<reqwest::Method>()
			.unwrap_or(reqwest::Method::POST);

		match &self.retry_config {
			Some(cfg) => {
				with_retry(cfg, self.on_retry.as_ref(), method, path, || {
					self.execute_multipart_request::<T>(http_method.clone(), &url, &parts)
				})
				.await
			}
			None => {
				self.execute_multipart_request::<T>(http_method, &url, &parts)
					.await
			}
		}
	}

	/// Execute a single multipart HTTP request (no retry, no rate limit).
	async fn execute_multipart_request<T: DeserializeOwned>(
		&self,
		method: reqwest::Method,
		url: &str,
		parts: &[MultipartPart],
	) -> Result<T, LolzteamError> {
		let mut form = reqwest::multipart::Form::new();

		for part in parts {
			match part {
				MultipartPart::Text { name, value } => {
					form = form.text(name.clone(), value.clone());
				}
				MultipartPart::File {
					name,
					data,
					filename,
					mime_type,
				} => {
					let mut file_part = reqwest::multipart::Part::bytes(data.clone());
					if let Some(fname) = filename {
						file_part = file_part.file_name(fname.clone());
					}
					if let Some(mime) = mime_type {
						file_part = file_part.mime_str(mime).map_err(|e| {
							ConfigError(format!("invalid MIME type '{}': {}", mime, e))
						})?;
					}
					form = form.part(name.clone(), file_part);
				}
			}
		}

		let response = self
			.client
			.request(method, url)
			.header("Authorization", format!("Bearer {}", self.token))
			.multipart(form)
			.send()
			.await
			.map_err(NetworkError)?;

		if !response.status().is_success() {
			let http_error = HttpError::from_response(response).await;
			return Err(LolzteamError::Http(http_error));
		}

		let parsed: T = response.json().await.map_err(NetworkError)?;
		Ok(parsed)
	}

	/// Execute a single JSON-body HTTP request (no retry, no rate limit).
	async fn execute_json_request<T: DeserializeOwned, B: Serialize + Sync>(
		&self,
		method: reqwest::Method,
		url: &str,
		query: Option<&[(String, ParamValue)]>,
		body: Option<&B>,
	) -> Result<T, LolzteamError> {
		let mut builder = self
			.client
			.request(method, url)
			.header("Authorization", format!("Bearer {}", self.token));

		if let Some(params) = query {
			let pairs: Vec<(&String, String)> =
				params.iter().map(|(k, v)| (k, v.to_string())).collect();
			builder = builder.query(&pairs);
		}

		if let Some(b) = body {
			builder = builder.json(b);
		}

		let response = builder.send().await.map_err(NetworkError)?;

		if !response.status().is_success() {
			let http_error = HttpError::from_response(response).await;
			return Err(LolzteamError::Http(http_error));
		}

		let parsed: T = response.json().await.map_err(NetworkError)?;
		Ok(parsed)
	}

	/// Execute a single text HTTP request (no retry, no rate limit).
	async fn execute_text_request(
		&self,
		method: reqwest::Method,
		url: &str,
		query: Option<&[(String, ParamValue)]>,
	) -> Result<String, LolzteamError> {
		let mut builder = self
			.client
			.request(method, url)
			.header("Authorization", format!("Bearer {}", self.token));

		if let Some(params) = query {
			let pairs: Vec<(&String, String)> =
				params.iter().map(|(k, v)| (k, v.to_string())).collect();
			builder = builder.query(&pairs);
		}

		let response = builder.send().await.map_err(NetworkError)?;

		if !response.status().is_success() {
			let http_error = HttpError::from_response(response).await;
			return Err(LolzteamError::Http(http_error));
		}

		let text = response.text().await.map_err(NetworkError)?;
		Ok(text)
	}

	/// Execute a single HTTP request (no retry, no rate limit).
	async fn execute_request<T: DeserializeOwned>(
		&self,
		method: reqwest::Method,
		url: &str,
		query: Option<&[(String, ParamValue)]>,
		body: Option<&[(String, ParamValue)]>,
	) -> Result<T, LolzteamError> {
		let mut builder = self
			.client
			.request(method, url)
			.header("Authorization", format!("Bearer {}", self.token));

		// Append query parameters.
		if let Some(params) = query {
			let pairs: Vec<(&String, String)> =
				params.iter().map(|(k, v)| (k, v.to_string())).collect();
			builder = builder.query(&pairs);
		}

		// Append form body.
		if let Some(params) = body {
			let pairs: Vec<(&String, String)> =
				params.iter().map(|(k, v)| (k, v.to_string())).collect();
			builder = builder.form(&pairs);
		}

		let response = builder.send().await.map_err(NetworkError)?;

		if !response.status().is_success() {
			let http_error = HttpError::from_response(response).await;
			return Err(LolzteamError::Http(http_error));
		}

		let parsed: T = response.json().await.map_err(NetworkError)?;
		Ok(parsed)
	}
}
