use serde_json::Value;

use crate::parser::MethodDef;
use crate::transforms::parameters::{
	extract_body_params, extract_path_params, extract_query_params, BodyEncoding,
};
use crate::transforms::responses;
use crate::utils::deref;
use crate::utils::naming::method_type_prefix;

/// Extract a `MethodDef` from a single OpenAPI operation.
pub fn build_method_def(
	root: &Value,
	method_name: &str,
	http_method: &str,
	path: &str,
	operation: &Value,
	group_name: &str,
) -> MethodDef {
	let operation = deref::deref(root, operation);

	let parameters: Vec<Value> = operation
		.get("parameters")
		.and_then(|v| v.as_array())
		.cloned()
		.unwrap_or_default();

	let path_params = extract_path_params(root, &parameters);
	let query_params = extract_query_params(root, &parameters);
	let body_result = extract_body_params(root, operation);

	let description = operation
		.get("summary")
		.and_then(|v| v.as_str())
		.map(String::from);

	let response_is_text = responses::is_text_response(root, operation);

	let prefix = method_type_prefix(group_name, method_name);
	let response_schema = responses::extract_response_schema(root, operation, &prefix);

	// GET requests must not have a body — move body params to query params instead
	let is_get = http_method.eq_ignore_ascii_case("get");
	let (final_query_params, final_body_params, final_has_body, final_is_raw_body, final_encoding) =
		if is_get && !body_result.params.is_empty() {
			let mut merged_query = query_params.clone();
			merged_query.extend(body_result.params.iter().cloned());
			(
				merged_query,
				Vec::new(),
				false,
				false,
				BodyEncoding::FormUrlEncoded,
			)
		} else {
			(
				query_params.clone(),
				body_result.params.clone(),
				!body_result.params.is_empty() || body_result.is_raw_body,
				body_result.is_raw_body,
				body_result.encoding,
			)
		};

	MethodDef {
		name: method_name.to_string(),
		http_method: http_method.to_uppercase(),
		path: path.to_string(),
		path_params,
		query_params: final_query_params,
		body_params: final_body_params,
		has_body: final_has_body,
		is_raw_body: final_is_raw_body,
		body_encoding: final_encoding,
		response_schema,
		response_is_text,
		description,
		one_of_body: body_result.one_of_body,
	}
}
