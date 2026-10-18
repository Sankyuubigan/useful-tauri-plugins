//! Атомарные записи JSON в `app_data_dir` / `sessions` (единый для хоста и плагинов).
//!
//! Три примитива записи:
//! - [`write_atomic`] — фиксация значения в журнале → tmp в той же папке →
//!   `sync_all` → `rename` → удаление tmp и журнала. Читатель никогда не видит
//!   битый/частичный файл, а обрыв записи не уничтожает значение.
//! - [`update_json`] — read-modify-write с эксклюзивной блокировкой на sidecar
//!   `<file>.lock` (fs2). Сам JSON файл **не** лочится — читатели не блокируются.
//!   Лок-файл — артефакт операции: после успешной записи он удаляется
//!   (SQLite-паттерн служебных файлов), хвосты от обрыва процесса убирает
//!   [`sweep_stale_locks`]. Занятость определяется захватом fs2-блокировки,
//!   НЕ наличием файла.
//! - [`roots::choose`] — выбор каталога записи настоящей пробой, один раз на метку.
//!
//! Доступность (не «атомарность при успехе», а гарантия самой попытки):
//! - [`roots`] — **где** писать: лестница кандидатов с настоящей пробой, решение
//!   принимается один раз на метку и общее для хоста и всех плагинов;
//! - [`journal`] — **что** уже зафиксировано, но не дошло до файла;
//! - [`health`] — счётчики и последняя причина отказа, каждая неудачная запись
//!   пишется в лог с путём и `io::ErrorKind`;
//! - блокировки **никогда не висят**: внутрипроцессная — обычный `Mutex` с
//!   исключённой реентрантностью, межпроцессная — `try_lock` с ограниченным
//!   повтором и fail-open с предупреждением. Раньше здесь стоял блокирующий
//!   `LockFileEx` без таймаута: удержание лока третьим процессом замораживало
//!   приложение навсегда. Файл лока после успеха удаляется: пользователь не
//!   видит служебного мусора рядом с данными; отказ удаления (чужой открытый
//!   handle) безвреден и подбирается [`sweep_stale_locks`].
//!
//! Правило проекта: записи в `app_data_dir`/`sessions` — **только** через этот
//! крейт, и читаются тоже только через него ([`read_text`], [`read_json`]) —
//! чтение в обход журнала не видит незакрытую запись.
//! Запрещено: `fs::write` напрямую и `fs::read_to_string` + `unwrap_or_default()` +
//! `fs::write` (lost update + рваный файл).

pub mod journal;
pub mod roots;

use fs2::FileExt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Сколько ждём межпроцессный лок, прежде чем признать его недоступным.
const LOCK_WAIT_BUDGET: Duration = Duration::from_millis(150);
const LOCK_POLL_INTERVAL: Duration = Duration::from_millis(25);

/// Счётчики записей и последняя причина отказа — источник для `get_storage_health`.
static SUCCESS_WRITES: AtomicU64 = AtomicU64::new(0);
static FAILED_WRITES: AtomicU64 = AtomicU64::new(0);
static LAST_ERROR: OnceLock<Mutex<Option<String>>> = OnceLock::new();

/// Снимок состояния хранилища для UI и диагностики.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct StorageHealth {
    pub successful_writes: u64,
    pub failed_writes: u64,
    pub last_error: Option<String>,
}

/// Хранилище последней причины отказа. Инициализируется при первом обращении —
/// `OnceLock::get()` без `get_or_init` всегда вернул бы `None`.
fn last_error_slot() -> &'static Mutex<Option<String>> {
    LAST_ERROR.get_or_init(|| Mutex::new(None))
}

/// Текущее состояние счётчиков. Не бросает: вызывается в том числе из
/// обработчиков ошибок, где падение самого отчёта недопустимо.
pub fn health() -> StorageHealth {
    let last_error = last_error_slot()
        .lock()
        .ok()
        .and_then(|guard| guard.clone());
    StorageHealth {
        successful_writes: SUCCESS_WRITES.load(Ordering::Relaxed),
        failed_writes: FAILED_WRITES.load(Ordering::Relaxed),
        last_error,
    }
}

/// Единая точка фиксации отказа: лог (путь + этап + причина) + счётчики для UI.
fn record_failure(path: &Path, stage: &str, err: &dyn std::fmt::Display) {
    FAILED_WRITES.fetch_add(1, Ordering::Relaxed);
    let detail = format!("ko-json-store: {} [{}]: {}", stage, path.display(), err);
    log::error!("{}", detail);
    if let Ok(mut guard) = last_error_slot().lock() {
        *guard = Some(detail);
    }
}

// ───────────────────────────── сериализация обновлений ─────────────────────────────

/// Один writer на процесс: без этого два потока теряют правки друг друга.
/// Критическая секция короткая и не ждёт внешних ресурсов, поэтому взаимного
/// ожидания здесь не возникает.
static WRITE_SERIAL: OnceLock<Mutex<()>> = OnceLock::new();

thread_local! {
    /// Глубина вложенности `update_json` в текущем потоке. Вложенный вызов
    /// не берёт блокировку повторно — иначе он ждал бы сам себя до вечности.
    static UPDATE_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

enum ProcessLock<'a> {
    /// Guard держится ради побочного эффекта `Drop` — само поле не читается.
    #[allow(dead_code)]
    Held(MutexGuard<'a, ()>),
    Nested,
}

impl Drop for ProcessLock<'_> {
    fn drop(&mut self) {
        if matches!(self, ProcessLock::Held(_)) {
            UPDATE_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
        }
    }
}

fn acquire_in_process() -> ProcessLock<'static> {
    if UPDATE_DEPTH.with(|depth| depth.get()) > 0 {
        return ProcessLock::Nested;
    }
    let slot = WRITE_SERIAL.get_or_init(|| Mutex::new(()));
    let guard = match slot.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    UPDATE_DEPTH.with(|depth| depth.set(depth.get() + 1));
    ProcessLock::Held(guard)
}

/// Владелец межпроцессного sidecar-лока: handle + путь файла лока, чтобы Drop
/// мог и отпереть блокировку, и убрать артефакт с диска.
struct CrossLockGuard {
    file: File,
    path: PathBuf,
}

/// Межпроцессный sidecar-лок: держим, если успели за бюджет; иначе пишем без него.
///
/// Инвариант занятости: она определяется успехом захвата fs2-блокировки,
/// а НЕ наличием `<имя>.lock` на диске. Поэтому `Held` при освобождении
/// удаляет и сам файл (SQLite-паттерн служебных файлов операций: успех не
/// оставляет следов), а режим `Skipped` файл не трогает вовсе — блокировку
/// мог уже забрать другой процесс, и его право на файл теперь главное.
enum CrossLock {
    /// `Option` обязателен: `Drop` не разрешает разобрать тип с Drop по
    /// значениям (E0509), а `take()` изнутри — стандартный способ забрать
    /// владение в `drop(&mut self)`.
    Held(Option<CrossLockGuard>),
    Skipped,
}

impl Drop for CrossLock {
    fn drop(&mut self) {
        // Порядок «unlock → закрыть handle → удалить файл» обязателен:
        // remove_file по собственному открытому handle на Windows даёт
        // sharing violation.
        let guard = match self {
            CrossLock::Held(slot) => slot.take(),
            CrossLock::Skipped => None,
        };
        let Some(guard) = guard else { return };
        let _ = guard.file.unlock();
        drop(guard.file);
        if let Err(e) = fs::remove_file(&guard.path) {
            // Не ошибка данных: пока у чужого процесса открыт handle,
            // удаление невозможно; хвост подберёт [`sweep_stale_locks`].
            log::debug!(
                "ko-json-store: лок-файл {} не удалён ({}) — подберёт санитария",
                guard.path.display(),
                e
            );
        }
    }
}

/// Единая точка вычисления пути sidecar-лока (SSOT): `<файл>.lock` рядом.
fn lock_path_for(data_path: &Path) -> PathBuf {
    let file_name = data_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    data_path.with_file_name(format!("{}.lock", file_name))
}

fn acquire_cross_process(file: File, lock_path: &Path) -> CrossLock {
    let deadline = Instant::now() + LOCK_WAIT_BUDGET;
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => {
                return CrossLock::Held(Some(CrossLockGuard {
                    file,
                    path: lock_path.to_path_buf(),
                }));
            }
            Err(_) if Instant::now() < deadline => std::thread::sleep(LOCK_POLL_INTERVAL),
            Err(e) => {
                log::warn!(
                    "ko-json-store: межпроцессный лок {} занят дольше {} мс ({}). \
                     Записываем без него: возможна гонка с другим процессом, но не зависание.",
                    lock_path.display(),
                    LOCK_WAIT_BUDGET.as_millis(),
                    e
                );
                return CrossLock::Skipped;
            }
        }
    }
}

// ─────────────────────────── санитария хвостов лока ───────────────────────────

/// SQLite-паттерн hot-journal: служебный файл операции не переживает саму
/// операцию ([`CrossLock::drop`] удаляет его после успеха), но хвост после
/// обрыва процесса возможен — и он подбирается при следующем старте.
///
/// Для каждого `<dir>/*.lock` пробуем захватить fs2-блокировку БЕЗ ожидания:
/// захват удался → владельца нет → файл-хвост удаляем; не удался → лок живой
/// (его держит другой процесс) — не трогаем. Признак «хвоста» — отсутствие
/// владельца, а НЕ «файл есть».
pub fn sweep_stale_locks(dir: &Path) -> Result<usize, String> {
    let entries = fs::read_dir(dir)
        .map_err(|e| format!("ko-json-store: свип {}: {}", dir.display(), e))?;

    let mut removed = 0usize;
    let mut busy = 0usize;

    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("lock") {
            continue;
        }

        let file = match OpenOptions::new().write(true).open(&path) {
            Ok(f) => f,
            Err(_) => {
                // Открыть не удалось: чей-то handle держит файл — это не хвост.
                busy += 1;
                continue;
            }
        };

        match file.try_lock_exclusive() {
            Ok(()) => {
                drop(file); // handle закрыть ДО удаления — иначе sharing violation
                match fs::remove_file(&path) {
                    Ok(()) => removed += 1,
                    // Пока кто-то держит handle, удаление невозможно.
                    Err(_) => busy += 1,
                }
            }
            Err(_) => {
                busy += 1;
                drop(file);
            }
        }
    }

    log::info!(
        "[BOOT] ko-json-store: санитария хвостов {}: удалено {}, занятых пропущено {}",
        dir.display(),
        removed,
        busy
    );
    Ok(removed)
}

// ───────────────────────────────── доступность ─────────────────────────────────

/// Настоящая проба записываемости каталога.
///
/// Порядок повторяет [`write_atomic`]: создать файл, записать, `rename` в
/// финальное имя, удалить. Если хоть один шаг не прошёл — каталог для
/// приложения непригоден, независимо от того, что показывает `exists()`.
/// Именно поэтому `access()`/`Path::exists()` здесь неприменимы: они смотрят
/// на метаданные и не ловят отказ в доступе (TOCTOU).
pub fn probe_dir(dir: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(dir)
        .map_err(|e| format!("{}: создать каталог не удалось ({})", dir.display(), e))?;

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let name = format!("probe-{}-{}.tmp", std::process::id(), nanos);
    let tmp = dir.join(&name);
    let renamed = dir.join(format!("{}.renamed", name));

    let failure = |stage: &str, e: &dyn std::fmt::Display| {
        let _ = fs::remove_file(&tmp);
        let _ = fs::remove_file(&renamed);
        format!("{}: {} не прошла ({})", dir.display(), stage, e)
    };

    let mut file = File::create(&tmp).map_err(|e| failure("создание файла", &e))?;
    file.write_all(b"probe")
        .and_then(|_| file.sync_all())
        .map_err(|e| failure("запись и синхронизация", &e))?;
    drop(file);

    fs::rename(&tmp, &renamed).map_err(|e| failure("переименование", &e))?;
    fs::remove_file(&renamed).map_err(|e| failure("удаление", &e))?;

    Ok(dir.to_path_buf())
}

/// Первый записываемый каталог из лестницы.
///
/// Возвращает выбранный каталог вместе с причинами отказа по предыдущим
/// кандидатам, либо список приказов отказа по всем, если не подошёл ни один.
/// Существующий, но закрытый каталог бесполезен так же, как отсутствующий.
pub fn first_writable<I>(candidates: I) -> Result<(PathBuf, Vec<String>), Vec<String>>
where
    I: IntoIterator<Item = PathBuf>,
{
    let mut rejected = Vec::new();
    for candidate in candidates {
        match probe_dir(&candidate) {
            Ok(dir) => return Ok((dir, rejected)),
            Err(reason) => rejected.push(reason),
        }
    }
    Err(rejected)
}

// ─────────────────────────────────── примитивы ───────────────────────────────────

/// Атомарная запись строки в `path`: журнал → tmp → sync_all → rename → удаление
/// tmp и журнала.
///
/// Гарантия шире, чем у голого `rename`: значение фиксируется в
/// [`journal`] **до** подмены файла, поэтому отказ на любом шаге после этого не
/// уничтожает данные — их подхватит [`read_text`] или [`journal::recover_tree`].
/// Отказ самого журнала не прерывает запись: он лишь ослабляет гарантию до
/// прежней (атомарность при успехе), но не превращается в новый источник отказов.
pub fn write_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    if let Err(e) = journal::commit(path, content) {
        log::warn!(
            "ko-json-store: журнал для {} не записан ({}) — запись продолжена без него",
            path.display(),
            e
        );
    }
    let result = write_atomic_raw(path, content);
    if result.is_ok() {
        journal::clear(path);
    }
    result
}

/// Собственно атомарная подмена файла без журнала: tmp → `sync_all` → `rename`.
/// Вынесено отдельно, чтобы восстановление из журнала не завело рекурсию
/// «коммит → запись → коммит».
pub fn write_atomic_raw(path: &Path, content: &str) -> std::io::Result<()> {
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
    match &write_res {
        Ok(()) => {
            SUCCESS_WRITES.fetch_add(1, Ordering::Relaxed);
        }
        Err(e) => record_failure(path, "НЕ ЗАПИСАНО", e),
    }
    write_res
}

/// Прочитать файл с учётом журнала: незакрытое значение важнее файла на диске.
///
/// Вызывать это, а не `fs::read_to_string`: только так «запись не завершилась,
/// но значение зафиксировано» превращается в обычное чтение, а не в потерю
/// данных. Отсутствие файла — `Ok(None)`, а не ошибка.
pub fn read_text(path: &Path) -> std::io::Result<Option<String>> {
    if let Some(pending) = journal::pending(path) {
        return Ok(Some(pending));
    }
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Прочитать и разобрать JSON с учётом журнала. Отсутствующий файл —
/// `T::default()`: пустой конфиг и сломанный конфиг должны различаться, а
/// битый файл обязан оставляться на месте для разбора (как в [`update_json`]).
pub fn read_json<T>(path: &Path) -> Result<T, String>
where
    T: serde::de::DeserializeOwned + Default,
{
    let Some(raw) = read_text(path).map_err(|e| format!("ko-json-store: чтение {}: {}", path.display(), e))?
    else {
        return Ok(T::default());
    };
    serde_json::from_str(&raw).map_err(|e| {
        format!(
            "ko-json-store: {} не разобран ({}). Файл оставлен как есть — \
             следующая успешная запись его перезапишет.",
            path.display(),
            e
        )
    })
}

/// Read-modify-write JSON-файла с эксклюзивной блокировкой на sidecar `<file>.lock`.
///
/// Внутрипроцессная конкуренция снимается [`WRITE_SERIAL`] — потерь обновлений нет.
/// Межпроцессный sidecar-лок берётся попыткой в пределах [`LOCK_WAIT_BUDGET`]; если
/// он недоступен, запись всё равно выполняется и в лог уходит предупреждение.
///
/// При ошибке парсинга существующего файла берётся `T::default()` + `log::warn!`
/// (не роняем файл). Если файла нет — стартуем с `T::default()`.
pub fn update_json<T>(path: &Path, update_fn: impl FnOnce(&mut T)) -> Result<(), String>
where
    T: serde::Serialize + serde::de::DeserializeOwned + Default,
{
    let lock_path = lock_path_for(path);

    let _process = acquire_in_process();

    let _cross = match OpenOptions::new()
        .create(true)
        .write(true)
        .open(&lock_path)
    {
        Ok(file) => acquire_cross_process(file, &lock_path),
        Err(e) => {
            record_failure(
                path,
                &format!("НЕ ЗАПИСАНО: не удалось открыть lock {}", lock_path.display()),
                &e,
            );
            return Err(format!(
                "ko-json-store: не удалось открыть lock {}: {}",
                lock_path.display(),
                e
            ));
        }
    };

    // Чтение с учётом журнала: незакрытое значение предыдущей записи важнее
    // того, что лежит на диске — иначе обрыв записи молча откатывал бы данные.
    let mut value: T = match crate::read_text(path) {
        Ok(Some(s)) => match serde_json::from_str(&s) {
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
        Ok(None) => T::default(),
        Err(e) => {
            log::warn!(
                "ko-json-store: {} не прочитан ({}), используем default",
                path.display(),
                e
            );
            T::default()
        }
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
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

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

    #[test]
    fn probe_dir_creates_and_accepts_writable_dir() {
        let dir = tmp_dir("probe_ok");
        assert!(probe_dir(&dir).is_ok());
        assert!(probe_dir(&dir).is_ok(), "повторная проба обязана проходить");
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("probe-"))
            .collect();
        assert!(leftovers.is_empty(), "проба обязана убирать за собой файлы");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn probe_dir_rejects_path_that_is_a_file() {
        let dir = tmp_dir("probe_file");
        let file = dir.join("occupied");
        fs::write(&file, b"x").unwrap();
        assert!(
            probe_dir(&file).is_err(),
            "путь-файл не может быть рабочей папкой"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn first_writable_skips_broken_candidate_and_records_reason() {
        let dir = tmp_dir("probe_ladder");
        let broken = dir.join("occupied");
        fs::write(&broken, b"x").unwrap();
        let good = dir.join("good");

        let (chosen, rejected) =
            first_writable(vec![broken, good.clone()]).expect("должен найти рабочий");
        assert_eq!(chosen, good);
        assert!(!rejected.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn first_writable_reports_every_failure_when_nothing_works() {
        let dir = tmp_dir("probe_allbad");
        let a = dir.join("a");
        let b = dir.join("b");
        fs::write(&a, b"x").unwrap();
        fs::write(&b, b"x").unwrap();

        let rejected = first_writable(vec![a, b]).expect_err("оба кандидата — файлы");
        assert_eq!(rejected.len(), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn health_counts_failed_writes_and_keeps_reason() {
        let before = health().failed_writes;
        let dir = tmp_dir("health");
        let missing = dir.join("nonexistent").join("data.json");
        assert!(write_atomic(&missing, "{}").is_err());

        let after = health();
        assert!(after.failed_writes > before);
        assert!(
            after.last_error.is_some(),
            "причина отказа обязана быть видна наружу"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn nested_update_json_on_same_thread_does_not_deadlock() {
        let dir = tmp_dir("nested");
        let path = dir.join("outer.json");
        fs::write(&path, r#"{}"#).unwrap();

        let inner = dir.join("inner.json");
        let inner_for_closure = inner.clone();
        let result = update_json(&path, move |root: &mut serde_json::Value| {
            // Вложенная запись в другой файл: без исключения реентрантности
            // она ждала бы блокировку, которую держит этот же поток.
            update_json(&inner_for_closure, |v: &mut serde_json::Value| {
                v["ok"] = serde_json::json!(true);
            })
            .unwrap();
            root["done"] = serde_json::json!(true);
        });
        assert!(result.is_ok());
        assert_eq!(
            fs::read_to_string(&inner).unwrap().contains("true"),
            true,
            "вложенная запись должна состояться"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn update_json_removes_sidecar_lock_after_success() {
        let dir = tmp_dir("lock_cleanup");
        let path = dir.join("data.json");

        update_json(&path, |val: &mut serde_json::Value| {
            val["a"] = serde_json::json!(1);
        })
        .unwrap();
        assert!(
            !dir.join("data.json.lock").exists(),
            "лок-файл обязан удаляться после успешной update_json"
        );

        // Второй прогон по тому же файлу — тоже без хвостов (лок создаётся
        // и удаляется заново на каждом вызове).
        update_json(&path, |val: &mut serde_json::Value| {
            val["b"] = serde_json::json!(2);
        })
        .unwrap();
        assert!(
            !dir.join("data.json.lock").exists(),
            "повторная update_json не должна оставлять лок"
        );

        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(val["a"], serde_json::json!(1));
        assert_eq!(val["b"], serde_json::json!(2));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sweep_stale_locks_removes_ownerless_and_skips_busy() {
        let dir = tmp_dir("sweep");

        // Живой лок: владелец держит fs2-блокировку — свип обязан пропустить.
        let lock_busy = dir.join("busy.json.lock");
        let holder = OpenOptions::new()
            .create(true)
            .write(true)
            .open(&lock_busy)
            .unwrap();
        holder.try_lock_exclusive().unwrap();

        // Хвост: файл на диске есть, владельца нет (open без захвата лока).
        let lock_stale = dir.join("stale.json.lock");
        drop(
            OpenOptions::new()
                .create(true)
                .write(true)
                .open(&lock_stale)
                .unwrap(),
        );

        let removed = sweep_stale_locks(&dir).unwrap();

        assert_eq!(removed, 1, "удалён только безвладелецный хвост");
        assert!(lock_busy.exists(), "живой лок обязан пережить свип");
        assert!(!lock_stale.exists(), "хвост обязан удаляться свипом");

        drop(holder);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sweep_stale_locks_counts_zero_on_empty_dir() {
        let dir = tmp_dir("sweep_empty");
        let removed = sweep_stale_locks(&dir).unwrap();
        assert_eq!(removed, 0);
        // Файл без суффикса .lock не должен трогаться вовсе.
        let stranger = dir.join("keep_me.json");
        fs::write(&stranger, b"{}").unwrap();
        let removed = sweep_stale_locks(&dir).unwrap();
        assert_eq!(removed, 0);
        assert!(stranger.exists());
        let _ = fs::remove_dir_all(&dir);
    }
}