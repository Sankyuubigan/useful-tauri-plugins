//! Телеметрия плагина.
//!
//! Плагин не обязателен к связке с tauri-plugin-logs, поэтому кидает аналитику
//! через маленький шим: если хост включил feature `host-logs` — события уходят в
//! `tauri_plugin_logs::track_event` (единый стандарт логирования King Orch
//! `core/rules.md` §2.5); иначе — просто в `log::info!`.

/// Аналитическое событие (например `llm_started` / `llm_finished` / `llm_error`).
pub fn track_event(name: &str, props: Option<serde_json::Value>) {
    #[cfg(feature = "host-logs")]
    {
        tauri_plugin_logs::track_event(name, props);
    }
    #[cfg(not(feature = "host-logs"))]
    {
        let _ = props;
        log::info!("[EVENT] {}", name);
    }
}