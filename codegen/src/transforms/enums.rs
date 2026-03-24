use std::collections::BTreeMap;
use std::fmt::Write;

use heck::ToUpperCamelCase;

use crate::parser::{EnumValues, ParseResult};

/// A resolved enum type ready for code generation.
#[derive(Debug, Clone)]
pub struct EnumDef {
	/// PascalCase type name, e.g. `ReplyGroup`
	pub type_name: String,
	/// The enum values
	pub values: EnumValues,
}

/// Collect all enum parameters across all groups, deduplicate, and assign type names.
///
/// Returns a map from `(api_param_name, EnumValues)` → `EnumDef`.
/// This allows the emitter to look up the enum type for any param.
pub fn collect_enums(parsed: &ParseResult) -> BTreeMap<(String, EnumValues), EnumDef> {
	// First pass: collect all unique (param_name, values) pairs
	// and track how many distinct value sets each param_name has.
	let mut name_to_value_sets: BTreeMap<String, Vec<EnumValues>> = BTreeMap::new();

	for group in &parsed.groups {
		for method in &group.methods {
			let all_params = method
				.path_params
				.iter()
				.chain(method.query_params.iter())
				.chain(method.body_params.iter());

			for param in all_params {
				if let Some(ref ev) = param.enum_values {
					let sets = name_to_value_sets
						.entry(param.api_name.clone())
						.or_default();
					if !sets.contains(ev) {
						sets.push(ev.clone());
					}
				}
			}
		}
	}

	// Second pass: assign type names.
	// - If a param_name has exactly one value set → PascalCase(param_name)
	// - If a param_name has multiple value sets → need group prefix
	let mut result: BTreeMap<(String, EnumValues), EnumDef> = BTreeMap::new();

	// For conflicting names, we need to find which group uses which values
	let conflicting_names: Vec<String> = name_to_value_sets
		.iter()
		.filter(|(_, sets)| sets.len() > 1)
		.map(|(name, _)| name.clone())
		.collect();

	// Register non-conflicting enums first
	for (param_name, sets) in &name_to_value_sets {
		if sets.len() == 1 {
			let ev = &sets[0];
			let type_name = param_name_to_enum_type(param_name);
			result.insert(
				(param_name.clone(), ev.clone()),
				EnumDef {
					type_name,
					values: ev.clone(),
				},
			);
		}
	}

	// Register conflicting enums with group prefix (+ method prefix when needed)
	if !conflicting_names.is_empty() {
		// Track which type names are already assigned to which value sets,
		// so we can detect when group-level disambiguation is insufficient.
		let mut type_name_to_values: BTreeMap<String, EnumValues> = BTreeMap::new();
		for def in result.values() {
			type_name_to_values.insert(def.type_name.clone(), def.values.clone());
		}

		for group in &parsed.groups {
			for method in &group.methods {
				let all_params = method
					.path_params
					.iter()
					.chain(method.query_params.iter())
					.chain(method.body_params.iter());

				for param in all_params {
					if let Some(ref ev) = param.enum_values {
						if conflicting_names.contains(&param.api_name) {
							let key = (param.api_name.clone(), ev.clone());
							result.entry(key).or_insert_with(|| {
								let group_prefix = group.name.to_upper_camel_case();
								let base = param_name_to_enum_type(&param.api_name);
								let candidate = format!("{group_prefix}{base}");

								// If this type name is already taken by a different value set,
								// add the method name for further disambiguation.
								let type_name =
									if let Some(existing) = type_name_to_values.get(&candidate) {
										if existing != ev {
											let method_prefix = method.name.to_upper_camel_case();
											format!("{group_prefix}{method_prefix}{base}")
										} else {
											candidate.clone()
										}
									} else {
										candidate.clone()
									};

								type_name_to_values.insert(type_name.clone(), ev.clone());
								EnumDef {
									type_name,
									values: ev.clone(),
								}
							});
						}
					}
				}
			}
		}
	}

	result
}

/// Look up the enum type name for a parameter, given its api_name and enum_values.
pub fn lookup_enum_type(
	enums: &BTreeMap<(String, EnumValues), EnumDef>,
	api_name: &str,
	values: &EnumValues,
) -> Option<String> {
	enums
		.get(&(api_name.to_string(), values.clone()))
		.map(|def| def.type_name.clone())
}

/// Convert a param name to a PascalCase enum type name.
fn param_name_to_enum_type(param_name: &str) -> String {
	let name = param_name.to_upper_camel_case();
	// Identifiers can't start with a digit
	if name.starts_with(|c: char| c.is_ascii_digit()) {
		format!("V{name}")
	} else {
		name
	}
}

/// Generate the Rust code for all enum definitions.
pub fn emit_enum_definitions(enums: &BTreeMap<(String, EnumValues), EnumDef>) -> String {
	let mut out = String::new();

	// Deduplicate by type_name (multiple param_name+values keys can map to same type)
	let mut emitted: BTreeMap<String, &EnumDef> = BTreeMap::new();
	for def in enums.values() {
		emitted.entry(def.type_name.clone()).or_insert(def);
	}

	if emitted.is_empty() {
		return out;
	}

	writeln!(out, "// ── Enums ──").unwrap();
	writeln!(out).unwrap();

	for def in emitted.values() {
		match &def.values {
			EnumValues::Integer(vals) => emit_int_enum(&mut out, &def.type_name, vals),
			EnumValues::String(vals) => emit_string_enum(&mut out, &def.type_name, vals),
		}
	}

	out
}

fn emit_int_enum(out: &mut String, type_name: &str, values: &[i64]) {
	writeln!(
		out,
		"#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]"
	)
	.unwrap();
	writeln!(out, "#[serde(into = \"i64\", try_from = \"i64\")]").unwrap();
	writeln!(out, "pub enum {type_name} {{").unwrap();

	for val in values {
		let variant = int_variant_name(*val);
		writeln!(out, "\t{variant} = {val},").unwrap();
	}

	writeln!(out, "}}").unwrap();
	writeln!(out).unwrap();

	// From impl for serde(into)
	writeln!(out, "impl From<{type_name}> for i64 {{").unwrap();
	writeln!(out, "\tfn from(v: {type_name}) -> i64 {{").unwrap();
	writeln!(out, "\t\tv as i64").unwrap();
	writeln!(out, "\t}}").unwrap();
	writeln!(out, "}}").unwrap();
	writeln!(out).unwrap();

	// TryFrom impl for serde(try_from)
	writeln!(out, "impl TryFrom<i64> for {type_name} {{").unwrap();
	writeln!(out, "\ttype Error = String;").unwrap();
	writeln!(out, "\tfn try_from(v: i64) -> Result<Self, Self::Error> {{").unwrap();
	writeln!(out, "\t\tmatch v {{").unwrap();
	for val in values {
		let variant = int_variant_name(*val);
		writeln!(out, "\t\t\t{val} => Ok(Self::{variant}),").unwrap();
	}
	writeln!(
		out,
		"\t\t\tother => Err(format!(\"unknown {type_name} value: {{other}}\")),"
	)
	.unwrap();
	writeln!(out, "\t\t}}").unwrap();
	writeln!(out, "\t}}").unwrap();
	writeln!(out, "}}").unwrap();
	writeln!(out).unwrap();

	// Display impl for query param serialization
	writeln!(out, "impl std::fmt::Display for {type_name} {{").unwrap();
	writeln!(
		out,
		"\tfn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{"
	)
	.unwrap();
	writeln!(out, "\t\twrite!(f, \"{{}}\", *self as i64)").unwrap();
	writeln!(out, "\t}}").unwrap();
	writeln!(out, "}}").unwrap();
	writeln!(out).unwrap();
}

fn emit_string_enum(out: &mut String, type_name: &str, values: &[String]) {
	writeln!(
		out,
		"#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]"
	)
	.unwrap();
	writeln!(out, "pub enum {type_name} {{").unwrap();

	for val in values {
		let variant = string_variant_name(val);
		if variant != *val {
			writeln!(out, "\t#[serde(rename = \"{val}\")]").unwrap();
		}
		writeln!(out, "\t{variant},").unwrap();
	}

	// Catch-all variant for forward compatibility with unknown values
	writeln!(out, "\t/// Unknown variant not yet in the schema.").unwrap();
	writeln!(out, "\t#[serde(other)]").unwrap();
	writeln!(out, "\tUnknown,").unwrap();

	writeln!(out, "}}").unwrap();
	writeln!(out).unwrap();

	// Display impl for query param serialization
	writeln!(out, "impl std::fmt::Display for {type_name} {{").unwrap();
	writeln!(
		out,
		"\tfn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{"
	)
	.unwrap();
	writeln!(out, "\t\tmatch self {{").unwrap();
	for val in values {
		let variant = string_variant_name(val);
		writeln!(out, "\t\t\tSelf::{variant} => write!(f, \"{val}\"),").unwrap();
	}
	writeln!(out, "\t\t\tSelf::Unknown => write!(f, \"unknown\"),").unwrap();
	writeln!(out, "\t\t}}").unwrap();
	writeln!(out, "\t}}").unwrap();
	writeln!(out, "}}").unwrap();
	writeln!(out).unwrap();
}

/// Generate a variant name for an integer enum value.
/// Negative values get `Neg` prefix.
fn int_variant_name(val: i64) -> String {
	if val < 0 {
		format!("Neg{}", val.unsigned_abs())
	} else {
		format!("V{val}")
	}
}

/// Generate a PascalCase variant name for a string enum value.
fn string_variant_name(val: &str) -> String {
	if val.is_empty() {
		return "Empty".to_string();
	}
	// Handle values with spaces, slashes, etc.
	val.to_upper_camel_case()
}
