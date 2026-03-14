# lolzteam

Fully typed Rust API wrapper for [Lolzteam](https://lolz.live) Forum and Market APIs.

**151 Forum endpoints + 115 Market endpoints** — all generated from the official OpenAPI schemas.

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
lolzteam = "0.1"
tokio = { version = "1", features = ["full"] }
```

## Quick Start

```rust
use lolzteam::{ForumClient, MarketClient, Config};

#[tokio::main]
async fn main() -> Result<(), lolzteam::Error> {
    // Forum
    let forum = ForumClient::new(Config::new("your_token"));

    let threads = forum.threads().list(None).await?;
    println!("{}", threads.threads[0].thread_title);

    // Market
    let market = MarketClient::new(Config::new("your_token"));

    let items = market.category().steam(None).await?;
    println!("{}", items.items[0].title);

    let item = market.managing().get(123456, None).await?;
    println!("{}", item.item.title);

    Ok(())
}
```

## Features

- **Fully typed** — generated request/response structs from OpenAPI schemas via serde
- **Async** — all methods are async with tokio runtime
- **Auto-retry** — 429 (respects `Retry-After`), 502, 503 with exponential backoff + jitter
- **Rate limiting** — built-in token bucket (Forum: 300 req/min, Market: 120 req/min)
- **Proxy support** — via reqwest (HTTP/HTTPS/SOCKS5)
- **File uploads** — `multipart/form-data` for avatar/background endpoints
- **Error hierarchy** — typed errors with `thiserror`

## Configuration

```rust
use lolzteam::{ForumClient, Config, RetryConfig};

let forum = ForumClient::new(
    Config::new("your_bearer_token")
        // Optional: custom base URL
        .base_url("https://api.lolz.live")
        // Optional: proxy
        .proxy("http://user:pass@proxy:8080")
        // Optional: retry config
        .retry(RetryConfig {
            max_retries: 5,       // default: 3
            base_delay_ms: 2000,  // default: 1000
            max_delay_ms: 60000,  // default: 30000
        })
        // Optional: rate limit override
        .requests_per_minute(60),
);
```

## Error Handling

```rust
use lolzteam::errors::{Error, HttpError};

match market.managing().get(999999, None).await {
    Ok(item) => println!("{}", item.item.title),
    Err(Error::RateLimit { retry_after, .. }) => {
        println!("Rate limited, retry after {retry_after}s");
    }
    Err(Error::Auth { .. }) => {
        println!("Invalid token");
    }
    Err(Error::NotFound { .. }) => {
        println!("Item not found");
    }
    Err(Error::Server { status, .. }) => {
        println!("Server error: {status}");
    }
    Err(Error::Network(e)) => {
        println!("Network error: {e}");
    }
    Err(e) => println!("Other error: {e}"),
}
```

## API Groups

### ForumClient

| Group | Methods | Description |
|-------|---------|-------------|
| `forum.threads()` | 22 | Threads CRUD, follow, bump, move |
| `forum.posts()` | 15 | Posts CRUD, like, report |
| `forum.users()` | 26 | Users, avatar/background upload, settings |
| `forum.conversations()` | 23 | Private conversations |
| `forum.profile_posts()` | 18 | Profile posts and comments |
| `forum.chatbox()` | 12 | Chat messages |
| `forum.forums()` | 9 | Forum listing, follow |
| `forum.search()` | 7 | Search threads, posts, users |
| `forum.tags()` | 4 | Content tags |
| `forum.notifications()` | 3 | Notifications |
| `forum.categories()` | 2 | Categories |
| `forum.forms()` | 2 | Forms |
| `forum.links()` | 2 | Link forums |
| `forum.pages()` | 2 | Pages |
| `forum.assets()` | 1 | CSS assets |
| `forum.batch()` | 1 | Batch requests |
| `forum.navigation()` | 1 | Navigation |
| `forum.o_auth()` | 1 | OAuth token |

### MarketClient

| Group | Methods | Description |
|-------|---------|-------------|
| `market.managing()` | 40 | Account management, edit, steam values |
| `market.category()` | 28 | Category search (Steam, Fortnite, etc.) |
| `market.payments()` | 12 | Payments, invoices, balance |
| `market.list()` | 6 | User items, orders, favorites |
| `market.purchasing()` | 5 | Buy, confirm, discount requests |
| `market.publishing()` | 4 | Publish accounts for sale |
| `market.custom_discounts()` | 4 | Custom discount management |
| `market.cart()` | 3 | Shopping cart |
| `market.auto_payments()` | 3 | Auto-payment setup |
| `market.profile()` | 3 | User profile |
| `market.proxy()` | 3 | Proxy management |
| `market.imap()` | 2 | IMAP email management |
| `market.batch()` | 1 | Batch requests |

## Code Generation

All client code and types are **generated from OpenAPI 3.1.0 schemas** — not written by hand.

```bash
cargo run -p codegen
```

This reads `schemas/forum.json` and `schemas/market.json`, resolves all `$ref` pointers, and emits typed Rust files. Generated code is committed to the repo — no codegen step needed at build time.

### Where types are generated

| What | Where |
|------|-------|
| Generator source | `codegen/` |
| Forum types + groups | `lolzteam/src/generated/forum/` |
| Market types + groups | `lolzteam/src/generated/market/` |

## Project Structure

```
lolzteam-rust/
  schemas/              OpenAPI 3.1.0 specs (forum.json, market.json)
  codegen/              Code generator (reads OpenAPI -> emits Rust code)
  lolzteam/             Library crate
    src/runtime/        HTTP client, retry, rate limiter, proxy, auth, errors
    src/generated/      Generated clients and types (committed to repo)
    src/lib.rs          Public API re-exports
    Cargo.toml
  Cargo.toml            Workspace root
```

## Development

```bash
cargo run -p codegen    # Regenerate clients from OpenAPI schemas
cargo clippy            # Lint
cargo fmt               # Format
cargo test              # Run tests
cargo build             # Build
```

## License

MIT
