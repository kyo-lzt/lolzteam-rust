use std::collections::BTreeMap;

use serde_json::Value;

use crate::transforms::operations::build_method_def;
use crate::transforms::parameters::BodyEncoding;
use crate::transforms::responses::ResponseSchema;
use crate::utils::naming::{group_to_file_name, group_to_struct_name, split_operation_id};

/// Result of parsing an OpenAPI spec.
#[derive(Debug)]
pub struct ParseResult {
    pub groups: Vec<ParsedGroup>,
}

/// A group of related API operations (e.g. all "threads" endpoints).
#[derive(Debug)]
pub struct ParsedGroup {
    /// snake_case group name, e.g. "threads"
    pub name: String,
    /// PascalCase struct name, e.g. "ThreadsApi"
    pub class_name: String,
    /// File name for the generated module, e.g. "threads"
    pub file_name: String,
    /// Methods in this group
    pub methods: Vec<MethodDef>,
}

/// A single API method definition.
#[derive(Debug, Clone)]
pub struct MethodDef {
    /// snake_case method name, e.g. "list", "sa_reset"
    pub name: String,
    /// HTTP method: "GET", "POST", etc.
    pub http_method: String,
    /// URL path template, e.g. "/threads/{thread_id}"
    pub path: String,
    /// Path parameters (extracted from URL template)
    pub path_params: Vec<ParamDef>,
    /// Query parameters
    pub query_params: Vec<ParamDef>,
    /// Request body parameters
    pub body_params: Vec<ParamDef>,
    /// Whether this method has a request body
    pub has_body: bool,
    /// Whether the body is a raw JSON value (e.g. array-typed body like POST /batch)
    pub is_raw_body: bool,
    /// Body content encoding
    pub body_encoding: BodyEncoding,
    /// Structured response schema for generating typed structs
    pub response_schema: ResponseSchema,
    /// Whether the response is text (not JSON)
    pub response_is_text: bool,
    /// Summary/description from the OpenAPI spec
    pub description: Option<String>,
}

/// A single parameter definition.
#[derive(Debug, Clone)]
pub struct ParamDef {
    /// Rust field name (snake_case, sanitized)
    pub name: String,
    /// Original API parameter name (may include `[]` suffix)
    pub api_name: String,
    /// Rust type string, e.g. "String", "i64", "Option<String>"
    pub rust_type: String,
    /// Whether this parameter is required
    pub required: bool,
    /// ParamValue variant name: "String", "Integer", "Float", "Bool"
    pub param_value_variant: String,
    /// Whether this parameter is a binary file upload (format: "binary")
    pub is_binary: bool,
    /// Whether this parameter uses deepObject serialization style
    pub is_deep_object: bool,
}

/// Parse an OpenAPI spec into grouped operations.
pub fn parse_spec(root: &Value) -> ParseResult {
    let paths = match root.get("paths").and_then(|v| v.as_object()) {
        Some(p) => p,
        None => return ParseResult { groups: Vec::new() },
    };

    // Collect all operations grouped by their prefix
    let mut group_methods: BTreeMap<String, Vec<MethodDef>> = BTreeMap::new();

    for (path, path_item) in paths {
        let path_item = match path_item.as_object() {
            Some(obj) => obj,
            None => continue,
        };

        for (http_method, operation) in path_item {
            // Skip non-HTTP-method keys (e.g. "parameters", "summary")
            if !matches!(
                http_method.as_str(),
                "get" | "post" | "put" | "delete" | "patch" | "head" | "options"
            ) {
                continue;
            }

            let operation_id = match operation.get("operationId").and_then(|v| v.as_str()) {
                Some(id) => id,
                None => continue,
            };

            let (group_name, method_name) = split_operation_id(operation_id);
            // Normalize known typos in group names
            let group_name = if group_name == "manging" {
                "managing".to_string()
            } else {
                group_name
            };

            let method_def = build_method_def(
                root,
                &method_name,
                http_method,
                path,
                operation,
                &group_name,
            );

            group_methods
                .entry(group_name)
                .or_default()
                .push(method_def);
        }
    }

    let groups = group_methods
        .into_iter()
        .map(|(name, methods)| ParsedGroup {
            class_name: group_to_struct_name(&name),
            file_name: group_to_file_name(&name),
            name,
            methods,
        })
        .collect();

    ParseResult { groups }
}
