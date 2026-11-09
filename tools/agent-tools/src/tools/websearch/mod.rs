//! Тул `websearch` — тонкая CLI/MCP-обёртка над SSOT-ядром поиска `web-tools`.
//!
//! Ядро (движки, HTTP, кеш, статистика, единый обработчик `search`) живёт в
//! крейте `web-tools`; здесь остаются: парсер CLI (`cli.rs`), MCP-режим
//! (`mcp.rs`), выбор директории данных (`store.rs`, env-цепочка) и маппинг
//! [`web_tools::SearchError`] на коды выхода.
//!
//! Режимы вызова (см. `cli.rs`):
//!   websearch <запрос>       — одиночный поиск,
//!   websearch mcp            — MCP-сервер (tools/list + tools/call WebSearch),
//!   websearch --stats        — сводка сбоев движков.
//!
//! Код возврата: 0 успех, 2 ошибка ввода (нет query, битый --engines), 3 сети
//! (все движки не дали результатов). Причины движков — в stderr.
//!
//! Тестовая вёрстка: сетевые юнит-тесты запрещены (гейт Г4 — весь пакет
//! зелёный без сети), поэтому ядро тестируется чистыми функциями в
//! `web-tools/src/search.rs`, а реальный вызов движков проверен
//! стаб-сквозняками `tests/websearch_e2e.rs` (гейт Г3).

use crate::exit::Exit;

pub(crate) mod cli;
pub(crate) mod mcp;
pub(crate) mod store;

/// Точка входа реестра тула (вызывается из tools/mod.rs). Полный `argv`
/// включает бинарь (argv[0]) и имя инструмента (argv[1]) — они не относятся
/// к парсеру CLI (регрессия codemap/cli.rs:424 — здесь та же).
pub(crate) fn run(args: Vec<String>) -> Exit {
    cli::run_to_exit(args.get(2..).unwrap_or(&[]))
}
