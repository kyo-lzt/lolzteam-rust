use std::collections::BTreeMap;

use serde_json::Value;

use crate::transforms::operations::build_method_def;
use crate::transforms::parameters::BodyEncoding;
use crate::transforms::responses::ResponseSchema;
use crate::utils::naming::{group_to_struct_name, split_operation_id};

/// Result of parsing an OpenAPI spec.
#[derive(Debug)]
pub struct ParseResult {
	pub groups: Vec<ParsedGroup>,
	/// Component schemas from `#/components/schemas/*`, keyed by schema name.
	pub component_schemas: BTreeMap<String, Value>,
}

/// A group of related API operations (e.g. all "threads" endpoints).
#[derive(Debug)]
pub struct ParsedGroup {
	/// snake_case group name, e.g. "threads"
	pub name: String,
	/// PascalCase struct name, e.g. "ThreadsApi"
	pub class_name: String,
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
	/// Whether the request body is required (from `requestBody.required`)
	pub body_required: bool,
	/// Whether the body is a raw JSON value (e.g. array-typed body like POST /batch)
	pub is_raw_body: bool,
	/// Body content encoding
	pub body_encoding: BodyEncoding,
	/// Structured response schema for generating typed structs
	pub response_schema: ResponseSchema,
	/// Whether the response is text (not JSON)
	pub response_is_text: bool,
	/// Summary line from the OpenAPI spec
	pub summary: Option<String>,
	/// Full description from the OpenAPI spec
	pub description: Option<String>,
	/// Discriminated union body (oneOf with discriminator), if detected
	pub one_of_body: Option<OneOfBody>,
}

/// Enum values extracted from the OpenAPI schema.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EnumValues {
	Integer(Vec<i64>),
	String(Vec<String>),
}

/// A single variant of a oneOf discriminated union body.
#[derive(Debug, Clone)]
pub struct OneOfVariant {
	/// PascalCase variant name, e.g. "ClientCredentials"
	pub variant_name: String,
	/// Discriminator field value (string or integer literal)
	pub discriminator_value: OneOfDiscriminatorValue,
	/// Parameters for this variant (excluding the discriminator itself)
	pub params: Vec<ParamDef>,
}

/// Discriminator value type for oneOf variants.
#[derive(Debug, Clone)]
pub enum OneOfDiscriminatorValue {
	String(String),
	Integer(i64),
}

/// A oneOf request body with discriminated union structure.
#[derive(Debug, Clone)]
pub struct OneOfBody {
	/// Discriminator field name (e.g. "grant_type", "form_id")
	pub discriminator_field: String,
	/// Variants of the union
	pub variants: Vec<OneOfVariant>,
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
	/// Enum constraint from the schema, if present
	pub enum_values: Option<EnumValues>,
	/// Human-readable description from the OpenAPI spec
	pub description: Option<String>,
	/// Default value from the schema, if present (rendered as doc comment)
	pub default_value: Option<String>,
	/// Raw JSON default value from the schema, for generating Rust default expressions
	pub default_value_raw: Option<serde_json::Value>,
}

/// Parse an OpenAPI spec into grouped operations.
pub fn parse_spec(root: &Value) -> ParseResult {
	// Extract component schemas
	let component_schemas: BTreeMap<String, Value> = root
		.get("components")
		.and_then(|c| c.get("schemas"))
		.and_then(|s| s.as_object())
		.map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
		.unwrap_or_default();

	let paths = match root.get("paths").and_then(|v| v.as_object()) {
		Some(p) => p,
		None => {
			return ParseResult {
				groups: Vec::new(),
				component_schemas,
			}
		}
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
			name,
			methods,
		})
		.collect();

	ParseResult {
		groups,
		component_schemas,
	}
}
