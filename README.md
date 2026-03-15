# lolzteam-rust

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![CI](https://github.com/kyo-lzt/lolzteam-rust/actions/workflows/ci.yml/badge.svg)](https://github.com/kyo-lzt/lolzteam-rust/actions)

Async Rust API wrapper for the [Lolzteam](https://lolz.live) Forum and Market APIs. **266 endpoints** (151 Forum + 115 Market), auto-generated from OpenAPI specifications.

## Features

- **Complete API coverage** -- 266 endpoints across Forum and Market clients
- **Auto-generated** -- clients and types generated from OpenAPI 3.1.0 specs, always in sync
- **Async-only** -- built on `tokio` + `reqwest`, no blocking API
- **Proxy support** -- HTTP, HTTPS, and SOCKS5 via reqwest
- **Retry logic** -- exponential backoff with jitter, respects `Retry-After` headers
- **Rate limiting** -- token bucket algorithm, thread-safe (`tokio::sync::Mutex`)
- **Typed errors** -- structured error hierarchy with `thiserror`
- **TLS** -- rustls by default, no OpenSSL dependency

## Quick Start

Add the dependency:

```toml
[dependencies]
lolzteam = { path = "../lolzteam-rust/lolzteam" }
tokio = { version = "1", features = ["full"] }
```

Requires **Rust 1.75+** (edition 2021).

## Usage

```rust
use lolzteam::generated::forum::client::ForumClient;
use lolzteam::generated::market::client::MarketClient;
use lolzteam::runtime::LolzteamError;

#[tokio::main]
async fn main() -> Result<(), LolzteamError> {
    let forum = ForumClient::new("your_token")?;
    let market = MarketClient::new("your_token")?;

    let threads = forum.threads().list(None).await?;
    let items = market.category().list(None).await?;

    Ok(())
}
```

Forum API groups: `assets`, `batch`, `categories`, `chatbox`, `conversations`, `forms`, `forums`, `links`, `navigation`, `notifications`, `o_auth`, `pages`, `posts`, `profile_posts`, `search`, `tags`, `threads`, `users`.

Market API groups: `auto_payments`, `batch`, `cart`, `category`, `custom_discounts`, `imap`, `list`, `managing`, `payments`, `profile`, `proxy`, `publishing`, `purchasing`.

## Configuration

```rust
use lolzteam::runtime::{ClientConfig, ProxyConfig, RetryConfig, RateLimitConfig};

let config = ClientConfig {
    token: "your_token".to_string(),
    base_url: "https://prod-api.lolz.live".to_string(),
    proxy: Some(ProxyConfig {
        url: "socks5://127.0.0.1:1080".to_string(),
    }),
    retry: RetryConfig {
        max_retries: 5,        // default: 3
        base_delay_ms: 1000,   // default: 1000
        max_delay_ms: 30_000,  // default: 30000
    },
    rate_limit: Some(RateLimitConfig {
        requests_per_minute: 200,  // default: 300 (Forum), 120 (Market)
    }),
};

let forum = ForumClient::with_config(config)?;
```

## Retry Logic

Failed requests are retried automatically for transient errors. The delay uses exponential backoff with jitter. `Retry-After` header on 429 responses is respected.

| Status | Retried | Behavior |
|--------|---------|----------|
| 429 | Yes | Uses `Retry-After` header if present |
| 502, 503, 504 | Yes | Exponential backoff with jitter |
| Network errors | Yes | Timeout and connection errors |
| 401, 403 | No | Thrown immediately |
| 404 | No | Thrown immediately |

Delay formula: `min(base_delay * 2^attempt + random(0, base_delay), max_delay)`

```rust
// Disable retry
let client = MarketClient::with_config(ClientConfig {
    token: "...".to_string(),
    retry: None,
    ..Default::default()
})?;

// on_retry callback
let client = MarketClient::with_config(ClientConfig {
    token: "...".to_string(),
    on_retry: Some(Arc::new(|info| {
        println!("Retry #{}", info.attempt);
    })),
    ..Default::default()
})?;
```

## Proxy

Configured via `ProxyConfig`. Supported schemes: `http`, `https`, `socks5`.

```rust
// HTTP proxy
ProxyConfig { url: "http://proxy.example.com:8080".to_string() }

// Authenticated proxy
ProxyConfig { url: "http://user:pass@127.0.0.1:8080".to_string() }

// SOCKS5 proxy
ProxyConfig { url: "socks5://127.0.0.1:1080".to_string() }
```

The proxy URL is validated at client construction time. Invalid schemes produce a `ConfigError`.

## Error Handling

All errors are represented by `LolzteamError`:

```rust
use lolzteam::runtime::LolzteamError;

match result {
    Err(LolzteamError::Http(e)) => {
        println!("status: {}, body: {}", e.status, e.body);
        if e.is_rate_limit() { /* 429 */ }
        if e.is_auth_error() { /* 401 or 403 */ }
        if e.is_not_found() { /* 404 */ }
    }
    Err(LolzteamError::Network(e)) => {
        println!("network error: {e}");
    }
    Err(LolzteamError::Config(e)) => {
        println!("config error: {e}");
    }
    Ok(data) => { /* success */ }
}
```

Error hierarchy:

```
LolzteamError
├── Http
│   ├── is_rate_limit()    (429)
│   ├── is_auth_error()    (401, 403)
│   ├── is_not_found()     (404)
│   └── is_server_error()  (5xx)
├── Network
└── Config
```

## Rate Limits

The built-in rate limiter uses a token bucket algorithm. Thread-safe via `tokio::sync::Mutex`, safe to share across tasks via `Arc`. When the bucket is empty, `acquire()` awaits until tokens refill -- no requests are dropped.

| Client | Default limit |
|--------|---------------|
| Forum  | 300 req/min   |
| Market | 120 req/min   |
| Market (search) | 20 req/min |

```rust
let client = MarketClient::with_config(ClientConfig {
    token: "...".to_string(),
    search_rate_limit: Some(RateLimitConfig { requests_per_minute: 30 }),
    ..Default::default()
})?;
```

## Code Generation

Clients and types are auto-generated from OpenAPI 3.1.0 specs in `schemas/`:

```bash
cargo run -p codegen
```

| Input | Output |
|-------|--------|
| `schemas/forum.json` | `lolzteam/src/generated/forum/client.rs`, `types.rs` |
| `schemas/market.json` | `lolzteam/src/generated/market/client.rs`, `types.rs` |

Generator source is in `codegen/`.

## Project Structure

```
schemas/                        OpenAPI 3.1.0 specifications
codegen/                        Code generator crate
lolzteam/                       Library crate
  src/
    runtime/
      http_client.rs            HTTP client (auth, rate limit, retry, proxy)
      retry.rs                  Exponential backoff with jitter
      rate_limiter.rs           Token bucket rate limiter
      errors.rs                 Error types
      types.rs                  Config structs
    generated/
      forum/
        client.rs               ForumClient (18 API groups, 151 methods)
        types.rs                Request/response types
      market/
        client.rs               MarketClient (13 API groups, 115 methods)
        types.rs                Request/response types
    lib.rs                      Public re-exports
  Cargo.toml
Cargo.toml                      Workspace root
```

## Commands

```bash
cargo run -p codegen    # Generate clients from OpenAPI specs
cargo build             # Build all crates
cargo test              # Run tests
cargo clippy            # Lint
cargo fmt               # Format
```

## Dependencies

| Crate | Purpose |
|-------|---------|
| reqwest | HTTP client (rustls-tls, socks, multipart) |
| serde / serde_json | Serialization |
| tokio | Async runtime |
| thiserror | Error derive macros |
| fastrand | Random jitter for retry backoff |

## License

[MIT](LICENSE)
