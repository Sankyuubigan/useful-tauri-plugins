//! Папка сбора для юнит-тестов плагина.
//!
//! ## Зачем своя, а не `std::env::temp_dir()`
//!
//! `std::env::temp_dir()` возвращает СИСТЕМНУЮ временную папку. На этой машине
//! с ней уже воспроизводился отказ: временный файл не удалялся
//! (`PermissionDenied`), и 39 тестов падали не из-за своей логики, а из-за
//! папки, которой владеет кто-то ещё на этой машине.
//!
//! Инвариант тот же, что в сборке (`tauri-build-toolkit/lib/scratch.cjs`) и в
//! установщике роутеров (`installer::scratch_dir`): артефакты принадлежат
//! дереву сборки крейта, а не разделяемой системной папке. Тогда тест не может
//! сломаться из-за чужого процесса, а его мусор убирается вместе с `target`.
//!
//! Тесты идут в несколько потоков, поэтому имя включает счётчик: общая папка
//! приводила бы к взаимному удалению каталогов.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static SEQ: AtomicUsize = AtomicUsize::new(0);

/// Корень сбора: `target/test-scratch` внутри крейта.
pub fn scratch_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-scratch")
}

/// Уникальная папка для одного теста. Создаётся сразу: тест, который упал на
/// `create_dir_all`, падает на своей логике, а не на файловой системе.
pub fn scratch_dir(tag: &str) -> PathBuf {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = scratch_root().join(format!("{}_{}", tag, seq));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).expect("создать папку сбора теста");
    p
}

/// Счётчик для тестов, которым нужно имя без создания папки прямо сейчас.
pub fn next_seq() -> usize {
    SEQ.fetch_add(1, Ordering::Relaxed)
}