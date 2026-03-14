mod emitter;
mod parser;
mod transforms;
mod utils;

use std::path::Path;

struct ApiConfig {
	schema_path: &'static str,
	output_dir: &'static str,
	client_name: &'static str,
	default_base_url: &'static str,
	default_rate_limit: u32,
}

fn main() {
	let apis = [
		ApiConfig {
			schema_path: "schemas/forum.json",
			output_dir: "lolzteam/src/generated/forum",
			client_name: "ForumClient",
			default_base_url: "https://prod-api.lolz.live",
			default_rate_limit: 300,
		},
		ApiConfig {
			schema_path: "schemas/market.json",
			output_dir: "lolzteam/src/generated/market",
			client_name: "MarketClient",
			default_base_url: "https://prod-api.lzt.market",
			default_rate_limit: 120,
		},
	];

	for api in &apis {
		eprintln!("Processing {} ...", api.schema_path);

		let schema_path = Path::new(api.schema_path);
		let content = std::fs::read_to_string(schema_path).unwrap_or_else(|e| {
			panic!("failed to read {}: {e}", schema_path.display());
		});

		let spec: serde_json::Value = serde_json::from_str(&content).unwrap_or_else(|e| {
			panic!("failed to parse {}: {e}", schema_path.display());
		});

		let parsed = parser::parse_spec(&spec);

		eprintln!(
			"  found {} groups, {} total methods",
			parsed.groups.len(),
			parsed.groups.iter().map(|g| g.methods.len()).sum::<usize>()
		);

		emitter::emit_types(&parsed, api.output_dir);
		emitter::emit_client(
			&parsed,
			api.client_name,
			api.default_base_url,
			api.default_rate_limit,
			api.output_dir,
		);
		emitter::emit_mod(&parsed, api.output_dir);
	}

	// Emit the top-level generated/mod.rs
	emitter::emit_generated_mod("lolzteam/src/generated");

	// Format generated files
	eprintln!("Running cargo fmt on generated files...");
	let status = std::process::Command::new("cargo")
		.args(["fmt", "-p", "lolzteam"])
		.status()
		.expect("failed to run cargo fmt");
	if !status.success() {
		eprintln!("Warning: cargo fmt exited with {status}");
	}

	eprintln!("Done.");
}
