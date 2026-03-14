use serde::Serialize;

/// A single part of a multipart form body.
#[derive(Debug, Clone)]
pub enum MultipartPart {
	/// A text field.
	Text { name: String, value: String },
	/// A binary file field.
	File {
		name: String,
		data: Vec<u8>,
		filename: Option<String>,
		mime_type: Option<String>,
	},
}

/// Top-level client configuration.
#[derive(Debug, Clone)]
pub struct ClientConfig {
	pub token: String,
	pub base_url: String,
	pub proxy: Option<ProxyConfig>,
	pub retry: RetryConfig,
	pub rate_limit: Option<RateLimitConfig>,
}

/// Proxy configuration — pass-through to reqwest.
#[derive(Debug, Clone)]
pub struct ProxyConfig {
	pub url: String,
}

/// Retry policy for transient errors (429, 502, 503).
#[derive(Debug, Clone)]
pub struct RetryConfig {
	pub max_retries: u32,
	pub base_delay_ms: u64,
	pub max_delay_ms: u64,
}

impl Default for RetryConfig {
	fn default() -> Self {
		Self {
			max_retries: 3,
			base_delay_ms: 1000,
			max_delay_ms: 30000,
		}
	}
}

/// Rate limiting configuration (token bucket).
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
	pub requests_per_minute: u32,
}

/// A parameter value that can be serialized for query strings or form bodies.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum ParamValue {
	String(String),
	Integer(i64),
	Float(f64),
	Bool(bool),
}

/// A value that can be either a string or an integer.
/// Used for user_id which accepts "me" or a numeric ID.
#[derive(Debug, Clone)]
pub enum StringOrInt {
	String(String),
	Int(i64),
}

impl Default for StringOrInt {
	fn default() -> Self {
		StringOrInt::String(String::new())
	}
}

impl std::fmt::Display for StringOrInt {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			StringOrInt::String(s) => f.write_str(s),
			StringOrInt::Int(n) => write!(f, "{n}"),
		}
	}
}

impl serde::Serialize for StringOrInt {
	fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		match self {
			StringOrInt::String(s) => serializer.serialize_str(s),
			StringOrInt::Int(n) => serializer.serialize_i64(*n),
		}
	}
}

impl<'de> serde::Deserialize<'de> for StringOrInt {
	fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		use serde::de;
		struct Visitor;
		impl<'de> de::Visitor<'de> for Visitor {
			type Value = StringOrInt;
			fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
				f.write_str("a string or integer")
			}
			fn visit_str<E: de::Error>(self, v: &str) -> Result<StringOrInt, E> {
				Ok(StringOrInt::String(v.to_string()))
			}
			fn visit_string<E: de::Error>(self, v: String) -> Result<StringOrInt, E> {
				Ok(StringOrInt::String(v))
			}
			fn visit_i64<E: de::Error>(self, v: i64) -> Result<StringOrInt, E> {
				Ok(StringOrInt::Int(v))
			}
			fn visit_u64<E: de::Error>(self, v: u64) -> Result<StringOrInt, E> {
				Ok(StringOrInt::Int(v as i64))
			}
		}
		deserializer.deserialize_any(Visitor)
	}
}

impl From<&str> for StringOrInt {
	fn from(s: &str) -> Self {
		StringOrInt::String(s.to_string())
	}
}
impl From<String> for StringOrInt {
	fn from(s: String) -> Self {
		StringOrInt::String(s)
	}
}
impl From<i64> for StringOrInt {
	fn from(n: i64) -> Self {
		StringOrInt::Int(n)
	}
}

impl std::fmt::Display for ParamValue {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			ParamValue::String(s) => f.write_str(s),
			ParamValue::Integer(n) => write!(f, "{n}"),
			ParamValue::Float(n) => write!(f, "{n}"),
			ParamValue::Bool(b) => write!(f, "{b}"),
		}
	}
}
