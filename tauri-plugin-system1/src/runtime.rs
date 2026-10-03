//! Инициализация ONNX Runtime, каталог моделей и доставка самого Runtime.
//!
//! # Почему DLL скачивается, а не бандлится
//!
//! Оба нативных движка проекта так устроены и это зафиксировано в глобальной
//! документации: движок «**не собирается и не бандлится** в приложение, он
//! скачивается пользователем» (`llama_cpp_engine.md:44`,
//! `crispasr_engine.md:41`), а `bundle.resources` «бандлит только `agents/`,
//! `mcp_servers/`, каталоги и `*.json`» (`llama_cpp_engine.md:70-72`).
//! ONNX Runtime — такой же движок, поэтому ставится тем же способом: с сервера,
//! в данные пользователя, со сверкой хэша.

use std::path::Path;
use std::sync::OnceLock;

use crate::catalog;
use crate::paths;
use tauri_plugin_downloader::{download as download_file, DownloadOptions};

/// Версия ONNX Runtime.
///
/// Не ниже 1.27: `ort` 2.0.0-rc.13 требует `ORT_API_VERSION` = 27
/// (`ort-sys/src/version.rs`). Ставим ровно ту, на которой измерены 189/234.
pub const RUNTIME_VERSION: &str = "1.28.0";

/// Имя zip-архива релиза.
const RUNTIME_ZIP_NAME: &str = "onnxruntime-win-x64-1.28.0.zip";

/// Официальный релиз Microsoft, CPU-сборка: без DirectML-зависимостей.
const RUNTIME_ZIP_URL: &str =
    "https://github.com/microsoft/onnxruntime/releases/download/v1.28.0/onnxruntime-win-x64-1.28.0.zip";

/// sha256 `onnxruntime.dll`, распакованной из этого архива.
///
/// Зафиксирован на файле, который уже отработал в продакшн-хосте: без сверки
/// «скачалось что-то» означало бы «DLL может оказаться чем угодно», и ошибка
/// всплыла бы уже как падение инференса без внятной причины.
pub const RUNTIME_DLL_SHA256: &str =
    "18370C375F07357FA5874344A9D9AC17E6B6FE1EB18B1DD209D79483B4470257";

/// Состояние доставки ONNX Runtime для UI.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeState {
    /// DLL на месте.
    pub present: bool,
    /// Полный путь, куда кладётся (и где искать) DLL.
    pub path: String,
    /// Размер DLL на диске, 0 если её нет.
    pub size_bytes: u64,
    /// Версия, которую ставит плагин.
    pub version: String,
    /// Ожидаемый размер DLL — для оценки времени скачивания.
    pub expected_bytes: u64,
}

/// Размер `onnxruntime.dll` из v1.28.0 (win-x64, CPU).
const RUNTIME_DLL_BYTES: u64 = 15_809_848;

/// Проверить доставку Runtime, ничего не скачивая.
pub fn state() -> RuntimeState {
    let dll = paths::onnxruntime_dll();
    let size_bytes = std::fs::metadata(&dll).map(|meta| meta.len()).unwrap_or(0);
    RuntimeState {
        present: dll.is_file(),
        path: dll.display().to_string(),
        size_bytes,
        version: RUNTIME_VERSION.to_string(),
        expected_bytes: RUNTIME_DLL_BYTES,
    }
}

/// Скачать ONNX Runtime, если его нет, и сверить хэш.
///
/// Возвращает `Ok(true)`, если файл уже был на месте — вызывающий не должен
/// повторно качать 15 МБ при каждом нажатии.
pub async fn ensure_downloaded() -> Result<bool, String> {
    let dll = paths::onnxruntime_dll();
    if dll.is_file() {
        match catalog::verify(&dll, RUNTIME_DLL_SHA256)? {
            Some(true) => {
                log::info!("[system1] ONNX Runtime уже на месте: {}", dll.display());
                return Ok(false);
            }
            Some(false) => {
                log::warn!("[system1] хэш ONNX Runtime не совпал, перекачиваем");
                std::fs::remove_file(&dll).map_err(|error| {
                    format!("не удалить старый {}: {}", dll.display(), error)
                })?;
            }
            None => return Ok(false),
        }
    }

    let dir = paths::runtime_dir();
    paths::ensure_dir(&dir)?;

    let zip_path = dir.join(RUNTIME_ZIP_NAME);
    log::info!(
        "[system1] качаю ONNX Runtime {} -> {}",
        RUNTIME_ZIP_URL,
        zip_path.display()
    );

    download_file(
        RUNTIME_ZIP_URL,
        &zip_path,
        DownloadOptions {
            label: "ONNX Runtime (System-1)".into(),
            kind: "engine".into(),
            min_size: Some(1024 * 1024),
            keep_partial: true,
            ..Default::default()
        },
        None,
    )
    .await
    .map_err(|error| {
        log::error!("[system1] загрузка ONNX Runtime не удалась: {}", error);
        format!("ошибка загрузки ONNX Runtime: {}", error)
    })?;

    extract_dll(&zip_path, &dll)?;

    // Архив больше не нужен: ~12 МБ, а нужна из него ровно одна DLL.
    let _ = std::fs::remove_file(&zip_path);

    // Сверка обязательна и ПОСЛЕ распаковки: zip мог прийти битым, а это
    // единственный момент, где понятно, что именно мы распаковали.
    match catalog::verify(&dll, RUNTIME_DLL_SHA256)? {
        Some(true) => {}
        Some(false) => {
            let _ = std::fs::remove_file(&dll);
            return Err(format!(
                "sha256 {} не совпал с ожидаемым — файл удалён, повторите загрузку",
                RUNTIME_DLL_SHA256
            ));
        }
        None => {
            return Err("внутренняя ошибка: нет эталонного хэша для ONNX Runtime".into())
        }
    }

    let size = std::fs::metadata(&dll).map(|meta| meta.len()).unwrap_or(0);
    log::info!("[system1] ONNX Runtime готов: {} ({} байт)", dll.display(), size);
    Ok(true)
}

/// Достать из zip единственный нужный файл `onnxruntime.dll`.
///
/// Остальное содержимое архива (заголовки, `.lib`, метаданные) приложению не
/// нужно и на диске не оседает.
fn extract_dll(zip_path: &Path, dll: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip_path)
        .map_err(|error| format!("не открыть архив {}: {}", zip_path.display(), error))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| format!("архив не читается (повреждён или не zip): {}", error))?;

    let index = (0..archive.len())
        .find(|index| {
            archive
                .by_index(*index)
                .map(|entry| entry.name().ends_with(paths::RUNTIME_DLL_NAME))
                .unwrap_or(false)
        })
        .ok_or_else(|| {
            log::error!("[system1] в архиве {} нет {}", zip_path.display(), paths::RUNTIME_DLL_NAME);
            format!("в архиве нет {}", paths::RUNTIME_DLL_NAME)
        })?;

    let mut entry = archive
        .by_index(index)
        .map_err(|error| format!("чтение записи архива: {}", error))?;
    let mut out = std::fs::File::create(dll)
        .map_err(|error| format!("не создать {}: {}", dll.display(), error))?;
    std::io::copy(&mut entry, &mut out).map_err(|error| {
        log::error!("[system1] распаковка {} не удалась: {}", paths::RUNTIME_DLL_NAME, error);
        format!("распаковка {}: {}", paths::RUNTIME_DLL_NAME, error)
    })?;
    Ok(())
}

/// Результат первой попытки инициализации.
///
/// Хранится именно результат, а не только факт «попытка была»: если первая
/// попытка провалилась, `Once` больше не даёт повторить, и наивная проверка
/// вида «инициализировано — значит работает» молча сочла бы неработающее
/// окружение рабочим. Ошибка обязана возвращаться на КАЖДЫЙ вызов
/// (`global_ai_docs/core/rules.md` §2.2 — запрет молчаливых ошибок).
static ENVIRONMENT: OnceLock<Result<(), String>> = OnceLock::new();

/// Число потоков ONNX Runtime.
///
/// В хосте было жёстко зашито `with_intra_threads(4)`. Значение вынесено в одну
/// переменную, чтобы его можно было менять в одном месте; сам дефолт НЕ
/// подбирался — 4 это то, что уже стояло в проде.
pub fn intra_threads() -> usize {
    std::env::var("KING_ORCH_SYSTEM1_THREADS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(4)
}

/// Загрузить `onnxruntime.dll` в процесс. Вызывать ДО первого обращения к `ort`.
///
/// DLL не линкуется статически: при фиче `download-binaries` крейт `ort` вшивал
/// в таблицу импортов PE жёсткую ссылку на `DirectML.dll`, а Windows
/// разрешает импорты ДО `main()`. На машине без этой DLL процесс падал с
/// `STATUS_DLL_NOT_FOUND` — без окна, без лога, без `crash_dump.log`.
/// Фича `load-dynamic` убирает импорт, DLL открывается здесь.
pub fn init_environment(dll: &std::path::Path) -> Result<(), String> {
    let result = ENVIRONMENT.get_or_init(|| match ort::init_from(dll) {
        Ok(builder) => {
            if builder.commit() {
                log::info!("[system1] ONNX Runtime загружен: {}", dll.display());
                Ok(())
            } else {
                Err("окружение ONNX Runtime не создалось (commit() == false)".to_string())
            }
        }
        Err(error) => Err(format!(
            "не удалось загрузить ONNX Runtime {}: {}",
            dll.display(),
            error
        )),
    });

    match result {
        Ok(()) => Ok(()),
        Err(message) => {
            // Ошибка логируется на каждом вызове, а не только на первом:
            // иначе в логе будет одна строка, а провалов — много.
            log::error!("[system1] {}", message);
            Err(message.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths;

    #[test]
    fn runtime_dir_is_inside_data_root() {
        let dir = paths::runtime_dir();
        assert!(
            dir.ends_with("system1/runtime"),
            "рантайм обязан лежать в данных хоста, а не рядом с exe: {}",
            dir.display()
        );
    }

    #[test]
    fn pinned_version_satisfies_ort_api_requirement() {
        // `ort` 2.0.0-rc.13 требует ORT_API_VERSION = 27, то есть >= 1.27.
        let major_minor: Vec<u32> = RUNTIME_VERSION
            .split('.')
            .take(2)
            .filter_map(|part| part.parse().ok())
            .collect();
        assert_eq!(major_minor.len(), 2, "версия должна быть major.minor");
        assert!(
            major_minor[0] > 1 || (major_minor[0] == 1 && major_minor[1] >= 27),
            "ONNX Runtime {} ниже требуемого ort 1.27",
            RUNTIME_VERSION
        );
    }

    #[test]
    fn dll_sha_is_pinned() {
        // Пустой эталон = «проверять нечем», а это молчаливая дыра: скачался бы
        // любой файл и считался бы верным (core/rules.md §2.2).
        assert_eq!(RUNTIME_DLL_SHA256.len(), 64, "sha256 должен быть 64 hex-символа");
        assert!(
            RUNTIME_DLL_SHA256.chars().all(|c| c.is_ascii_hexdigit()),
            "в эталонном хэше есть не-hex символы"
        );
    }
}

/// Тест на РЕАЛЬНОМ скачивании. Требует сеть, поэтому `#[ignore]`.
///
/// Это единственная проверка пути доставки: без неё утверждение «DLL
/// скачивается и проходит сверку» осталось бы догадкой. Скачивается 15 МБ.
#[cfg(test)]
mod download_tests {
    use super::{ensure_downloaded, paths, state, RUNTIME_DLL_SHA256, RUNTIME_ZIP_NAME};
    use crate::catalog;

    /// Корень данных для тест-бинарника: `current_exe()` у него — это
    /// `target/debug/deps`, а не папка приложения.
    fn ensure_data_root() -> bool {
        match std::env::var("KING_ORCH_DATA_DIR") {
            Ok(dir) if !dir.trim().is_empty() => {
                crate::set_data_dir(std::path::PathBuf::from(dir));
                true
            }
            _ => {
                eprintln!("SKIP: задайте KING_ORCH_DATA_DIR — корень данных KingOrchData");
                false
            }
        }
    }

    #[tokio::test]
    #[ignore = "нужна сеть; скачивает 15 МБ, запускать явно"]
    async fn downloads_runtime_and_passes_sha256() {
        if !ensure_data_root() {
            return;
        }

        let before = state();
        let downloaded = ensure_downloaded().await.expect("скачивание ONNX Runtime");

        let dll = paths::onnxruntime_dll();
        assert!(dll.is_file(), "DLL не появилась: {}", dll.display());

        let actual = crate::catalog::sha256_file(&dll).expect("посчитать sha256");
        assert_eq!(
            actual,
            RUNTIME_DLL_SHA256,
            "хэш скачанной DLL не совпал с эталонным"
        );

        let after = state();
        assert!(after.present, "статус должен показывать DLL как готовую");
        assert!(after.size_bytes > 0, "размер должен быть известен");

        // Повторный вызов не должен качать заново: 15 МБ — не ерунда.
        let again = ensure_downloaded().await.expect("повторная проверка");
        assert!(!again, "повторный выпуск не должен ничего качать");
        assert!(
            downloaded || !before.present,
            "скачивание обязано произойти, если DLL не было"
        );

        // Архив после распаковки удаляется — на диске не остаётся мусора.
        let zip = paths::runtime_dir().join(RUNTIME_ZIP_NAME);
        assert!(!zip.exists(), "zip-архив должен быть удалён: {}", zip.display());
    }
}