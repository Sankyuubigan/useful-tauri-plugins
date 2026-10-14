//! Журнал записи: значение фиксируется **до** подмены файла.
//!
//! `write_atomic` (tmp → `sync_all` → `rename`) гарантирует только одно:
//! читатель никогда не увидит наполовину записанный файл. Он **не** гарантирует,
//! что значение вообще дойдёт до диска: отказ на `File::create` или на `rename`
//! означал, что данные пользователя исчезают, хотя модель уже ответила.
//!
//! Протокол повторяет commit-семантику SQLite с redo-журналом, приведённую к
//! полному значению JSON (файл всегда пишется целиком, поэтому чтение по
//! смещению не нужно):
//!
//! 1. значение записывается в `<файл>.journal` и `sync_all` — **с этого момента
//!    значение считается зафиксированным**;
//! 2. значение записывается в сам файл атомарно;
//! 3. журнал удаляется — состояния, при которых значение потеряно, не существует.
//!
//! Падение между шагами 1 и 3 не теряет данные: [`pending`] читает последний
//! целый кадр, [`recover_tree`] накладывает его поверх файла при следующем старте,
//! а [`crate::read_text`] отдаёт его сразу, не дожидаясь восстановления.
//!
//! Формат кадра: заголовок `<crc32>\t<длина>\n` и сразу за ним ровно `длина`
//! байт значения. Длина в заголовке — обязательный элемент: значение может быть
//! многострочным (pretty JSON), и разделять кадры по переводу строки нельзя.
//! Контрольная сумма и точное совпадение длины отсекают оборванную запись —
//! главный источник «мусорного» значения.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Суффикс файла журнала.
const JOURNAL_SUFFIX: &str = ".journal";

/// Путь журнала для файла данных.
fn journal_path(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    Some(parent.join(format!("{}{}", name, JOURNAL_SUFFIX)))
}

/// Зафиксировать значение в журнале.
///
/// Ошибка **не** прерывает запись: журнал — слой прочности, и падать из-за него
/// означало бы превратить улучшение в новый источник отказов. Причина уходит в
/// лог — иначе «журнал не работает» обнаружится только после реальной потери.
pub fn commit(path: &Path, content: &str) -> std::io::Result<()> {
    let journal = journal_path(path).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("некорректный путь: {}", path.display()),
        )
    })?;
    let payload = content.as_bytes();
    let mut frame = format!("{:08x}\t{}\n", crc32(payload), payload.len()).into_bytes();
    frame.extend_from_slice(payload);

    // Однослотовый журнал: важен только последний зафиксированный кадр —
    // предыдущие значения либо уже в файле, либо ими заменены. Это держит файл
    // ограниченного размера даже при многократных отказах записи.
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&journal)?;
    file.write_all(&frame)?;
    file.sync_all()
}

/// Убрать журнал — значение теперь в файле.
pub fn clear(path: &Path) {
    if let Some(journal) = journal_path(path) {
        if let Err(e) = fs::remove_file(&journal) {
            if e.kind() != std::io::ErrorKind::NotFound {
                log::warn!(
                    "ko-json-store: журнал {} не удалён ({}) — значение всё равно прочитается из него",
                    journal.display(),
                    e
                );
            }
        }
    }
}

/// Последний целый кадр журнала, если он есть.
pub fn pending(path: &Path) -> Option<String> {
    let journal = journal_path(path)?;
    let raw = fs::read(&journal).ok()?;
    verify(&raw).ok().map(str::to_string)
}

/// Разбор кадра: заголовок, затем ровно `длина` байт значения.
///
/// Возвращает ошибку, если запись оборвана (файл короче объявленного) или
/// содержит хвост (значение дописано после кадра) — и то и другое означает, что
/// кадр не дошёл целиком и читать его нельзя.
fn verify(raw: &[u8]) -> Result<&str, &'static str> {
    let newline = raw
        .iter()
        .position(|byte| *byte == b'\n')
        .ok_or("нет заголовка — запись оборвана")?;
    let header = std::str::from_utf8(&raw[..newline]).map_err(|_| "заголовок не UTF-8")?;
    let mut parts = header.split('\t');
    let crc = parts.next().ok_or("нет контрольной суммы")?;
    let length: usize = parts.next().ok_or("нет длины")?.parse().map_err(|_| "длина не число")?;

    let start = newline + 1;
    let end = start.checked_add(length).ok_or("длина кадра переполняет файл")?;
    if end != raw.len() {
        return Err("длина кадра не совпадает — запись оборвана");
    }
    let payload = std::str::from_utf8(&raw[start..end]).map_err(|_| "значение не UTF-8")?;
    if format!("{:08x}", crc32(payload.as_bytes())) != crc {
        return Err("контрольная сумма не совпадает — запись оборвана");
    }
    Ok(payload)
}

/// Наложить журнал на файл: записать значение и удалить журнал.
/// Возвращает `true`, если было что восстанавливать.
pub fn recover(path: &Path) -> bool {
    let Some(value) = pending(path) else {
        return false;
    };
    log::warn!(
        "ko-json-store: восстановление {} из журнала (предыдущая запись не завершилась)",
        path.display()
    );
    match crate::write_atomic_raw(path, &value) {
        Ok(()) => {
            clear(path);
            true
        }
        Err(e) => {
            log::error!(
                "ko-json-store: восстановление {} НЕ удалось ({}) — значение осталось в журнале",
                path.display(),
                e
            );
            false
        }
    }
}

/// Восстановить всё дерево каталогов. Вызывается один раз при старте, после
/// выбора корня.
///
/// Обход идёт **по журналам**, а не по файлам данных: прерывание между шагами 1
/// и 2 — это как раз состояние «файла ещё нет, значение уже зафиксировано»,
/// и найти его можно только поjournal-файлу. Обход по файлам данных такой случай
/// пропускал бы, то есть первая в жизни запись терялась бы, а все следующие
/// восстанавливались.
pub fn recover_tree(dir: &Path) -> usize {
    let mut restored = 0usize;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                stack.push(path);
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if let Some(stem) = name.strip_suffix(JOURNAL_SUFFIX) {
                let target = path.with_file_name(stem);
                if is_data_file(&target) && recover(&target) {
                    restored += 1;
                }
            }
        }
    }
    if restored > 0 {
        log::info!(
            "ko-json-store: восстановлено {} файлов из журнала в {}",
            restored,
            dir.display()
        );
    }
    restored
}

/// Файл данных, а не журнал и не временный остаток.
fn is_data_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    !name.ends_with(JOURNAL_SUFFIX) && !name.ends_with(".tmp") && !name.ends_with(".lock")
}

/// CRC-32 (IEEE 802.3, reflected) — таблица считается один раз.
fn crc32(data: &[u8]) -> u32 {
    static TABLE: OnceLock<[u32; 256]> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut table = [0u32; 256];
        for (i, slot) in table.iter_mut().enumerate() {
            let mut value = i as u32;
            for _ in 0..8 {
                value = if value & 1 != 0 {
                    0xEDB8_8320 ^ (value >> 1)
                } else {
                    value >> 1
                };
            }
            *slot = value;
        }
        table
    });

    let mut crc = 0xFFFF_FFFFu32;
    for byte in data {
        crc = table[((crc ^ *byte as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("ko_journal_{}_{}", tag, nanos));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn commit_then_pending_round_trip() {
        let dir = scratch("round");
        let path = dir.join("data.json");
        commit(&path, r#"{"a":1}"#).unwrap();

        assert_eq!(pending(&path).as_deref(), Some(r#"{"a":1}"#));
        assert!(!path.exists(), "журнал не заменяет файл — он его опережает");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn clear_removes_pending() {
        let dir = scratch("clear");
        let path = dir.join("data.json");
        commit(&path, "{}").unwrap();
        clear(&path);
        assert!(pending(&path).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn multiline_content_survives_the_round_trip() {
        let dir = scratch("multiline");
        let path = dir.join("data.json");
        let pretty = "{\n  \"a\": 1\n}";
        commit(&path, pretty).unwrap();

        let recovered = pending(&path).unwrap();
        assert_eq!(recovered, pretty, "значение обязано вернуться байт в байт");
        let value: serde_json::Value = serde_json::from_str(&recovered).unwrap();
        assert_eq!(value["a"], 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn torn_frame_is_discarded() {
        let dir = scratch("torn");
        let path = dir.join("data.json");
        let journal = journal_path(&path).unwrap();
        // Кадр объявлен длиннее, чем записано, — запись оборвалась.
        let payload = r#"{"a":1}"#;
        let mut frame = format!("{:08x}\t{}\n", crc32(payload.as_bytes()), payload.len() + 10)
            .into_bytes();
        frame.extend_from_slice(payload.as_bytes());
        fs::write(&journal, frame).unwrap();

        assert!(
            pending(&path).is_none(),
            "оборванный кадр обязан отбрасываться, а не читаться как значение"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupted_payload_fails_verification() {
        let dir = scratch("corrupt");
        let path = dir.join("data.json");
        let journal = journal_path(&path).unwrap();
        let payload = r#"{"a":1}"#;
        let forged = r#"{"b":2}"#;
        let mut frame =
            format!("{:08x}\t{}\n", crc32(payload.as_bytes()), payload.len()).into_bytes();
        frame.extend_from_slice(forged.as_bytes());
        fs::write(&journal, frame).unwrap();

        assert!(pending(&path).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recover_writes_value_and_clears_journal() {
        let dir = scratch("recover");
        let path = dir.join("data.json");
        fs::write(&path, r#"{"старое":1}"#).unwrap();
        commit(&path, r#"{"новое":2}"#).unwrap();

        assert!(recover(&path));
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value["новое"], 2);
        assert!(pending(&path).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recover_tree_walks_nested_dirs_and_ignores_journals() {
        let dir = scratch("tree");
        let nested = dir.join("sessions").join("problem_1");
        fs::create_dir_all(&nested).unwrap();
        let path = nested.join("s1.json");
        commit(&path, r#"{"восстановлено":true}"#).unwrap();

        assert_eq!(recover_tree(&dir), 1);
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("восстановлено"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_text_prefers_pending_value_over_file() {
        let dir = scratch("read");
        let path = dir.join("data.json");
        fs::write(&path, r#"{"старое":1}"#).unwrap();
        commit(&path, r#"{"новое":2}"#).unwrap();

        // Именно этот сценарий и приводил к потере данных: файл старый,
        // недописанная запись — новая. Читать обязан быть новый.
        assert_eq!(
            crate::read_text(&path).unwrap().as_deref(),
            Some(r#"{"новое":2}"#)
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn crc32_matches_known_vector() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }
}