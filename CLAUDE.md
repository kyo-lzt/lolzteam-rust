# lolzteam-rust

API wrapper for Lolzteam Forum and Market, generated from OpenAPI specs.

## Project structure

```
schemas/          — OpenAPI 3.1.0 specs (forum.json, market.json)
codegen/          — Code generator (reads OpenAPI → emits Rust client code)
lolzteam/         — Library crate (runtime + generated clients)
  src/runtime/    — HTTP client, retry, rate limiter, proxy, auth, errors
  src/generated/  — Generated clients and types (committed to repo)
```

## Key commands

- `cargo build` — build all
- `cargo run -p codegen` — regenerate clients from OpenAPI schemas
- `cargo test` — run tests
- `cargo clippy` — lint
- `cargo fmt` — format

## Architecture

- **Two separate clients**: `ForumClient` and `MarketClient` (different baseUrl, rate limits)
- **DX pattern**: `client.threads().list(None).await?` — methods grouped by API tags
- **Codegen**: Reads OpenAPI JSON, resolves $refs, groups by tags, emits typed Rust code
- **Runtime**: reqwest for HTTP, reqwest proxy feature for proxy support
- **Generated code is committed** — no codegen at build time

## API details

- **Auth**: Bearer token (both Forum and Market)
- **Forum**: base URL `https://api.lolz.live`, rate limit 300 req/min
- **Market**: base URL `https://api.lzt.market`, rate limit 120 req/min
- **Retry**: 429 (Retry-After), 502, 503 — exponential backoff + jitter
- **Parameters**: snake_case (1:1 with API)
- **Body encoding**: application/x-www-form-urlencoded

## Conventions

- No `unwrap()` in library code — use proper error handling
- Custom error hierarchy with thiserror
- serde for serialization with `#[serde(skip_serializing_if = "Option::is_none")]`
- All async with tokio runtime
