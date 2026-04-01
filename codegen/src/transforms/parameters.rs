use serde_json::Value;

use heck::ToUpperCamelCase;

use crate::parser::{EnumValues, OneOfBody, OneOfDiscriminatorValue, OneOfVariant, ParamDef};
use crate::transforms::types::schema_to_rust_type;
use crate::utils::deref;
use crate::utils::naming::param_name_to_field;

/// Extract a default value from the schema as a display string.
fn extract_default_value(schema: &Value) -> Option<String> {
	let val = schema.get("default")?;
	match val {
		Value::String(s) => Some(format!("\"{s}\"")),
		Value::Number(n) => Some(n.to_string()),
		Value::Bool(b) => Some(b.to_string()),
		_ => None,
	}
}

/// Extract the raw JSON default value from the schema.
fn extract_default_value_raw(schema: &Value) -> Option<Value> {
	schema.get("default").cloned()
}

/// Extract enum values from a resolved schema, if present.
fn extract_enum_values(schema: &Value) -> Option<EnumValues> {
	let arr = schema.get("enum")?.as_array()?;
	let type_str = schema.get("type").and_then(|v| v.as_str()).unwrap_or("");
	match type_str {
		"integer" => {
			let vals: Vec<i64> = arr.iter().filter_map(|v| v.as_i64()).collect();
			if vals.is_empty() {
				None
			} else {
				Some(EnumValues::Integer(vals))
			}
		}
		"string" => {
			let vals: Vec<String> = arr
				.iter()
				.filter_map(|v| v.as_str().map(String::from))
				.collect();
			if vals.is_empty() {
				None
			} else {
				Some(EnumValues::String(vals))
			}
		}
		_ => None,
	}
}

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

		let enum_values = extract_enum_values(schema);
		let description = param
			.get("description")
			.and_then(|v| v.as_str())
			.map(String::from);
		let default_value = extract_default_value(schema);
		let default_value_raw = extract_default_value_raw(schema);

		result.push(ParamDef {
			name: param_name_to_field(api_name),
			api_name: api_name.to_string(),
			rust_type,
			required,
			param_value_variant,
			is_binary: false,
			is_deep_object,
			enum_values,
			description,
			default_value,
			default_value_raw,
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
	/// Discriminated union body (oneOf with detectable discriminator), if present.
	pub one_of_body: Option<OneOfBody>,
	/// Whether the request body is required (from `requestBody.required`).
	pub body_required: bool,
}

/// Extract body parameters from the request body schema.
pub fn extract_body_params(root: &Value, operation: &Value) -> BodyParamsResult {
	let default_result = BodyParamsResult {
		params: Vec::new(),
		encoding: BodyEncoding::FormUrlEncoded,
		is_raw_body: false,
		one_of_body: None,
		body_required: false,
	};

	let request_body = match operation.get("requestBody") {
		Some(rb) => deref::deref(root, rb),
		None => return default_result,
	};

	let body_required = request_body
		.get("required")
		.and_then(|v| v.as_bool())
		.unwrap_or(false);

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
				one_of_body: None,
				body_required,
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
			one_of_body: None,
			body_required,
		};
	}

	// Handle oneOf in body schema (e.g. OAuth)
	if schema.get("oneOf").is_some() {
		let one_of_body = try_extract_discriminated_one_of(root, schema);
		// Flatten all properties from all oneOf variants (for backward compat and audit)
		return BodyParamsResult {
			params: extract_one_of_params(root, schema),
			encoding,
			is_raw_body: false,
			one_of_body,
			body_required,
		};
	}

	BodyParamsResult {
		params: extract_properties_as_params(root, schema),
		encoding,
		is_raw_body: false,
		one_of_body: None,
		body_required,
	}
}

fn extract_one_of_params(root: &Value, schema: &Value) -> Vec<ParamDef> {
	let variants = match schema.get("oneOf").and_then(|v| v.as_array()) {
		Some(v) => v,
		None => return Vec::new(),
	};

	let mut seen = std::collections::BTreeSet::new();
	let mut result = Vec::new();

	// Collect required sets from each variant for intersection logic
	let variant_required_sets: Vec<std::collections::BTreeSet<String>> = variants
		.iter()
		.map(|v| {
			let v = deref::deref(root, v);
			v.get("required")
				.and_then(|r| r.as_array())
				.map(|arr| {
					arr.iter()
						.filter_map(|s| s.as_str().map(String::from))
						.collect()
				})
				.unwrap_or_default()
		})
		.collect();

	for variant in variants {
		let variant = deref::deref(root, variant);
		for param in extract_properties_as_params(root, variant) {
			if seen.insert(param.name.clone()) {
				// required = true only if required in ALL variants
				let all_required = variant_required_sets
					.iter()
					.all(|rs| rs.contains(&param.api_name));
				result.push(ParamDef {
					required: all_required,
					is_binary: false,
					enum_values: param.enum_values.clone(),
					default_value: param.default_value.clone(),
					default_value_raw: param.default_value_raw.clone(),
					..param
				});
			}
		}
	}

	result
}

/// Try to detect a discriminated union in a oneOf schema.
///
/// Looks for a property present in all variants whose `enum` has exactly one value
/// (the discriminator). Returns `None` if no discriminator is found.
fn try_extract_discriminated_one_of(root: &Value, schema: &Value) -> Option<OneOfBody> {
	let variants = schema.get("oneOf")?.as_array()?;
	if variants.len() < 2 {
		return None;
	}

	// Find candidate discriminator: a property that exists in every variant
	// with exactly one enum value
	let resolved_variants: Vec<&Value> = variants.iter().map(|v| deref::deref(root, v)).collect();

	// Get property names from first variant
	let first_props = resolved_variants[0].get("properties")?.as_object()?;

	let mut discriminator_field: Option<String> = None;
	for prop_name in first_props.keys() {
		let is_discriminator = resolved_variants.iter().all(|v| {
			v.get("properties")
				.and_then(|p| p.get(prop_name))
				.and_then(|s| {
					let s = deref::deref(root, s);
					s.get("enum")?.as_array().filter(|arr| arr.len() == 1)
				})
				.is_some()
		});
		if is_discriminator {
			discriminator_field = Some(prop_name.clone());
			break;
		}
	}

	let discriminator_field = discriminator_field?;

	// Build variants
	let mut result_variants = Vec::new();
	for variant_schema in &resolved_variants {
		let props = variant_schema.get("properties")?.as_object()?;
		let disc_prop = props.get(&discriminator_field)?;
		let disc_prop = deref::deref(root, disc_prop);
		let disc_enum = disc_prop.get("enum")?.as_array()?;
		let disc_value = disc_enum.first()?;

		let discriminator_value = if let Some(s) = disc_value.as_str() {
			OneOfDiscriminatorValue::String(s.to_string())
		} else if let Some(n) = disc_value.as_i64() {
			OneOfDiscriminatorValue::Integer(n)
		} else {
			return None;
		};

		// Derive variant name from title or discriminator value
		let variant_name = variant_schema
			.get("title")
			.and_then(|t| t.as_str())
			.map(|t| t.to_upper_camel_case())
			.unwrap_or_else(|| match &discriminator_value {
				OneOfDiscriminatorValue::String(s) => s.to_upper_camel_case(),
				OneOfDiscriminatorValue::Integer(n) => format!("V{n}"),
			});

		// Extract params for this variant, excluding the discriminator field
		let params: Vec<ParamDef> = extract_properties_as_params(root, variant_schema)
			.into_iter()
			.filter(|p| p.api_name != discriminator_field)
			.collect();

		result_variants.push(OneOfVariant {
			variant_name,
			discriminator_value,
			params,
		});
	}

	Some(OneOfBody {
		discriminator_field,
		variants: result_variants,
	})
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
		let enum_values = extract_enum_values(prop_schema);
		let description = prop_schema
			.get("description")
			.and_then(|v| v.as_str())
			.map(String::from);
		let default_value = extract_default_value(prop_schema);
		let default_value_raw = extract_default_value_raw(prop_schema);

		result.push(ParamDef {
			name: param_name_to_field(prop_name),
			api_name: prop_name.clone(),
			rust_type,
			required,
			param_value_variant,
			is_binary,
			is_deep_object: false,
			enum_values,
			description,
			default_value,
			default_value_raw,
		});
	}

	result
}
