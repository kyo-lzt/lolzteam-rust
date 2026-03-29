use serde_json::Value;

use crate::transforms::responses::component_ref_type;
use crate::utils::deref;

/// Map an OpenAPI schema to a Rust type string.
///
/// Returns `(rust_type, param_value_variant)` where `param_value_variant`
/// is the `ParamValue` enum variant name for query/body building.
pub fn schema_to_rust_type(root: &Value, schema: &Value) -> (String, String) {
	// Check for $ref to a component schema before dereffing (deref loses the ref info)
	if let Some(component_type) = component_ref_type(schema) {
		return (component_type, "String".to_string());
	}

	let schema = deref::deref(root, schema);

	// Handle nullable wrapper
	if let Some(any_of) = schema.get("anyOf").and_then(|v| v.as_array()) {
		// anyOf with null = nullable type
		let non_null: Vec<&Value> = any_of
			.iter()
			.filter(|v| v.get("type").and_then(|t| t.as_str()) != Some("null"))
			.collect();
		if non_null.len() == 1 {
			if let Some(inner) = non_null.first() {
				let (ty, variant) = schema_to_rust_type(root, inner);
				return (format!("Option<{ty}>"), variant);
			}
		}
		return ("serde_json::Value".to_string(), "String".to_string());
	}

	if schema.get("oneOf").is_some() {
		return ("serde_json::Value".to_string(), "String".to_string());
	}

	// Handle allOf — merge all schemas' properties
	if let Some(all_of) = schema.get("allOf").and_then(|v| v.as_array()) {
		if all_of.len() == 1 {
			if let Some(inner) = all_of.first() {
				return schema_to_rust_type(root, inner);
			}
		}
		// Merge properties from all schemas
		let mut merged = serde_json::Map::new();
		let mut merged_required = Vec::new();
		let mut has_props = false;
		for item in all_of {
			if let Some(props) = item.get("properties").and_then(|v| v.as_object()) {
				for (k, v) in props {
					merged.insert(k.clone(), v.clone());
				}
				has_props = true;
			}
			if let Some(req) = item.get("required").and_then(|v| v.as_array()) {
				for r in req {
					if let Some(s) = r.as_str() {
						merged_required.push(Value::String(s.to_string()));
					}
				}
			}
		}
		if has_props {
			let mut merged_schema = serde_json::Map::new();
			merged_schema.insert("type".to_string(), Value::String("object".to_string()));
			merged_schema.insert("properties".to_string(), Value::Object(merged));
			if !merged_required.is_empty() {
				merged_schema.insert("required".to_string(), Value::Array(merged_required));
			}
			return schema_to_rust_type(root, &Value::Object(merged_schema));
		}
		return ("serde_json::Value".to_string(), "String".to_string());
	}

	// Type can be a string or an array of strings (OpenAPI 3.1)
	let type_val = schema.get("type");

	let type_str = match type_val {
		Some(Value::String(s)) => Some(s.as_str()),
		Some(Value::Array(arr)) => {
			// e.g. ["string", "integer"] or ["string", "null"]
			let non_null: Vec<&str> = arr
				.iter()
				.filter_map(|v| v.as_str())
				.filter(|s| *s != "null")
				.collect();
			let is_nullable = arr.iter().any(|v| v.as_str() == Some("null"));
			if non_null.len() == 1 {
				let inner_type = non_null[0];
				let (ty, variant) = primitive_type(inner_type, schema);
				if is_nullable {
					return (format!("Option<{ty}>"), variant);
				}
				return (ty, variant);
			}
			// ["string", "integer"] or ["integer", "string"] → StringOrInt
			if non_null.len() == 2 {
				let mut sorted = non_null.clone();
				sorted.sort();
				if sorted == ["integer", "string"] {
					let ty = if is_nullable {
						"Option<StringOrInt>".to_string()
					} else {
						"StringOrInt".to_string()
					};
					return (ty, "StringOrInt".to_string());
				}
			}
			// Other multiple non-null types — fall back to Value
			if is_nullable {
				return (
					"Option<serde_json::Value>".to_string(),
					"String".to_string(),
				);
			}
			return ("serde_json::Value".to_string(), "String".to_string());
		}
		_ => None,
	};

	match type_str {
		Some("string") => {
			let format = schema.get("format").and_then(|v| v.as_str()).unwrap_or("");
			if format == "binary" {
				("Vec<u8>".to_string(), "String".to_string())
			} else if schema.get("enum").is_some() {
				// String enums — just use String for simplicity
				("String".to_string(), "String".to_string())
			} else {
				("String".to_string(), "String".to_string())
			}
		}
		Some("integer") | Some("int32") | Some("int64") => {
			("i64".to_string(), "Integer".to_string())
		}
		Some("number") | Some("float") | Some("double") => ("f64".to_string(), "Float".to_string()),
		Some("boolean") => ("bool".to_string(), "Bool".to_string()),
		Some("array") => {
			let empty_obj = Value::Object(serde_json::Map::new());
			let items = schema.get("items").unwrap_or(&empty_obj);
			let (inner, _) = schema_to_rust_type(root, items);
			(format!("Vec<{inner}>"), "String".to_string())
		}
		Some("object") => {
			if let Some(ap) = schema.get("additionalProperties") {
				let ap = deref::deref(root, ap);
				let (inner, _) = schema_to_rust_type(root, ap);
				(format!("HashMap<String, {inner}>"), "String".to_string())
			} else {
				// object without specific properties — use Value
				("serde_json::Value".to_string(), "String".to_string())
			}
		}
		Some("null") => (
			"Option<serde_json::Value>".to_string(),
			"String".to_string(),
		),
		_ => {
			// No type specified — treat as opaque JSON value
			("serde_json::Value".to_string(), "String".to_string())
		}
	}
}

fn primitive_type(type_str: &str, schema: &Value) -> (String, String) {
	match type_str {
		"string" => {
			let format = schema.get("format").and_then(|v| v.as_str()).unwrap_or("");
			match format {
				"binary" => ("Vec<u8>".to_string(), "String".to_string()),
				"int64" | "int32" => ("i64".to_string(), "Integer".to_string()),
				"float" | "double" => ("f64".to_string(), "Float".to_string()),
				_ => ("String".to_string(), "String".to_string()),
			}
		}
		"integer" | "int32" | "int64" => ("i64".to_string(), "Integer".to_string()),
		"number" | "float" | "double" => ("f64".to_string(), "Float".to_string()),
		"boolean" => ("bool".to_string(), "Bool".to_string()),
		"array" => ("Vec<serde_json::Value>".to_string(), "String".to_string()),
		"object" => ("serde_json::Value".to_string(), "String".to_string()),
		_ => ("serde_json::Value".to_string(), "String".to_string()),
	}
}

/// Determine the ParamValue variant for a Rust type string.
#[allow(dead_code)]
pub fn rust_type_to_param_variant(rust_type: &str) -> &'static str {
	let base = rust_type
		.strip_prefix("Option<")
		.and_then(|s| s.strip_suffix('>'))
		.unwrap_or(rust_type);

	match base {
		"i64" => "Integer",
		"f64" => "Float",
		"bool" => "Bool",
		"String" => "String",
		_ => "String",
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_simple_types() {
		let root = serde_json::json!({});
		let schema = serde_json::json!({"type": "string"});
		assert_eq!(
			schema_to_rust_type(&root, &schema),
			("String".into(), "String".into())
		);

		let schema = serde_json::json!({"type": "integer"});
		assert_eq!(
			schema_to_rust_type(&root, &schema),
			("i64".into(), "Integer".into())
		);

		let schema = serde_json::json!({"type": "boolean"});
		assert_eq!(
			schema_to_rust_type(&root, &schema),
			("bool".into(), "Bool".into())
		);
	}

	#[test]
	fn test_nullable_type() {
		let root = serde_json::json!({});
		let schema = serde_json::json!({"type": ["string", "null"]});
		let (ty, _) = schema_to_rust_type(&root, &schema);
		assert_eq!(ty, "Option<String>");
	}

	#[test]
	fn test_array_type() {
		let root = serde_json::json!({});
		let schema = serde_json::json!({"type": "array", "items": {"type": "integer"}});
		let (ty, _) = schema_to_rust_type(&root, &schema);
		assert_eq!(ty, "Vec<i64>");
	}

	#[test]
	fn test_string_or_integer_union() {
		let root = serde_json::json!({});
		let schema = serde_json::json!({"type": ["string", "integer"]});
		let (ty, variant) = schema_to_rust_type(&root, &schema);
		assert_eq!(ty, "StringOrInt");
		assert_eq!(variant, "StringOrInt");
	}

	#[test]
	fn test_string_or_integer_reversed() {
		let root = serde_json::json!({});
		let schema = serde_json::json!({"type": ["integer", "string"]});
		let (ty, variant) = schema_to_rust_type(&root, &schema);
		assert_eq!(ty, "StringOrInt");
		assert_eq!(variant, "StringOrInt");
	}

	#[test]
	fn test_nullable_integer() {
		let root = serde_json::json!({});
		let schema = serde_json::json!({"type": ["integer", "null"]});
		let (ty, _) = schema_to_rust_type(&root, &schema);
		assert_eq!(ty, "Option<i64>");
	}
}
