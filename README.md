# lolzteam-rust

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![CI](https://github.com/kyo-lzt/lolzteam-rust/actions/workflows/ci.yml/badge.svg)](https://github.com/kyo-lzt/lolzteam-rust/actions)

Async Rust SDK для [Lolzteam](https://lolz.live) Forum и Market API. **266 эндпоинтов** (151 Forum + 115 Market), автоматически сгенерированные из OpenAPI спецификаций.

---

## Содержание / Table of Contents

- [Быстрый старт / Quick Start](#быстрый-старт--quick-start)
- [Опции клиента / Client Options](#опции-клиента--client-options)
- [Прокси / Proxy](#прокси--proxy)
- [Авто-retry / Auto-retry](#авто-retry--auto-retry)
- [Обработка ошибок / Error Handling](#обработка-ошибок--error-handling)
- [Rate Limits](#rate-limits)
- [Forum API](#forum-api)
- [Market API](#market-api)
- [Генерация кода / Code Generation](#генерация-кода--code-generation)
- [Сборка и тесты / Build & Test](#сборка-и-тесты--build--test)
- [Структура проекта / Project Structure](#структура-проекта--project-structure)
- [Лицензия / License](#лицензия--license)

---

## Быстрый старт / Quick Start

Требуется **Rust 1.75+** (edition 2021).

```bash
git clone https://github.com/kyo-lzt/lolzteam-rust.git
cd lolzteam-rust
cargo build
```

Добавьте зависимость в `Cargo.toml`:

```toml
[dependencies]
lolzteam = { path = "../lolzteam-rust/lolzteam" }
tokio = { version = "1", features = ["full"] }
```

Минимальный пример:

```rust
use lolzteam::generated::forum::client::ForumClient;
use lolzteam::generated::market::client::MarketClient;
use lolzteam::runtime::LolzteamError;

#[tokio::main]
async fn main() -> Result<(), LolzteamError> {
    let forum = ForumClient::new("your_token")?;
    let market = MarketClient::new("your_token")?;

    // Forum: получить список тем
    let threads = forum.threads().list(None).await?;

    // Market: получить все категории
    let items = market.category().list(None).await?;

    Ok(())
}
```

---

## Опции клиента / Client Options

Все настройки передаются через `ClientConfig`:

| Параметр | Тип | По умолчанию | Описание |
|----------|-----|-------------|----------|
| `token` | `String` | *обязательный* | API токен доступа |
| `base_url` | `String` | per-client | Базовый URL API |
| `proxy` | `Option<ProxyConfig>` | `None` | Прокси (http/https/socks5) |
| `retry` | `Option<RetryConfig>` | 3 попытки, 1s base, 30s max | Настройки повторов |
| `rate_limit` | `Option<RateLimitConfig>` | Forum: 300/min, Market: 120/min | Лимит запросов |
| `search_rate_limit` | `Option<RateLimitConfig>` | 20/min | Лимит поисковых запросов |
| `timeout_ms` | `Option<u64>` | `30000` (30s) | Таймаут запроса в мс |
| `on_retry` | `Option<Arc<dyn Fn(RetryInfo) + Send + Sync>>` | `None` | Колбэк при повторе |

```rust
use std::sync::Arc;
use lolzteam::runtime::{ClientConfig, ProxyConfig, RetryConfig, RateLimitConfig};

let config = ClientConfig {
    token: "your_token".to_string(),
    base_url: "https://prod-api.lolz.live".to_string(),
    proxy: Some(ProxyConfig {
        url: "socks5://127.0.0.1:1080".to_string(),
    }),
    retry: Some(RetryConfig {
        max_retries: 5,
        base_delay_ms: 1000,
        max_delay_ms: 30_000,
    }),
    rate_limit: Some(RateLimitConfig {
        requests_per_minute: 200,
    }),
    search_rate_limit: Some(RateLimitConfig {
        requests_per_minute: 30,
    }),
    timeout_ms: Some(10_000),
    on_retry: Some(Arc::new(|info| {
        println!("Retry #{} after {}ms", info.attempt, info.delay_ms);
    })),
};

let forum = ForumClient::with_config(config)?;
```

---

## Прокси / Proxy

Поддерживаемые схемы: `http`, `https`, `socks5`. URL валидируется при создании клиента — невалидная схема вернёт `ConfigError`.

```rust
// HTTP прокси
ProxyConfig { url: "http://proxy.example.com:8080".to_string() }

// Прокси с авторизацией
ProxyConfig { url: "http://user:pass@127.0.0.1:8080".to_string() }

// SOCKS5 прокси
ProxyConfig { url: "socks5://127.0.0.1:1080".to_string() }
```

---

## Авто-retry / Auto-retry

Неудачные запросы повторяются автоматически для транзиентных ошибок. Задержка — экспоненциальный backoff с джиттером. Заголовок `Retry-After` на 429 учитывается.

| Статус | Повтор | Поведение |
|--------|--------|-----------|
| 429 | Да | Использует `Retry-After` если есть |
| 502, 503, 504 | Да | Экспоненциальный backoff с джиттером |
| Сетевые ошибки | Да | Таймаут и ошибки соединения |
| 401, 403 | Нет | Бросается сразу |
| 404 | Нет | Бросается сразу |

Формула задержки: `min(base_delay * 2^attempt + random(0, base_delay), max_delay)`

```rust
// Отключить retry
let client = MarketClient::with_config(ClientConfig {
    token: "...".to_string(),
    retry: None,
    ..Default::default()
})?;

// Колбэк on_retry
let client = MarketClient::with_config(ClientConfig {
    token: "...".to_string(),
    on_retry: Some(Arc::new(|info| {
        println!("Retry #{}", info.attempt);
    })),
    ..Default::default()
})?;
```

---

## Обработка ошибок / Error Handling

Все ошибки представлены `LolzteamError`:

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

---

## Rate Limits

Встроенный rate limiter использует алгоритм token bucket. Потокобезопасен через `tokio::sync::Mutex`, можно расшарить между задачами через `Arc`. Когда bucket пуст, `acquire()` ждёт пополнения — запросы не отбрасываются.

| Клиент | Лимит по умолчанию |
|--------|--------------------|
| Forum  | 300 req/min |
| Market | 120 req/min |
| Market (search) | 20 req/min |

```rust
let client = MarketClient::with_config(ClientConfig {
    token: "...".to_string(),
    search_rate_limit: Some(RateLimitConfig { requests_per_minute: 30 }),
    ..Default::default()
})?;
```

---

## Forum API

### OAuth

```rust
// Получить токен доступа (POST /oauth/token)
let token = forum.o_auth().token(Some(&OAuthTokenBody::ClientCredentials {
    client_id: "id".into(), client_secret: "secret".into(), scope: vec![],
})).await?;
```

### Ассеты / Assets

```rust
// Получить CSS ассеты (GET /assets/css)
let css = forum.assets().css(None).await?;
```

### Категории / Categories

```rust
// Список категорий (GET /categories)
let categories = forum.categories().list(None).await?;

// Получить категорию (GET /categories/{category_id})
let category = forum.categories().get(1).await?;
```

### Форумы / Forums

```rust
// Список форумов (GET /forums)
let forums = forum.forums().list(None).await?;

// Форумы с группировкой (GET /forums/grouped)
let grouped = forum.forums().grouped().await?;

// Получить форум (GET /forums/{forum_id})
let f = forum.forums().get(1).await?;

// Подписчики форума (GET /forums/{forum_id}/followers)
let followers = forum.forums().followers(1).await?;

// Подписаться на форум (POST /forums/{forum_id}/followers)
let follow = forum.forums().follow(1, None).await?;

// Отписаться от форума (DELETE /forums/{forum_id}/followers)
let unfollow = forum.forums().unfollow(1).await?;

// Отслеживаемые форумы (GET /forums/followed)
let followed = forum.forums().followed(None).await?;

// Получить настройки ленты (GET /forums/feed-options)
let opts = forum.forums().get_feed_options().await?;

// Изменить настройки ленты (PUT /forums/feed-options)
let edited = forum.forums().edit_feed_options(None).await?;
```

### Ссылки / Links

```rust
// Список ссылок (GET /links)
let links = forum.links().list().await?;

// Получить ссылку (GET /links/{link_id})
let link = forum.links().get(1).await?;
```

### Страницы / Pages

```rust
// Список страниц (GET /pages)
let pages = forum.pages().list(None).await?;

// Получить страницу (GET /pages/{page_id})
let page = forum.pages().get(1).await?;
```

### Навигация / Navigation

```rust
// Список элементов навигации (GET /navigation)
let nav = forum.navigation().list(None).await?;
```

### Темы / Threads

```rust
// Список тем (GET /threads)
let threads = forum.threads().list(None).await?;

// Создать тему (POST /threads)
let thread = forum.threads().create(None).await?;

// Создать конкурс (POST /threads/contests)
let contest = forum.threads().create_contest(None).await?;

// Заявка на награду (POST /threads/claims)
let claim = forum.threads().claim(None).await?;

// Получить тему (GET /threads/{thread_id})
let thread = forum.threads().get(1, None).await?;

// Редактировать тему (PUT /threads/{thread_id})
let edited = forum.threads().edit(1, None).await?;

// Удалить тему (DELETE /threads/{thread_id})
let deleted = forum.threads().delete(1, None).await?;

// Переместить тему (POST /threads/{thread_id}/move)
let moved = forum.threads().move_(1, None).await?;

// Поднять тему (POST /threads/{thread_id}/bump)
let bumped = forum.threads().bump(1).await?;

// Скрыть тему (POST /threads/{thread_id}/hide)
let hidden = forum.threads().hide(1).await?;

// Добавить в избранное (POST /threads/{thread_id}/star)
let starred = forum.threads().star(1).await?;

// Убрать из избранного (DELETE /threads/{thread_id}/star)
let unstarred = forum.threads().unstar(1).await?;

// Подписчики темы (GET /threads/{thread_id}/followers)
let followers = forum.threads().followers(1).await?;

// Подписаться на тему (POST /threads/{thread_id}/followers)
let follow = forum.threads().follow(1, None).await?;

// Отписаться от темы (DELETE /threads/{thread_id}/followers)
let unfollow = forum.threads().unfollow(1).await?;

// Отслеживаемые темы (GET /threads/followed)
let followed = forum.threads().followed(None).await?;

// Навигация по теме (GET /threads/{thread_id}/navigation)
let nav = forum.threads().navigation(1).await?;

// Получить опрос (GET /threads/{thread_id}/poll)
let poll = forum.threads().poll_get(1).await?;

// Голосовать в опросе (POST /threads/{thread_id}/poll/votes)
let vote = forum.threads().poll_vote(1, None).await?;

// Непрочитанные темы (GET /threads/unread)
let unread = forum.threads().unread(None).await?;

// Последние темы (GET /threads/recent)
let recent = forum.threads().recent(None).await?;

// Завершить конкурс (POST /threads/{thread_id}/finish)
let finished = forum.threads().finish(1).await?;
```

### Посты / Posts

```rust
// Список постов (GET /posts)
let posts = forum.posts().list(None).await?;

// Создать пост (POST /posts)
let post = forum.posts().create(None).await?;

// Получить пост (GET /posts/{post_id})
let post = forum.posts().get(1).await?;

// Редактировать пост (PUT /posts/{post_id})
let edited = forum.posts().edit(1, None).await?;

// Удалить пост (DELETE /posts/{post_id})
let deleted = forum.posts().delete(1, None).await?;

// Лайки поста (GET /posts/{post_id}/likes)
let likes = forum.posts().likes(1, None).await?;

// Лайкнуть пост (POST /posts/{post_id}/likes)
let liked = forum.posts().like(1).await?;

// Убрать лайк (DELETE /posts/{post_id}/likes)
let unliked = forum.posts().unlike(1).await?;

// Причины жалоб (GET /posts/{post_id}/report/reasons)
let reasons = forum.posts().report_reasons(1).await?;

// Пожаловаться на пост (POST /posts/{post_id}/report)
let report = forum.posts().report(1, None).await?;

// Получить комментарии (GET /posts/comments)
let comments = forum.posts().comments_get(None).await?;

// Создать комментарий (POST /posts/comments)
let comment = forum.posts().comments_create(None).await?;

// Редактировать комментарий (PUT /posts/comments)
let edited = forum.posts().comments_edit(None).await?;

// Удалить комментарий (DELETE /posts/comments)
let deleted = forum.posts().comments_delete(None).await?;

// Пожаловаться на комментарий (POST /posts/comments/report)
let report = forum.posts().comments_report(None).await?;
```

### Пользователи / Users

```rust
// Список пользователей (GET /users)
let users = forum.users().list(None).await?;

// Поля пользователей (GET /users/fields)
let fields = forum.users().fields().await?;

// Поиск пользователей (GET /users/find)
let found = forum.users().find(None).await?;

// Получить пользователя (GET /users/{user_id})
let user = forum.users().get(1.into(), None).await?;

// Редактировать пользователя (PUT /users/{user_id})
let edited = forum.users().edit(1.into(), None).await?;

// Жалобы пользователя (GET /users/{user_id}/claims)
let claims = forum.users().claims(1.into(), None).await?;

// Загрузить аватар (POST /users/{user_id}/avatar)
let avatar = forum.users().avatar_upload(1.into(), None).await?;

// Удалить аватар (DELETE /users/{user_id}/avatar)
let deleted = forum.users().avatar_delete(1.into()).await?;

// Обрезать аватар (POST /users/{user_id}/avatar-crop)
let cropped = forum.users().avatar_crop(1.into(), None).await?;

// Загрузить фон (POST /users/{user_id}/background)
let bg = forum.users().background_upload(1.into(), None).await?;

// Удалить фон (DELETE /users/{user_id}/background)
let deleted = forum.users().background_delete(1.into()).await?;

// Обрезать фон (POST /users/{user_id}/background-crop)
let cropped = forum.users().background_crop(1.into(), None).await?;

// Подписчики (GET /users/{user_id}/followers)
let followers = forum.users().followers(1.into(), None).await?;

// Подписаться (POST /users/{user_id}/followers)
let follow = forum.users().follow(1.into()).await?;

// Отписаться (DELETE /users/{user_id}/followers)
let unfollow = forum.users().unfollow(1.into()).await?;

// Подписки (GET /users/{user_id}/followings)
let followings = forum.users().followings(1.into(), None).await?;

// Список игнора (GET /users/ignored)
let ignored = forum.users().ignored(None).await?;

// Добавить в игнор (POST /users/{user_id}/ignore)
let ignore = forum.users().ignore(1.into()).await?;

// Редактировать игнор (PUT /users/{user_id}/ignore)
let edit = forum.users().ignore_edit(1.into(), None).await?;

// Убрать из игнора (DELETE /users/{user_id}/ignore)
let unignore = forum.users().unignore(1.into()).await?;

// Лайки пользователя (GET /users/{user_id}/likes)
let likes = forum.users().likes(1.into(), None).await?;

// Контент пользователя (GET /users/{user_id}/contents)
let contents = forum.users().contents(1.into(), None).await?;

// Трофеи (GET /users/{user_id}/trophies)
let trophies = forum.users().trophies(1.into()).await?;

// Типы секретных ответов (GET /users/secret-answer-types)
let types = forum.users().secret_answer_types().await?;

// Сброс секретного ответа (POST /users/secret-answer/reset)
let reset = forum.users().sa_reset().await?;

// Отмена сброса секретного ответа (DELETE /users/secret-answer/reset)
let cancel = forum.users().sa_cancel_reset().await?;
```

### Посты профиля / Profile Posts

```rust
// Список постов профиля (GET /profile-posts)
let posts = forum.profile_posts().list(1.into(), None).await?;

// Создать пост профиля (POST /profile-posts)
let post = forum.profile_posts().create(None).await?;

// Получить пост профиля (GET /profile-posts/{profile_post_id})
let post = forum.profile_posts().get(1).await?;

// Редактировать пост профиля (PUT /profile-posts/{profile_post_id})
let edited = forum.profile_posts().edit(1, None).await?;

// Удалить пост профиля (DELETE /profile-posts/{profile_post_id})
let deleted = forum.profile_posts().delete(1, None).await?;

// Причины жалоб (GET /profile-posts/{profile_post_id}/report/reasons)
let reasons = forum.profile_posts().report_reasons(1).await?;

// Пожаловаться (POST /profile-posts/{profile_post_id}/report)
let report = forum.profile_posts().report(1, None).await?;

// Закрепить пост (POST /profile-posts/{profile_post_id}/stick)
let stick = forum.profile_posts().stick(1).await?;

// Открепить пост (DELETE /profile-posts/{profile_post_id}/stick)
let unstick = forum.profile_posts().unstick(1).await?;

// Лайки поста (GET /profile-posts/{profile_post_id}/likes)
let likes = forum.profile_posts().likes(1).await?;

// Лайкнуть (POST /profile-posts/{profile_post_id}/likes)
let liked = forum.profile_posts().like(1).await?;

// Убрать лайк (DELETE /profile-posts/{profile_post_id}/likes)
let unliked = forum.profile_posts().unlike(1).await?;

// Список комментариев (GET /profile-posts/comments)
let comments = forum.profile_posts().comments_list(None).await?;

// Создать комментарий (POST /profile-posts/comments)
let comment = forum.profile_posts().comments_create(None).await?;

// Редактировать комментарий (PUT /profile-posts/comments)
let edited = forum.profile_posts().comments_edit(None).await?;

// Удалить комментарий (DELETE /profile-posts/comments)
let deleted = forum.profile_posts().comments_delete(None).await?;

// Получить комментарий (GET /profile-posts/{profile_post_id}/comments/{comment_id})
let comment = forum.profile_posts().comments_get(1, 1).await?;

// Пожаловаться на комментарий (POST /profile-posts/comments/{comment_id}/report)
let report = forum.profile_posts().comments_report(1, None).await?;
```

### Личные сообщения / Conversations

```rust
// Список диалогов (GET /conversations)
let convos = forum.conversations().list(None).await?;

// Создать диалог (POST /conversations)
let convo = forum.conversations().create(None).await?;

// Обновить диалог (PUT /conversations)
let updated = forum.conversations().update(None).await?;

// Удалить диалог (DELETE /conversations)
let deleted = forum.conversations().delete(None).await?;

// Начать диалог (POST /conversations/start)
let started = forum.conversations().start(None).await?;

// Сохранить диалог (POST /conversations/save)
let saved = forum.conversations().save(None).await?;

// Получить диалог (GET /conversations/{conversation_id})
let convo = forum.conversations().get(1).await?;

// Список сообщений (GET /conversations/{conversation_id}/messages)
let msgs = forum.conversations().messages_list(1, None).await?;

// Создать сообщение (POST /conversations/{conversation_id}/messages)
let msg = forum.conversations().messages_create(1, None).await?;

// Поиск диалогов (POST /conversations/search)
let results = forum.conversations().search(None).await?;

// Получить сообщение (GET /conversations/messages/{message_id})
let msg = forum.conversations().messages_get(1).await?;

// Редактировать сообщение (PUT /conversations/{conversation_id}/messages/{message_id})
let edited = forum.conversations().messages_edit(1, 1, None).await?;

// Удалить сообщение (DELETE /conversations/{conversation_id}/messages/{message_id})
let deleted = forum.conversations().messages_delete(1, 1).await?;

// Пригласить в диалог (POST /conversations/{conversation_id}/invite)
let invite = forum.conversations().invite(1, None).await?;

// Кикнуть из диалога (POST /conversations/{conversation_id}/kick)
let kick = forum.conversations().kick(1, None).await?;

// Прочитать диалог (POST /conversations/{conversation_id}/read)
let read = forum.conversations().read(1).await?;

// Прочитать все (POST /conversations/read)
let read_all = forum.conversations().read_all().await?;

// Закрепить сообщение (POST /conversations/{conversation_id}/messages/{message_id}/stick)
let stick = forum.conversations().messages_stick(1, 1).await?;

// Открепить сообщение (DELETE /conversations/{conversation_id}/messages/{message_id}/stick)
let unstick = forum.conversations().messages_unstick(1, 1).await?;

// Пометить диалог звездой (POST /conversations/{conversation_id}/star)
let star = forum.conversations().star(1).await?;

// Убрать звезду (DELETE /conversations/{conversation_id}/star)
let unstar = forum.conversations().unstar(1).await?;

// Включить уведомления (POST /conversations/{conversation_id}/alerts/enable)
let enable = forum.conversations().alerts_enable(1).await?;

// Отключить уведомления (POST /conversations/{conversation_id}/alerts/disable)
let disable = forum.conversations().alerts_disable(1).await?;
```

### Уведомления / Notifications

```rust
// Список уведомлений (GET /notifications)
let notifs = forum.notifications().list(None).await?;

// Получить уведомление (GET /notifications/{notification_id})
let notif = forum.notifications().get(1).await?;

// Прочитать уведомления (POST /notifications/read)
let read = forum.notifications().read(None).await?;
```

### Теги / Tags

```rust
// Популярные теги (GET /tags/popular)
let popular = forum.tags().popular().await?;

// Список тегов (GET /tags)
let tags = forum.tags().list(None).await?;

// Получить тег (GET /tags/{tag_id})
let tag = forum.tags().get(1, None).await?;

// Найти тег (GET /tags/find)
let found = forum.tags().find("rust".into()).await?;
```

### Поиск / Search

```rust
// Поиск по всему (POST /search)
let all = forum.search().all(None).await?;

// Поиск тем (POST /search/threads)
let threads = forum.search().threads(None).await?;

// Поиск постов (POST /search/posts)
let posts = forum.search().posts(None).await?;

// Поиск пользователей (POST /search/users)
let users = forum.search().users(None).await?;

// Поиск постов профиля (POST /search/profile-posts)
let pp = forum.search().profile_posts(None).await?;

// Поиск по тегам (POST /search/tagged)
let tagged = forum.search().tagged(None).await?;

// Результаты поиска (GET /search/{search_id})
let results = forum.search().results(1.into(), None).await?;
```

### Batch

```rust
// Выполнить batch-запрос (POST /batch)
let batch = forum.batch().execute(None).await?;
```

### Чатбокс / Chatbox

```rust
// Индекс чатбокса (GET /chatbox)
let index = forum.chatbox().index(None).await?;

// Получить сообщения (GET /chatbox/messages)
let msgs = forum.chatbox().get_messages(None).await?;

// Отправить сообщение (POST /chatbox/messages)
let msg = forum.chatbox().post_message(None).await?;

// Редактировать сообщение (PUT /chatbox/messages)
let edited = forum.chatbox().edit_message(None).await?;

// Удалить сообщение (DELETE /chatbox/messages)
let deleted = forum.chatbox().delete_message(None).await?;

// Онлайн в комнате (GET /chatbox/{room_id}/online)
let online = forum.chatbox().online(RoomId::default()).await?;

// Причины жалоб (GET /chatbox/report/reasons)
let reasons = forum.chatbox().report_reasons(None).await?;

// Пожаловаться (POST /chatbox/report)
let report = forum.chatbox().report(None).await?;

// Лидерборд (GET /chatbox/leaderboard)
let leaders = forum.chatbox().get_leaderboard(None).await?;

// Получить список игнора (GET /chatbox/ignore)
let ignore = forum.chatbox().get_ignore().await?;

// Добавить в игнор (POST /chatbox/ignore)
let added = forum.chatbox().post_ignore(None).await?;

// Убрать из игнора (DELETE /chatbox/ignore)
let removed = forum.chatbox().delete_ignore(None).await?;
```

### Формы / Forms

```rust
// Список форм (GET /forms)
let forms = forum.forms().list(None).await?;

// Создать форму (POST /forms/save)
let form = forum.forms().create(None).await?;
```

---

## Market API

### Категории / Category

```rust
// Все категории (GET /market)
let all = market.category().all(None).await?;

// Steam (GET /market/steam)
let steam = market.category().steam(None).await?;

// Fortnite (GET /market/fortnite)
let fortnite = market.category().fortnite(None).await?;

// Mihoyo (GET /market/mihoyo)
let mihoyo = market.category().mihoyo(None).await?;

// Riot (GET /market/riot)
let riot = market.category().riot(None).await?;

// Telegram (GET /market/telegram)
let telegram = market.category().telegram(None).await?;

// Supercell (GET /market/supercell)
let supercell = market.category().supercell(None).await?;

// EA (GET /market/ea)
let ea = market.category().ea(None).await?;

// WoT (GET /market/wot)
let wot = market.category().wot(None).await?;

// WoT Blitz (GET /market/wot-blitz)
let wot_blitz = market.category().wot_blitz(None).await?;

// Gifts (GET /market/gifts)
let gifts = market.category().gifts(None).await?;

// Epic Games (GET /market/epicgames)
let epic = market.category().epic_games(None).await?;

// Escape from Tarkov (GET /market/escape-from-tarkov)
let eft = market.category().escape_from_tarkov(None).await?;

// Social Club (GET /market/socialclub)
let sc = market.category().social_club(None).await?;

// Uplay (GET /market/uplay)
let uplay = market.category().uplay(None).await?;

// Discord (GET /market/discord)
let discord = market.category().discord(None).await?;

// TikTok (GET /market/tiktok)
let tiktok = market.category().tik_tok(None).await?;

// Instagram (GET /market/instagram)
let ig = market.category().instagram(None).await?;

// Battle.net (GET /market/battlenet)
let bnet = market.category().battle_net(None).await?;

// ChatGPT (GET /market/chatgpt)
let gpt = market.category().chat_gpt(None).await?;

// VPN (GET /market/vpn)
let vpn = market.category().vpn(None).await?;

// Roblox (GET /market/roblox)
let roblox = market.category().roblox(None).await?;

// Warface (GET /market/warface)
let warface = market.category().warface(None).await?;

// Minecraft (GET /market/minecraft)
let mc = market.category().minecraft(None).await?;

// Hytale (GET /market/hytale)
let hytale = market.category().hytale(None).await?;

// Список подкатегорий (GET /market/category)
let list = market.category().list(None).await?;

// Параметры категории (GET /market/{category_name}/params)
let params = market.category().params("steam".into()).await?;

// Игры категории (GET /market/{category_name}/games)
let games = market.category().games("steam".into()).await?;
```

### Список / List

```rust
// Аккаунты пользователя (GET /market/user)
let user = market.list().user(None).await?;

// Заказы (GET /market/user/orders)
let orders = market.list().orders(None).await?;

// Статусы (GET /market/user/states)
let states = market.list().states(None).await?;

// Скачать данные (GET /market/user/{type}/download)
let download = market.list().download("accounts".into(), None).await?;

// Избранное (GET /market/fave)
let faves = market.list().favorites(None).await?;

// Просмотренные (GET /market/viewed)
let viewed = market.list().viewed(None).await?;
```

### Управление / Managing

```rust
// Получить аккаунт (GET /market/{item_id})
let item = market.managing().get(123, None).await?;

// Удалить аккаунт (DELETE /market/{item_id})
let deleted = market.managing().delete(123, None).await?;

// Создать жалобу (POST /market/claims)
let claim = market.managing().create_claim(None).await?;

// Массовое получение (POST /market/bulk-get)
let bulk = market.managing().bulk_get(None).await?;

// Стоимость инвентаря Steam (GET /market/{item_id}/steam-inventory-value)
let inv = market.managing().steam_inventory_value(123, None).await?;

// Стоимость Steam (GET /market/steam-value)
let val = market.managing().steam_value(None).await?;

// Превью Steam (GET /market/{item_id}/steam-preview)
let preview = market.managing().steam_preview(123, None).await?;

// Редактировать аккаунт (PUT /market/{item_id}/edit)
let edited = market.managing().edit(123, None).await?;

// AI-оценка (GET /market/{item_id}/ai-price)
let price = market.managing().ai_price(123).await?;

// Цена автопокупки (GET /market/{item_id}/auto-buy-price)
let abp = market.managing().auto_buy_price(123).await?;

// Заметка (POST /market/{item_id}/note)
let note = market.managing().note(123, None).await?;

// Обновить стоимость Steam (PUT /market/{item_id}/steam-value)
let updated = market.managing().steam_update_value(123, None).await?;

// Поднять аккаунт (POST /market/{item_id}/bump)
let bumped = market.managing().bump(123).await?;

// Автоподнятие (POST /market/{item_id}/auto-bump)
let ab = market.managing().auto_bump(123, None).await?;

// Отключить автоподнятие (DELETE /market/{item_id}/auto-bump)
let disabled = market.managing().auto_bump_disable(123).await?;

// Открыть аккаунт (POST /market/{item_id}/open)
let opened = market.managing().open(123).await?;

// Закрыть аккаунт (POST /market/{item_id}/close)
let closed = market.managing().close(123).await?;

// Получить изображения (GET /market/{item_id}/image)
let img = market.managing().image(123, None).await?;

// Email код (GET /market/{item_id}/email-code)
let code = market.managing().email_code(123).await?;

// Получить письма (GET /market/letters)
let letters = market.managing().get_letters2(None).await?;

// Получить MA-файл Steam (GET /market/{item_id}/steam-mafile)
let mafile = market.managing().steam_get_mafile(123).await?;

// Добавить MA-файл Steam (POST /market/{item_id}/steam-mafile)
let added = market.managing().steam_add_mafile(123).await?;

// Удалить MA-файл Steam (DELETE /market/{item_id}/steam-mafile)
let removed = market.managing().steam_remove_mafile(123).await?;

// Код MA-файла Steam (GET /market/{item_id}/steam-mafile-code)
let code = market.managing().steam_mafile_code(123).await?;

// Steam Desktop Authenticator (POST /market/{item_id}/steam-sda)
let sda = market.managing().steam_sda(123, None).await?;

// Код Telegram (GET /market/{item_id}/telegram-code)
let code = market.managing().telegram_code(123).await?;

// Сброс авторизации Telegram (POST /market/{item_id}/telegram-reset-auth)
let reset = market.managing().telegram_reset_auth(123).await?;

// Отказ от гарантии (POST /market/{item_id}/refuse-guarantee)
let refused = market.managing().refuse_guarantee(123).await?;

// Отклонить видеозапись (POST /market/{item_id}/decline-video-recording)
let declined = market.managing().decline_video_recording(123, None).await?;

// Проверить гарантию (POST /market/{item_id}/check-guarantee)
let checked = market.managing().check_guarantee(123).await?;

// Сменить пароль (POST /market/{item_id}/change-password)
let changed = market.managing().change_password(123, None).await?;

// Временный пароль email (GET /market/{item_id}/temp-email-password)
let temp = market.managing().temp_email_password(123).await?;

// Установить тег (POST /market/{item_id}/tag)
let tagged = market.managing().tag(123, None).await?;

// Убрать тег (DELETE /market/{item_id}/tag)
let untagged = market.managing().untag(123, None).await?;

// Публичный тег (POST /market/{item_id}/public-tag)
let ptag = market.managing().public_tag(123, None).await?;

// Убрать публичный тег (DELETE /market/{item_id}/public-tag)
let uptag = market.managing().public_untag(123, None).await?;

// В избранное (POST /market/{item_id}/star)
let faved = market.managing().favorite(123).await?;

// Убрать из избранного (DELETE /market/{item_id}/star)
let unfaved = market.managing().unfavorite(123).await?;

// Закрепить (POST /market/{item_id}/stick)
let stuck = market.managing().stick(123).await?;

// Открепить (DELETE /market/{item_id}/stick)
let unstuck = market.managing().unstick(123).await?;

// Передать аккаунт (POST /market/{item_id}/transfer)
let transferred = market.managing().transfer(123, None).await?;
```

### Профиль / Profile

```rust
// Получить профиль (GET /market/me)
let profile = market.profile().get(None).await?;

// Редактировать профиль (PUT /market/me)
let edited = market.profile().edit(None).await?;

// Жалобы (GET /market/me/claims)
let claims = market.profile().claims(None).await?;
```

### Корзина / Cart

```rust
// Получить корзину (GET /market/cart)
let cart = market.cart().get(None).await?;

// Добавить в корзину (POST /market/cart)
let added = market.cart().add(None).await?;

// Удалить из корзины (DELETE /market/cart)
let deleted = market.cart().delete(None).await?;
```

### Покупка / Purchasing

```rust
// Быстрая покупка (POST /market/{item_id}/fast-buy)
let buy = market.purchasing().fast_buy(123, None).await?;

// Проверить аккаунт (POST /market/{item_id}/check-account)
let check = market.purchasing().check(123).await?;

// Подтвердить покупку (POST /market/{item_id}/confirm-buy)
let confirm = market.purchasing().confirm(123, None).await?;

// Запрос скидки (POST /market/{item_id}/discount-request)
let req = market.purchasing().discount_request(123, None).await?;

// Отменить запрос скидки (DELETE /market/{item_id}/discount-request)
let cancel = market.purchasing().discount_cancel(123).await?;
```

### Кастомные скидки / Custom Discounts

```rust
// Получить скидки (GET /market/custom-discounts)
let discounts = market.custom_discounts().get().await?;

// Создать скидку (POST /market/custom-discounts)
let created = market.custom_discounts().create(None).await?;

// Редактировать скидку (PUT /market/custom-discounts)
let edited = market.custom_discounts().edit(None).await?;

// Удалить скидку (DELETE /market/custom-discounts)
let deleted = market.custom_discounts().delete(None).await?;
```

### Публикация / Publishing

```rust
// Добавить аккаунт (POST /item/add)
let added = market.publishing().add(None).await?;

// Быстрая продажа (POST /item/fast-sell)
let sold = market.publishing().fast_sell(None).await?;

// Внешний аккаунт (POST /{item_id}/external-account)
let ext = market.publishing().external(123, None).await?;

// Проверить детали (POST /{item_id}/goods/check)
let check = market.publishing().check(123, None).await?;
```

### Платежи / Payments

```rust
// Получить инвойс (GET /market/payments/invoice)
let invoice = market.payments().invoice_get(None).await?;

// Создать инвойс (POST /market/payments/invoice)
let created = market.payments().invoice_create(None).await?;

// Список инвойсов (GET /market/payments/invoices)
let list = market.payments().invoice_list(None).await?;

// Валюты (GET /market/payments/currency)
let currency = market.payments().currency().await?;

// Список балансов (GET /market/payments/balance)
let balance = market.payments().balance_list().await?;

// Обмен валют (POST /market/payments/balance/exchange)
let exchange = market.payments().balance_exchange(None).await?;

// Перевод средств (POST /market/payments/transfer)
let transfer = market.payments().transfer(None).await?;

// Комиссия (GET /market/payments/fee)
let fee = market.payments().fee(None).await?;

// Отмена платежа (POST /market/payments/cancel)
let cancel = market.payments().cancel(None).await?;

// История платежей (GET /market/payments/history)
let history = market.payments().history(None).await?;

// Сервисы выплат (GET /market/payments/payout/services)
let services = market.payments().payout_services().await?;

// Выплата (POST /market/payments/payout)
let payout = market.payments().payout(None).await?;
```

### Автоплатежи / Auto Payments

```rust
// Список автоплатежей (GET /market/auto-payments)
let list = market.auto_payments().list().await?;

// Создать автоплатёж (POST /market/auto-payments)
let created = market.auto_payments().create(None).await?;

// Удалить автоплатёж (DELETE /market/auto-payments)
let deleted = market.auto_payments().delete(None).await?;
```

### Прокси / Proxy

```rust
// Получить прокси (GET /market/proxy)
let proxy = market.proxy().get().await?;

// Добавить прокси (POST /market/proxy)
let added = market.proxy().add(None).await?;

// Удалить прокси (DELETE /market/proxy)
let deleted = market.proxy().delete(None).await?;
```

### IMAP

```rust
// Создать IMAP (POST /market/imap)
let created = market.imap().create(None).await?;

// Удалить IMAP (DELETE /market/imap)
let deleted = market.imap().delete(None).await?;
```

### Batch

```rust
// Batch-запрос (POST /market/batch)
let batch = market.batch().batch(None).await?;
```

---

## Генерация кода / Code Generation

Клиенты и типы генерируются из OpenAPI 3.1.0 спецификаций в `schemas/`:

```bash
cargo run -p codegen
```

| Вход | Выход |
|------|-------|
| `schemas/forum.json` | `lolzteam/src/generated/forum/client.rs`, `types.rs` |
| `schemas/market.json` | `lolzteam/src/generated/market/client.rs`, `types.rs` |

Исходный код генератора в `codegen/`.

---

## Сборка и тесты / Build & Test

```bash
cargo build             # Собрать все крейты
cargo test              # Запустить тесты
cargo clippy            # Линтер
cargo fmt               # Форматирование
cargo fmt --check       # Проверка форматирования
cargo run -p codegen    # Генерация клиентов из OpenAPI спецификаций
```

---

## Структура проекта / Project Structure

```
schemas/                        OpenAPI 3.1.0 спецификации
codegen/                        Крейт генератора кода
lolzteam/                       Библиотечный крейт
  src/
    runtime/
      http_client.rs            HTTP клиент (auth, rate limit, retry, proxy)
      retry.rs                  Экспоненциальный backoff с джиттером
      rate_limiter.rs           Token bucket rate limiter
      errors.rs                 Типы ошибок
      types.rs                  Конфигурационные структуры
    generated/
      forum/
        client.rs               ForumClient (18 API групп, 151 метод)
        types.rs                Типы запросов/ответов
      market/
        client.rs               MarketClient (13 API групп, 115 методов)
        types.rs                Типы запросов/ответов
    lib.rs                      Публичные реэкспорты
  Cargo.toml
Cargo.toml                      Workspace root
```

---

## Лицензия / License

[MIT](LICENSE)
