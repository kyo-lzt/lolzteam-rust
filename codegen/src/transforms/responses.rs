use std::collections::HashSet;

use serde_json::Value;

use crate::transforms::types::schema_to_rust_type;
use crate::utils::deref;
use crate::utils::naming::sanitize_ident;

/// A single field in a response struct.
#[derive(Debug, Clone)]
pub struct ResponseFieldDef {
    /// Rust field name (snake_case)
    pub name: String,
    /// Original JSON key
    pub api_name: String,
    /// Rust type string
    pub rust_type: String,
    /// Whether the field is required
    pub required: bool,
}

/// Describes the shape of a response: either a struct with fields, or a plain type alias.
#[derive(Debug, Clone)]
pub enum ResponseSchema {
    /// A struct with named fields (and optional nested struct definitions).
    Struct {
        fields: Vec<ResponseFieldDef>,
        /// Additional struct definitions needed (inner structs for nested objects).
        /// Each entry is (struct_name, fields).
        nested_structs: Vec<(String, Vec<ResponseFieldDef>)>,
    },
    /// A simple type alias (e.g. `serde_json::Value`).
    Alias(String),
}

/// Detect whether the response content type is text (not JSON).
pub fn is_text_response(root: &Value, operation: &Value) -> bool {
    let responses = match operation.get("responses") {
        Some(r) => r,
        None => return false,
    };

    let success_response = responses
        .get("200")
        .or_else(|| responses.get("2XX"))
        .or_else(|| responses.get("201"));

    let success_response = match success_response {
        Some(r) => deref::deref(root, r),
        None => return false,
    };

    let content = match success_response.get("content") {
        Some(c) => c,
        None => return false,
    };

    let content_obj = match content.as_object() {
        Some(o) => o,
        None => return false,
    };

    !content_obj.contains_key("application/json")
        && content_obj.keys().any(|k| k.starts_with("text/"))
}

/// Extract the response schema from an operation's 200 response.
/// Returns a `ResponseSchema` describing how to emit the response type.
pub fn extract_response_schema(root: &Value, operation: &Value, prefix: &str) -> ResponseSchema {
    let responses = match operation.get("responses") {
        Some(r) => r,
        None => return ResponseSchema::Alias("serde_json::Value".to_string()),
    };

    let success_response = responses
        .get("200")
        .or_else(|| responses.get("2XX"))
        .or_else(|| responses.get("201"));

    let success_response = match success_response {
        Some(r) => deref::deref(root, r),
        None => return ResponseSchema::Alias("serde_json::Value".to_string()),
    };

    let schema = success_response
        .get("content")
        .and_then(|c| c.get("application/json"))
        .and_then(|ct| ct.get("schema"));

    let schema = match schema {
        Some(s) => deref::deref(root, s),
        None => return ResponseSchema::Alias("serde_json::Value".to_string()),
    };

    extract_struct_from_schema(root, schema, prefix)
}

/// Try to extract a struct definition from a schema with properties.
/// Falls back to Alias("serde_json::Value") if no properties found.
fn extract_struct_from_schema(root: &Value, schema: &Value, prefix: &str) -> ResponseSchema {
    let properties = match schema.get("properties").and_then(|v| v.as_object()) {
        Some(p) => p,
        None => return ResponseSchema::Alias("serde_json::Value".to_string()),
    };

    if properties.is_empty() {
        return ResponseSchema::Alias("serde_json::Value".to_string());
    }

    let required_fields: Vec<String> = schema
        .get("required")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let mut fields = Vec::new();
    let mut nested_structs: Vec<(String, Vec<ResponseFieldDef>)> = Vec::new();
    let mut seen_fields = HashSet::new();

    for (prop_name, prop_schema) in properties {
        let field_name = sanitize_ident(prop_name);

        // Skip duplicate fields (e.g. camelCase + snake_case both map to same ident)
        if !seen_fields.insert(field_name.clone()) {
            continue;
        }

        let prop_schema = deref::deref(root, prop_schema);
        let required = required_fields.contains(prop_name);

        let rust_type =
            resolve_response_field_type(root, prop_schema, prefix, prop_name, &mut nested_structs);

        fields.push(ResponseFieldDef {
            name: field_name,
            api_name: prop_name.clone(),
            rust_type,
            required,
        });
    }

    ResponseSchema::Struct {
        fields,
        nested_structs,
    }
}

/// Resolve the Rust type for a response field, generating nested structs when needed.
fn resolve_response_field_type(
    root: &Value,
    schema: &Value,
    parent_prefix: &str,
    prop_name: &str,
    nested_structs: &mut Vec<(String, Vec<ResponseFieldDef>)>,
) -> String {
    let schema = deref::deref(root, schema);

    // Check for an object with properties — generate a nested struct
    let has_properties = schema
        .get("properties")
        .and_then(|v| v.as_object())
        .is_some_and(|p| !p.is_empty());
    let is_object_type = schema.get("type").and_then(|v| v.as_str()) == Some("object");

    if has_properties {
        let struct_name = nested_struct_name(parent_prefix, prop_name);
        let inner = extract_struct_from_schema(root, schema, &struct_name);
        match inner {
            ResponseSchema::Struct {
                fields,
                nested_structs: inner_nested,
            } => {
                nested_structs.extend(inner_nested);
                nested_structs.push((struct_name.clone(), fields));
                return struct_name;
            }
            ResponseSchema::Alias(ty) => return ty,
        }
    }

    // Check for array with items that have properties or $ref to object
    if schema.get("type").and_then(|v| v.as_str()) == Some("array") {
        if let Some(items) = schema.get("items") {
            let items = deref::deref(root, items);
            let items_has_props = items
                .get("properties")
                .and_then(|v| v.as_object())
                .is_some_and(|p| !p.is_empty());

            if items_has_props {
                let struct_name = nested_struct_name(parent_prefix, prop_name);
                let inner = extract_struct_from_schema(root, items, &struct_name);
                match inner {
                    ResponseSchema::Struct {
                        fields,
                        nested_structs: inner_nested,
                    } => {
                        nested_structs.extend(inner_nested);
                        nested_structs.push((struct_name.clone(), fields));
                        return format!("Vec<{struct_name}>");
                    }
                    ResponseSchema::Alias(ty) => return format!("Vec<{ty}>"),
                }
            }

            // Simple array items — use schema_to_rust_type
            let (inner_ty, _) = schema_to_rust_type(root, items);
            return format!("Vec<{inner_ty}>");
        }
    }

    // For plain object without properties — use serde_json::Value
    if is_object_type && !has_properties {
        return "serde_json::Value".to_string();
    }

    // Fall back to schema_to_rust_type for primitives
    let (ty, _) = schema_to_rust_type(root, schema);
    ty
}

/// Generate a PascalCase struct name for a nested response field.
/// e.g. parent_prefix="ThreadsList", prop_name="system_info" -> "ThreadsListSystemInfo"
fn nested_struct_name(parent_prefix: &str, prop_name: &str) -> String {
    use heck::ToUpperCamelCase;
    format!("{parent_prefix}{}", prop_name.to_upper_camel_case())
}
