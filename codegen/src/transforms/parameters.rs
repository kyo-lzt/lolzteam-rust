use serde_json::Value;

use crate::parser::ParamDef;
use crate::transforms::types::schema_to_rust_type;
use crate::utils::deref;
use crate::utils::naming::param_name_to_field;

/// Extract path parameters from an operation's `parameters` array.
pub fn extract_path_params(root: &Value, parameters: &[Value]) -> Vec<ParamDef> {
	extract_params_by_location(root, parameters, "path")
}

/// Extract query parameters from an operation's `parameters` array.
pub fn extract_query_params(root: &Value, parameters: &[Value]) -> Vec<ParamDef> {
	extract_params_by_location(root, parameters, "query")
}

fn extract_params_by_location(root: &Value, parameters: &[Value], location: &str) -> Vec<ParamDef> {
	let mut result = Vec::new();

	for param_value in parameters {
		let param = deref::deref(root, param_value);

		let in_field = param.get("in").and_then(|v| v.as_str()).unwrap_or("");
		if in_field != location {
			continue;
		}

		let api_name = param
			.get("name")
			.and_then(|v| v.as_str())
			.unwrap_or("unknown");

		let required = param
			.get("required")
			.and_then(|v| v.as_bool())
			.unwrap_or(false);

		let is_deep_object = param.get("style").and_then(|v| v.as_str()) == Some("deepObject");

		let empty_obj = Value::Object(serde_json::Map::new());
		let schema = param.get("schema").unwrap_or(&empty_obj);
		let schema = deref::deref(root, schema);

		let (mut rust_type, param_value_variant) = if is_deep_object {
			// deepObject with additionalProperties → HashMap<String, T>
			deep_object_type(root, schema)
		} else {
			schema_to_rust_type(root, schema)
		};

		// If the API name ends with `[]` but the schema type isn't already Vec,
		// force it to Vec<T> (some schemas incorrectly use type: "string" for array params)
		let has_bracket_suffix = api_name.ends_with("[]");
		if has_bracket_suffix && !rust_type.starts_with("Vec<") {
			rust_type = format!("Vec<{rust_type}>");
			// param_value_variant stays the same — it's the variant for each element
		}

		result.push(ParamDef {
			name: param_name_to_field(api_name),
			api_name: api_name.to_string(),
			rust_type,
			required,
			param_value_variant,
			is_binary: false,
			is_deep_object,
		});
	}

	result
}

/// Map a deepObject schema to `HashMap<String, T>` based on `additionalProperties`.
fn deep_object_type(root: &Value, schema: &Value) -> (String, String) {
	let additional = schema.get("additionalProperties");
	match additional {
		Some(ap) => {
			let ap = deref::deref(root, ap);
			let (inner_type, variant) = schema_to_rust_type(root, ap);
			(format!("HashMap<String, {inner_type}>"), variant)
		}
		None => (
			"HashMap<String, serde_json::Value>".to_string(),
			"String".to_string(),
		),
	}
}

/// Body content encoding detected from the OpenAPI spec.
#[derive(Debug, Clone, PartialEq)]
pub enum BodyEncoding {
	FormUrlEncoded,
	Json,
	Multipart,
}

/// Result of extracting body parameters, including content type info.
pub struct BodyParamsResult {
	pub params: Vec<ParamDef>,
	pub encoding: BodyEncoding,
	/// True when the body schema is an array (e.g. POST /batch) — needs raw JSON body.
	pub is_raw_body: bool,
}

/// Extract body parameters from the request body schema.
pub fn extract_body_params(root: &Value, operation: &Value) -> BodyParamsResult {
	let default_result = BodyParamsResult {
		params: Vec::new(),
		encoding: BodyEncoding::FormUrlEncoded,
		is_raw_body: false,
	};

	let request_body = match operation.get("requestBody") {
		Some(rb) => deref::deref(root, rb),
		None => return default_result,
	};

	let content = match request_body.get("content") {
		Some(c) => c,
		None => return default_result,
	};

	let encoding = if content.get("multipart/form-data").is_some()
		&& content.get("application/x-www-form-urlencoded").is_none()
	{
		BodyEncoding::Multipart
	} else if content.get("application/json").is_some()
		&& content.get("application/x-www-form-urlencoded").is_none()
	{
		BodyEncoding::Json
	} else {
		BodyEncoding::FormUrlEncoded
	};

	// Try common content types
	let schema = content
		.get("application/x-www-form-urlencoded")
		.or_else(|| content.get("application/json"))
		.or_else(|| content.get("multipart/form-data"))
		.and_then(|ct| ct.get("schema"));

	let schema = match schema {
		Some(s) => deref::deref(root, s),
		None => {
			return BodyParamsResult {
				params: Vec::new(),
				encoding,
				is_raw_body: false,
			}
		}
	};

	// Handle array body schema (e.g. POST /batch) — raw JSON body
	if schema.get("type").and_then(|v| v.as_str()) == Some("array")
		&& schema.get("properties").is_none()
	{
		return BodyParamsResult {
			params: Vec::new(),
			encoding: BodyEncoding::Json,
			is_raw_body: true,
		};
	}

	// Handle oneOf in body schema (e.g. OAuth)
	if schema.get("oneOf").is_some() {
		// Flatten all properties from all oneOf variants
		return BodyParamsResult {
			params: extract_one_of_params(root, schema),
			encoding,
			is_raw_body: false,
		};
	}

	BodyParamsResult {
		params: extract_properties_as_params(root, schema),
		encoding,
		is_raw_body: false,
	}
}

fn extract_one_of_params(root: &Value, schema: &Value) -> Vec<ParamDef> {
	let variants = match schema.get("oneOf").and_then(|v| v.as_array()) {
		Some(v) => v,
		None => return Vec::new(),
	};

	let mut seen = std::collections::HashSet::new();
	let mut result = Vec::new();

	for variant in variants {
		let variant = deref::deref(root, variant);
		for param in extract_properties_as_params(root, variant) {
			if seen.insert(param.name.clone()) {
				// Mark all as optional since they come from different variants
				result.push(ParamDef {
					required: false,
					is_binary: false,
					..param
				});
			}
		}
	}

	result
}

fn extract_properties_as_params(root: &Value, schema: &Value) -> Vec<ParamDef> {
	let properties = match schema.get("properties").and_then(|v| v.as_object()) {
		Some(p) => p,
		None => return Vec::new(),
	};

	let required_fields: Vec<String> = schema
		.get("required")
		.and_then(|v| v.as_array())
		.map(|arr| {
			arr.iter()
				.filter_map(|v| v.as_str().map(String::from))
				.collect()
		})
		.unwrap_or_default();

	let mut result = Vec::new();

	for (prop_name, prop_schema) in properties {
		let prop_schema = deref::deref(root, prop_schema);
		let is_binary = prop_schema.get("format").and_then(|v| v.as_str()) == Some("binary");
		let (rust_type, param_value_variant) = schema_to_rust_type(root, prop_schema);
		let required = required_fields.contains(prop_name);

		result.push(ParamDef {
			name: param_name_to_field(prop_name),
			api_name: prop_name.clone(),
			rust_type,
			required,
			param_value_variant,
			is_binary,
			is_deep_object: false,
		});
	}

	result
}
