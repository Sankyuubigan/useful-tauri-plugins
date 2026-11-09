//! Общие пути хранения websearch-тула: куда пишутся search_cache.json и
//! search_stats.json.
//!
//! Цепочка (решение PRP-2 §3): env `AGENT_TOOLS_DATA_DIR` (приоритет 1,
//! тест-крюк и точка интеграции для хоста) → `%APPDATA%\agent-tools`
//! (tmp чистится системой, а кеш/статистика должны переживать перезагрузку)
//! → системный tmpdir (крайний фоллбэк).
//!
//! Функции `_from(dir)` (у cache.rs / stats.rs) принимают директорию явно —
//! так юнит-тесты работают в tempfs без глобального env, а будущий lib-крейт
//! (Фаза 2, крейт `web-tools`) получит data_dir как параметр API.

use std::path::PathBuf;

pub(crate) fn data_dir() -> PathBuf {
    if let Some(dir) = env_dir("AGENT_TOOLS_DATA_DIR") {
        return dir;
    }
    if let Some(appdata) = env_dir("APPDATA") {
        let mut dir = PathBuf::from(appdata);
        dir.push("agent-tools");
        return dir;
    }
    std::env::temp_dir()
}

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_beats_appdata_and_defaults_are_sane() {
        // env задан (когда есть) — его директория; иначе хотя бы не tmpdir при APPDATA.
        if let Some(d) = env_dir("AGENT_TOOLS_DATA_DIR") {
            assert_eq!(data_dir(), d);
        } else if env_dir("APPDATA").is_some() {
            assert!(data_dir().ends_with("agent-tools"));
        } else {
            assert_eq!(data_dir(), std::env::temp_dir());
        }
    }
}
