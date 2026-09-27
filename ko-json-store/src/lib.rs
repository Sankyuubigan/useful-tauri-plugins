//! Атомарные записи JSON в `app_data_dir` / `sessions` (единый для хоста и плагинов).
//!
//! Два примитива:
//! - [`write_atomic`] — tmp в той же папке → `sync_all` → `rename` → удаление tmp при ошибке.
//!   Гарантирует, что читатель никогда не видит битый/частичный файл.
//! - [`update_json`] — read-modify-write с эксклюзивной блокировкой на sidecar
//!   `<file>.lock` (fs2). Сам JSON файл **не** лочится — читатели не блокируются.
//!
//! Правило проекта: записи в `app_data_dir`/`sessions` — **только** через этот крейт.
//! Запрещено: `fs::write` напрямую и `fs::read_to_string` + `unwrap_or_default()` +
//! `fs::write` (lost update + рваный файл).

use fs2::FileExt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// RAII- guard эксклюзивной блокировки sidecar-файла.
struct LockGuard(File);

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

/// Атомарная запись строки в `path`: tmp → sync_all → rename → удаление tmp при ошибке.
pub fn write_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("data.json");
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = dir.join(format!("{}.{}.{}.tmp", file_name, std::process::id(), nanos));

    let write_res = (|| -> std::io::Result<()> {
        let mut f = File::create(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
        fs::rename(&tmp, path)
    })();

    if write_res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    write_res
}

/// Read-modify-write JSON-файла с эксклюзивной блокировкой на sidecar `<file>.lock`.
///
/// Сам JSON файл не лочится (читатели не блокируются) — атомарность чтения-записи
/// обеспечивается sidecar-локом, а целимость файла — через [`write_atomic`].
/// При ошибке парсинга существующего файла берётся `T::default()` + `log::warn!`
/// (не роняем файл). Если файла нет — стартуем с `T::default()`.
pub fn update_json<T>(path: &Path, update_fn: impl FnOnce(&mut T)) -> Result<(), String>
where
    T: serde::Serialize + serde::de::DeserializeOwned + Default,
{
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("ko-json-store: некорректный путь: {}", path.display()))?;
    let lock_path = path.with_file_name(format!("{}.lock", file_name));

    let lock_file = OpenOptions::new()
        .create(true)
        .write(true)
        .open(&lock_path)
        .map_err(|e| format!("ko-json-store: не удалось открыть lock {}: {}", lock_path.display(), e))?;

    lock_file
        .lock_exclusive()
        .map_err(|e| format!("ko-json-store: не удалось захватить lock {}: {}", lock_path.display(), e))?;
    let _guard = LockGuard(lock_file);

    let mut value: T = match fs::read_to_string(path) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(v) => v,
            Err(e) => {
                log::warn!(
                    "ko-json-store: ошибка парсинга {}: {}, используем default",
                    path.display(),
                    e
                );
                T::default()
            }
        },
        Err(_) => T::default(),
    };

    update_fn(&mut value);

    let content = serde_json::to_string_pretty(&value)
        .map_err(|e| format!("ko-json-store: ошибка сериализации {}: {}", path.display(), e))?;

    write_atomic(path, &content)
        .map_err(|e| format!("ko-json-store: ошибка записи {}: {}", path.display(), e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("ko_json_store_{}_{}", tag, nanos));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn write_atomic_creates_file_and_no_tmp_left() {
        let dir = tmp_dir("write_ok");
        let path = dir.join("data.json");
        write_atomic(&path, r#"{"a":1}"#).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"a":1}"#);
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_atomic_cleans_tmp_on_error() {
        let dir = tmp_dir("write_err");
        // rename файла на существующую директорию падает — tmp должен быть удалён
        let target_dir = dir.join("target_dir");
        fs::create_dir_all(&target_dir).unwrap();

        let res = write_atomic(&target_dir, r#"{"a":1}"#);
        assert!(res.is_err());

        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reader_never_sees_invalid_json() {
        let dir = tmp_dir("invalid");
        let path = dir.join("data.json");
        fs::write(&path, r#"{"valid":true}"#).unwrap();

        let missing = dir.join("nonexistent").join("data.json");
        let _ = write_atomic(&missing, "garbage");

        assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"valid":true}"#);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn update_json_merges_without_losing_existing_keys() {
        let dir = tmp_dir("merge");
        let path = dir.join("app_config.json");
        fs::write(&path, r#"{"host_key":"host_value"}"#).unwrap();

        update_json(&path, |root: &mut serde_json::Value| {
            root.as_object_mut()
                .unwrap()
                .insert("plugin_key".into(), serde_json::json!("plugin_value"));
        })
        .unwrap();

        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(val["host_key"], "host_value");
        assert_eq!(val["plugin_key"], "plugin_value");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn update_json_concurrent_no_lost_updates() {
        let dir = tmp_dir("concurrent");
        let path = Arc::new(dir.join("counter.json"));
        fs::write(&*path, r#"{}"#).unwrap();

        let errors = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();
        for t in 0..8u32 {
            let p = Arc::clone(&path);
            let errs = Arc::clone(&errors);
            handles.push(std::thread::spawn(move || {
                for i in 0..100u32 {
                    let key = format!("t{}_{}", t, i);
                    let res = update_json(&p, |map: &mut BTreeMap<String, u32>| {
                        map.insert(key.clone(), i);
                    });
                    if res.is_err() {
                        errs.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(errors.load(Ordering::SeqCst), 0);
        let map: BTreeMap<String, u32> =
            serde_json::from_str(&fs::read_to_string(&*path).unwrap()).unwrap();
        assert_eq!(map.len(), 800);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn update_json_recovers_from_corrupt_file() {
        let dir = tmp_dir("corrupt");
        let path = dir.join("app_config.json");
        fs::write(&path, r#"{"unclosed":"#).unwrap();

        update_json(&path, |val: &mut BTreeMap<String, serde_json::Value>| {
            val.insert("fresh".into(), serde_json::json!(1));
        })
        .unwrap();

        let val: BTreeMap<String, serde_json::Value> =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(val.get("fresh").unwrap(), &serde_json::json!(1));
        assert!(!val.contains_key("unclosed"));
        let _ = fs::remove_dir_all(&dir);
    }
}
