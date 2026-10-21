//! Диск-кеш результатов поиска — порт `search_cache.ts` (King Orch).
//!
//! Файл `search_cache.json` в `store::data_dir()`; ключ
//! `scope::(p|c):engines.join(',')::query`; TTL 30 мин; prune старых записей
//! при превышении MAX_ENTRIES=300 (LRU по ts).
//!
//! Осознанные отличия: нет in-memory кеша TS (см. шапку stats.rs) и JSON
//! компактный (TS — такой же компактный `JSON.stringify(data)`).

use std::path::Path;

use serde_json::{Value, json};

use super::store::data_dir;

const CACHE_FILE: &str = "search_cache.json";
const CACHE_TTL_MS: u64 = 30 * 60 * 1000;
const MAX_ENTRIES: usize = 300;

static WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Тестовый крюк фиксированного «сейчас» (для TTL/прун-тестов).
static TEST_NOW: std::sync::Mutex<Option<u64>> = std::sync::Mutex::new(None);

fn now_ms() -> u64 {
    if let Ok(guard) = TEST_NOW.lock() {
        if let Some(v) = *guard {
            return v;
        }
    }
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn cache_path(dir: &Path) -> std::path::PathBuf {
    let mut p = dir.to_path_buf();
    p.push(CACHE_FILE);
    p
}

fn load_from(dir: &Path) -> Value {
    let path = cache_path(dir);
    if let Ok(Some(raw)) = ko_json_store::read_text(&path) {
        if let Ok(v) = serde_json::from_str::<Value>(&raw) {
            if v.is_object() {
                return v;
            }
        }
    }
    json!({})
}

fn save_to(dir: &Path, data: &Value) {
    let _ = std::fs::create_dir_all(dir);
    let content = serde_json::to_string(data).unwrap_or_else(|_| "{}".to_string());
    let _ = ko_json_store::write_atomic(&cache_path(dir), &content);
}

/// Ключ кеша: scope + конфиг вызова + query — порт `cacheKey`.
pub fn cache_key(scope: &str, engines: &[String], parallel: bool, query: &str) -> String {
    let cfg = if parallel { "p:" } else { "c:" }.to_string() + &engines.join(",");
    format!("{scope}::{cfg}::{query}")
}

/// Кешированный текст, если TTL не истёк — порт `cacheGet`.
pub fn cache_get(key: &str) -> Option<String> {
    let _guard = WRITE_LOCK.lock();
    let data = load_from(&data_dir());
    let e = data.get(key)?;
    let ts = e.get("ts").and_then(|v| v.as_u64())?;
    if now_ms().saturating_sub(ts) > CACHE_TTL_MS {
        return None;
    }
    e.get("text").and_then(|v| v.as_str()).map(str::to_string)
}

/// Сохранить текст ответа — порт `cachePut` (prune убирает лишнее при save).
pub fn cache_put(key: &str, text: &str) {
    let _guard = WRITE_LOCK.lock();
    let dir = data_dir();
    let mut data = load_from(&dir);
    if let Some(map) = data.as_object_mut() {
        map.insert(key.to_string(), json!({ "ts": now_ms(), "text": text }));
    }
    prune_over_limit(&mut data);
    save_to(&dir, &data);
}

fn prune_over_limit(data: &mut Value) {
    let map = match data.as_object_mut() {
        Some(m) => m,
        None => return,
    };
    if map.len() <= MAX_ENTRIES {
        return;
    }
    let mut entries: Vec<(String, u64)> = map
        .iter()
        .map(|(k, v)| (k.clone(), v.get("ts").and_then(|t| t.as_u64()).unwrap_or(0)))
        .collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1));
    let keep: std::collections::HashSet<String> =
        entries.into_iter().take(MAX_ENTRIES).map(|(k, _)| k).collect();
    let stale: Vec<String> = map.keys().filter(|k| !keep.contains(*k)).cloned().collect();
    for k in stale {
        map.remove(&k);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Тестовый крюк TEST_NOW — общий static для модуля; юнит-тесты cargo
    // исполняются параллельно, поэтому все тесты, меняющие TEST_NOW,
    // обязаны держать этот замок, иначе now(ts) записи ломается между
    // шагами теста (несинхронный тест ловил Some при ожидании None).
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn temp_dir() -> std::path::PathBuf {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!(
            "agent-tools-cache-{}-{}",
            std::process::id(),
            seq
        ));
        let _ = std::fs::create_dir_all(&d);
        d
    }

    #[test]
    fn key_matches_ts_scope_mode_engines_query() {
        assert_eq!(
            cache_key("web", &["duckduckgo".to_string(), "exa".to_string()], false, "запрос"),
            "web::c:duckduckgo,exa::запрос"
        );
        assert_eq!(
            cache_key("web", &["exa".to_string()], true, "q"),
            "web::p:exa::q"
        );
    }

    #[test]
    fn put_get_and_ttl_expiry() {
        let _guard = TEST_LOCK.lock().expect("serial lock");
        let dir = temp_dir();
        let _ = std::fs::remove_file(dir.join(CACHE_FILE));
        *TEST_NOW.lock().expect("крюк") = Some(1000);
        cache_put_into(&dir, "k1", "текст ответа");
        *TEST_NOW.lock().expect("крюк") = Some(1000 + CACHE_TTL_MS);
        assert_eq!(cache_get_from(&dir, "k1"), Some("текст ответа".to_string()));
        // Через минуту больше TTL — просрочено.
        *TEST_NOW.lock().expect("крюк") = Some(1000 + CACHE_TTL_MS + 60_000);
        assert_eq!(cache_get_from(&dir, "k1"), None);
        *TEST_NOW.lock().expect("крюк") = None;
    }

    #[test]
    fn prune_keeps_newest_300() {
        let _guard = TEST_LOCK.lock().expect("serial lock");
        let dir = temp_dir();
        let _ = std::fs::remove_file(dir.join(CACHE_FILE));
        let engines = vec!["exa".to_string()];
        for i in 0..308 {
            *TEST_NOW.lock().expect("крюк") = Some(1000 + i as u64);
            cache_put_into(&dir, &cache_key("web", &engines, false, &format!("q{i}")), &format!("t{i}"));
        }
        *TEST_NOW.lock().expect("крюк") = None;
        let data = load_from(&dir);
        let map = data.as_object().expect("объект");
        assert_eq!(map.len(), 300);
        // Самые старые (q0..q7) отброшены, остальные живы.
        assert!(map.contains_key("web::c:exa::q15"));
        assert!(!map.contains_key("web::c:exa::q0"));
    }

    // ── Обёртки с явной директорией (как доступ продакшен-кодa через data_dir) ──
    fn cache_put_into(dir: &Path, key: &str, text: &str) {
        let mut data = load_from(dir);
        if let Some(map) = data.as_object_mut() {
            map.insert(key.to_string(), json!({ "ts": now_ms(), "text": text }));
        }
        prune_over_limit(&mut data);
        save_to(dir, &data);
    }

    fn cache_get_from(dir: &Path, key: &str) -> Option<String> {
        let data = load_from(dir);
        let e = data.get(key)?;
        let ts = e.get("ts").and_then(|v| v.as_u64())?;
        if now_ms().saturating_sub(ts) > CACHE_TTL_MS {
            return None;
        }
        e.get("text").and_then(|v| v.as_str()).map(str::to_string)
    }
}
