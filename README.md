# lolzteam-rust

Rust API wrapper for Lolzteam Forum and Market (async, tokio). Clients and types are generated from OpenAPI specs.

## Requirements

- Rust 1.75+ (edition 2021)
- Cargo

## Setup

```bash
git clone https://github.com/kyo-lzt/lolzteam-rust.git
cd lolzteam-rust
cargo build
```

## Code Generation

```bash
cargo run -p codegen
```

Reads schemas from `schemas/forum.json` and `schemas/market.json`, generates typed clients into:

| What | Where |
|------|-------|
| Forum types | `lolzteam/src/generated/forum/types.rs` |
| Market types | `lolzteam/src/generated/market/types.rs` |
| Forum client | `lolzteam/src/generated/forum/client.rs` |
| Market client | `lolzteam/src/generated/market/client.rs` |

Generator source — `codegen/`.

## Project Structure

```
schemas/              — OpenAPI 3.1.0 specs
codegen/              — Code generator
lolzteam/             — Library crate
  src/runtime/        — HTTP client, retry, rate limiter, proxy, errors
  src/generated/      — Generated code (committed to repo)
  src/lib.rs          — Public re-exports
  Cargo.toml
Cargo.toml            — Workspace root
```

## Commands

```bash
cargo run -p codegen    # Generate clients from schemas
cargo clippy            # Lint
cargo fmt               # Format
cargo test              # Tests
cargo build             # Build
```

## License

MIT
