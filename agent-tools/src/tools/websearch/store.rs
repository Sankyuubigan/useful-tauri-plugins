//! Общие пути хранения websearch-тула: куда пишутся search_cache.json и
//! search_stats.json (аналог `Deno.env.get("KING_ORCH_BINS_DIR") || os.tmpdir()`).
//!
//! Осознанное отличие (решение PRP §3.5): без env директория —
//! `%APPDATA%\agent-tools` (Windows), а не tmpdir напрямую: tmp-директории
//! чистятся системой, а кеш/статистика должны переживать перезагрузку.
//! Наличие env — приоритет 1 (туда в King Orch пишут бины).
//!
//! Функции `_from(dir)` (у cache.rs / stats.rs) принимают директорию явно —
//! так юнит-тесты работают в tempfs без глобального env.

use std::path::PathBuf;

pub(crate) fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("KING_ORCH_BINS_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    if let Some(appdata) = std::env::var_os("APPDATA").filter(|v| !v.is_empty()) {
        let mut dir = PathBuf::from(appdata);
        dir.push("agent-tools");
        return dir;
    }
    std::env::temp_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_beats_appdata_and_defaults_are_sane() {
        // env задан (когда есть) — его директория; иначе хотя бы не tmpdir при APPDATA.
        if std::env::var_os("KING_ORCH_BINS_DIR").is_some_and(|v| !v.is_empty()) {
            assert_eq!(data_dir(), PathBuf::from(std::env::var_os("KING_ORCH_BINS_DIR").unwrap()));
        } else if std::env::var_os("APPDATA").is_some_and(|v| !v.is_empty()) {
            assert!(data_dir().ends_with("agent-tools"));
        } else {
            assert_eq!(data_dir(), std::env::temp_dir());
        }
    }
}
