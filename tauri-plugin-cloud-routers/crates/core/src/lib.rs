//! `cloud-routers-core` — доменный слой шлюза `cloud-routers`.
//!
//! Крейт существует по одной причине: **разделить «знание о конфигурации» от
//! «исполнения прокси»**. Tauri-плагину (`tauri-plugin-cloud-routers`) нужны типы
//! конфигурации и умение прочитать список комбо, чтобы отвечать на `get_combos`
//! **без поднятия HTTP-сервера**. Серверному бинарю (`cloud-routers-gateway`)
//! нужны те же типы плюс HTTP-клиент к апстримам.
//!
//! Если смешать их в одном крейте, `axum`/`reqwest` попадут в бинарь хоста через
//! path-зависимость плагина. Поэтому здесь **нет ни axum, ни reqwest, ни tauri** —
//! только `serde` + атомарная запись JSON. Это проверено `cargo tree` хоста.
//!
//! ## Разделение состояния
//!
//! | Слой | Где живёт | Что содержит |
//! |---|---|---|
//! | Конфигурация | `<data_dir>/gateway.json` | провайдеры, ключи, модели, релей |
//! | Рантайм | память процесса | round-robin, кулдауны ключей |
//!
//! Кулдаун **никогда** не попадает на диск (тип `Instant` не сериализуется, и
//! переживать перезапуск ему не нужно). См. [`keys`].
//!
//! ## Формат комбо
//!
//! Комбо шлюза — это пара «провайдер + конкретная модель», а не пользовательская
//! связка с fallback-цепочкой (как у 9Router). Идентификатор детерминирован:
//! `"{provider_id}/{model_id}"`, например `groq/llama-3.3-70b-versatile`.
//! Детерминизм здесь принципиален: один и тот же id всегда означает один и тот же
//! апстрим, поэтому маршрутизация не зависит от скрытого состояния.

pub mod config;
pub mod keys;
pub mod models;
pub mod presets;
pub mod relay_token;
pub mod store;

pub use config::{AccountKey, DiscoveredModel, GatewayConfig, ProviderConfig};
pub use keys::{FailureKind, KeyRotation};
pub use models::{combo_id, resolve_combo};
pub use presets::{ProviderPreset, ProviderPresetsCatalog};
pub use relay_token::{generate_relay_token, is_usable_token, TokenError};
pub use store::{ServerRecord, load as load_config, save as save_config};

/// Версия крейта (совпадает с версией бинаря шлюза).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");