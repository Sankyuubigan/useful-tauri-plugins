//! Tauri-команды плагина.

use serde_json::Value;
use tauri::command;

use crate::error_reporting::JsErrorReport;

/// Разрешённый путь dev-зеркала `test/last_logs.txt` (или пустая строка,
/// если dev-комплект не найден). Показывается во вкладке «Логи».
#[command]
pub(crate) fn get_last_logs_path() -> String {
    crate::logger::last_logs_path()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// Путь exe-файла лога (`king_orch.log`) или null.
#[command]
pub(crate) fn get_log_file_path() -> Option<String> {
    crate::logger::log_file_path().map(|p| p.to_string_lossy().to_string())
}

/// Запись строки лога из frontend через единый логгер.
/// `level` — любая короткая метка (FE, WARN, ERROR…).
#[command]
pub(crate) fn log_frontend_event(level: String, msg: String) {
    crate::logger::write_regular(&level, &msg);
}

/// Аналитическое событие (проверяет флаги reporting + events).
#[command]
pub(crate) fn track_event(name: String, props: Option<Value>) -> Result<(), String> {
    if let Some(p) = &props {
        if !p.is_object() {
            return Err("props must be a JSON object".to_string());
        }
    }
    crate::error_reporting::track_event(&name, props);
    Ok(())
}

/// Ошибка с фронта (window.onerror / unhandledrejection и т.п.).
#[command]
pub(crate) fn track_error(
    error_type: String,
    message: String,
    stack: Option<String>,
    severity: Option<String>,
    kind: Option<String>,
    breadcrumbs: Option<Vec<String>>,
) {
    crate::error_reporting::report_js(JsErrorReport {
        error_type,
        message,
        stack,
        severity: severity.unwrap_or_default(),
        kind: kind.unwrap_or_default(),
        breadcrumbs,
    });
}

/// Необработанная ошибка фронта: сброс контекста (breadcrumbs) в постоянный
/// `crash_dump.log` + запись в лог сессии + облачный отчёт (если включено).
#[command]
pub(crate) fn dump_frontend_error(
    error_type: String,
    message: String,
    stack: Option<String>,
    breadcrumbs: Option<Vec<String>>,
) {
    crate::logger::write_regular("FE-CRASH", &message);
    let crumbs = breadcrumbs.unwrap_or_default();
    crate::flight::dump_frontend(&error_type, &message, &crumbs);
    crate::error_reporting::report_js(JsErrorReport {
        error_type,
        message,
        stack,
        severity: "fatal".to_string(),
        kind: "unhandled".to_string(),
        breadcrumbs: Some(crumbs),
    });
}

/// Рантайм-переключатель облачной отправки (флаг `allow_error_reports` хоста —
/// единственный источник правды, плагин лишь исполняет).
#[command]
pub(crate) fn set_reporting_enabled(enabled: bool) {
    crate::error_reporting::set_enabled(enabled);
}

/// Запись текста лога в файл, который выбрал юзер в диалоге сохранения.
#[command]
pub(crate) fn save_logs_file(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, content).map_err(|e| format!("не удалось сохранить логи: {e}"))
}