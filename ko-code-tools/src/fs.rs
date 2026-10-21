//! Файловые тулы ядра: чтение, пакетное чтение, запись, точечная правка, хук
//! проверки качества.
//!
//! Перенос из King Orch `src-tauri/src/infra/tools/fs.rs` (SSOT теперь здесь).
//! Чтение — любой путь (без подтверждения). Запись — авторизуется хуком
//! [`ToolCtx::authorize_write`]; политика (авто/плашка/запрет) — на стороне хоста.
//!
//! Файл — фасад: реализации разнесены по назначению (см. подмодули
//! `fs_read` / `fs_write` / `fs_edit` / `fs_lint`), а имена тулов остаются
//! прежними: [`ReadFile`], [`ReadManyFiles`], [`WriteFile`], [`EditFile`],
//! [`LINT_CONTEXT_LINES`].

mod fs_edit;
mod fs_lint;
mod fs_read;
mod fs_write;

pub use fs_edit::EditFile;
pub use fs_lint::LINT_CONTEXT_LINES;
pub use fs_read::{ReadFile, ReadManyFiles};
pub use fs_write::WriteFile;

use serde_json::Value;

use crate::contract::ToolError;

fn arg_str(args: &Value, key: &str) -> Result<String, ToolError> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| ToolError::Usage(format!("параметр '{}' (строка) обязателен", key)))
}

fn arg_int(args: &Value, key: &str, default: i64) -> i64 {
    args.get(key).and_then(|v| v.as_i64()).unwrap_or(default)
}
