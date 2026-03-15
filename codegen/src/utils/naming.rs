use heck::{ToSnakeCase, ToUpperCamelCase};

/// Convert an operationId like `Threads.List` or `Users.SA.Reset` into
/// a (group_name, method_name) tuple.
///
/// - Group = first segment, lower snake_case
/// - Method = remaining segments joined with `_`, lower snake_case
pub fn split_operation_id(operation_id: &str) -> (String, String) {
	let parts: Vec<&str> = operation_id.split('.').collect();
	let group = parts[0].to_snake_case();
	let method = if parts.len() > 1 {
		let raw = parts[1..]
			.iter()
			.map(|s| s.to_snake_case())
			.collect::<Vec<_>>()
			.join("_");
		sanitize_method_name(&raw)
	} else {
		parts[0].to_snake_case()
	};
	(group, method)
}

/// Group name to PascalCase API struct name: `threads` -> `ThreadsApi`
pub fn group_to_struct_name(group: &str) -> String {
	format!("{}Api", group.to_upper_camel_case())
}

/// Sanitize a Rust identifier — avoid Rust keywords and invalid starts.
pub fn sanitize_ident(name: &str) -> String {
	let snake = name.to_snake_case();
	// Identifiers can't start with a digit
	let snake = if snake.starts_with(|c: char| c.is_ascii_digit()) {
		format!("_{snake}")
	} else {
		snake
	};
	match snake.as_str() {
		"type" => "r#type".to_string(),
		"self" => "self_".to_string(),
		"match" => "r#match".to_string(),
		"ref" => "r#ref".to_string(),
		"mod" => "r#mod".to_string(),
		"move" => "r#move".to_string(),
		"fn" => "fn_".to_string(),
		"use" => "r#use".to_string(),
		"pub" => "pub_".to_string(),
		"in" => "r#in".to_string(),
		"let" => "let_".to_string(),
		"for" => "r#for".to_string(),
		"loop" => "r#loop".to_string(),
		"where" => "r#where".to_string(),
		"while" => "r#while".to_string(),
		"if" => "if_".to_string(),
		"else" => "else_".to_string(),
		"return" => "return_".to_string(),
		"async" => "async_".to_string(),
		"await" => "await_".to_string(),
		"yield" => "yield_".to_string(),
		"box" => "box_".to_string(),
		"try" => "r#try".to_string(),
		"show" => "show".to_string(),
		_ => snake,
	}
}

/// Sanitize a method name — Rust keywords get a trailing underscore.
/// We avoid `r#` for method names as it looks ugly in the public API.
pub fn sanitize_method_name(name: &str) -> String {
	match name {
		"type" | "move" | "match" | "ref" | "mod" | "use" | "in" | "for" | "loop" | "while"
		| "if" | "else" | "return" | "async" | "await" | "yield" | "fn" | "pub" | "let"
		| "self" | "super" | "crate" | "where" | "box" | "break" | "continue" | "extern"
		| "impl" | "struct" | "enum" | "trait" | "const" | "static" | "unsafe" | "mut" | "as"
		| "dyn" | "true" | "false" => format!("{name}_"),
		_ => name.to_string(),
	}
}

/// Convert a param name to snake_case, preserving brackets for array params.
/// e.g. `tag_id[]` stays as `tag_id` (strips brackets for Rust field name).
pub fn param_name_to_field(name: &str) -> String {
	let clean = name.replace("[]", "");
	sanitize_ident(&clean)
}

/// The original API param name, for use in serialization / query building.
/// We keep it as-is from the schema (including `[]` suffix if present).
#[allow(dead_code)]
pub fn param_api_name(name: &str) -> String {
	name.to_string()
}

/// Convert a group + method name to a struct prefix in PascalCase.
/// e.g. group="threads", method="list" -> "ThreadsList"
pub fn method_type_prefix(group: &str, method: &str) -> String {
	format!(
		"{}{}",
		group.to_upper_camel_case(),
		method.to_upper_camel_case()
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_split_operation_id() {
		assert_eq!(
			split_operation_id("Threads.List"),
			("threads".into(), "list".into())
		);
		assert_eq!(
			split_operation_id("Users.SA.Reset"),
			("users".into(), "sa_reset".into())
		);
		assert_eq!(
			split_operation_id("Category.All"),
			("category".into(), "all".into())
		);
		assert_eq!(
			split_operation_id("Batch.Execute"),
			("batch".into(), "execute".into())
		);
	}

	#[test]
	fn test_group_to_struct_name() {
		assert_eq!(group_to_struct_name("threads"), "ThreadsApi");
		assert_eq!(group_to_struct_name("profile_posts"), "ProfilePostsApi");
	}
}
