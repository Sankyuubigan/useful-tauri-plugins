//! SSOT-ядро мульти-движкового веб-поиска: 11 универсальных + 6 адресных
//! движков, цепочка фолбэков / параллельный режим, multi-query с дедупом,
//! диск-кеш и статистика сбоев.
//!
//! Ядро ничего не знает о потребителе: директория данных (кеш + статистика) —
//! параметр API ([`SearchOpts::data_dir`]), лог движков — stderr, env-переменных
//! в ядре нет. Потребители:
//! - CLI/MCP-обёртка `agent-tools` (маппинг [`SearchError`] на коды выхода);
//! - нативные тула хост-приложений — дверь [`search_blocking`].
//!
//! Единый обработчик — [`search`]: сюда приходят аргументы от всех
//! потребителей, логика одна (порт `handlers.WebSearch`).

pub mod cache;
pub mod engines;
pub mod http;
pub mod search;
pub mod stats;

pub use search::{
    SearchError, SearchOpts, parse_limit, resolve_engines, runtime, search, search_blocking,
};
