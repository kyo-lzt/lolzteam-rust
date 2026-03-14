use serde_json::Value;

/// Resolve a JSON `$ref` pointer like `#/components/parameters/page`
/// against the root OpenAPI document.
pub fn resolve_ref<'a>(root: &'a Value, ref_path: &str) -> &'a Value {
	let path = ref_path.strip_prefix("#/").unwrap_or(ref_path);
	let mut current = root;
	for segment in path.split('/') {
		current = match current {
			Value::Object(map) => map.get(segment).unwrap_or(&Value::Null),
			Value::Array(arr) => {
				if let Ok(idx) = segment.parse::<usize>() {
					arr.get(idx).unwrap_or(&Value::Null)
				} else {
					&Value::Null
				}
			}
			_ => &Value::Null,
		};
	}
	current
}

/// If the value has a `$ref` key, resolve it. Otherwise return the value as-is.
/// Recursively follows chains of `$ref`s.
pub fn deref<'a>(root: &'a Value, value: &'a Value) -> &'a Value {
	let mut current = value;
	for _ in 0..20 {
		if let Some(ref_path) = current.get("$ref").and_then(|v| v.as_str()) {
			current = resolve_ref(root, ref_path);
		} else {
			return current;
		}
	}
	current
}

/// Deep-resolve all `$ref` values in a JSON value, returning a new owned value
/// with all refs inlined.
#[allow(dead_code)]
pub fn deep_deref(root: &Value, value: &Value) -> Value {
	match value {
		Value::Object(map) => {
			if let Some(ref_path) = map.get("$ref").and_then(|v| v.as_str()) {
				let resolved = resolve_ref(root, ref_path);
				return deep_deref(root, resolved);
			}
			let mut new_map = serde_json::Map::new();
			for (k, v) in map {
				new_map.insert(k.clone(), deep_deref(root, v));
			}
			Value::Object(new_map)
		}
		Value::Array(arr) => Value::Array(arr.iter().map(|v| deep_deref(root, v)).collect()),
		other => other.clone(),
	}
}
