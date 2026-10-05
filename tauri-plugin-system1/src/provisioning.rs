//! Скачивание модели с проверкой целостности.
//!
//! Порядок не переставляется: сначала файл, потом сверка хэша. Файл с неверным
//! хэшем удаляется, а не оставляется «на потом» — иначе следующий запуск
//! найдёт его и решит, что модель готова.
//!
//! Скачивание идёт через `tauri-plugin-downloader` (единый движок хоста): у него
//! есть multi-chunk, resume с `.part`, зеркало HuggingFace на случай обрыва и
//! событие `downloader:progress`. Свой загрузчик здесь означал бы второй
//! механизм доставки файлов в том же приложении.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri_plugin_downloader::{download as download_file, DownloadOptions};

use crate::catalog::{self, ModelEntry};
use crate::paths;
use crate::state;

/// Ожидаемый размер `model.onnx`.
///
/// Число НЕ выдумано: `646 870 871` — размер файла, который лежит в
/// `test/laya_probe/models/laya-multilingual-onnx/model.onnx` и чей sha256
/// зафиксирован в каталоге. Оно же уходит в `expected_size`, чтобы UI показал
/// прогрессбар, а не бесконечный индикатор.
const MODEL_ONNX_BYTES: u64 = 646_870_871;

/// Скачать все файлы модели и проверить каждый по sha256.
pub async fn download(entry: &ModelEntry) -> Result<(), String> {
    let dir = paths::model_dir(&entry.id);
    paths::ensure_dir(&dir)?;

    for file in &entry.files {
        let local = catalog::local_path(entry, &file.remote);

        if local.is_file() {
            // Уже есть — но хэш всё равно сверяем: файл мог остаться от
            // прерванной загрузки или быть подменён.
            match catalog::verify(&local, &file.sha256)? {
                Some(true) => {
                    log::info!("[system1] уже на месте и цел: {}", file.remote);
                    continue;
                }
                Some(false) => {
                    log::warn!("[system1] хэш не совпал, перекачиваем: {}", file.remote);
                    let _ = std::fs::remove_file(&local);
                }
                None => {
                    log::warn!("[system1] нет эталонного хэша для {}", file.remote);
                }
            }
        }

        if let Some(parent) = local.parent() {
            paths::ensure_dir(parent)?;
        }

        let url = catalog::file_url(entry, &file.remote);
        log::info!("[system1] скачиваю {} -> {}", url, local.display());

        let options = if file.remote == "model.onnx" {
            DownloadOptions {
                label: "Модель System-1 (Laya)".into(),
                kind: "model".into(),
                expected_size: Some(MODEL_ONNX_BYTES),
                min_size: Some(1024 * 1024),
                keep_partial: true,
                ..Default::default()
            }
        } else {
            DownloadOptions {
                label: format!("System-1: {}", file.remote),
                kind: "model".into(),
                keep_partial: false,
                ..Default::default()
            }
        };

        download_file(&url, &local, options, None)
            .await
            .map_err(|error| format!("не скачать {}: {}", file.remote, error))?;

        // Сверка сразу после загрузки. Несовпадение = файл удаляется и
        // команда падает: лучше явная ошибка, чем модель, которая тихо
        // выдаёт другие вероятности.
        match catalog::verify(&local, &file.sha256)? {
            Some(true) => log::info!("[system1] цел: {}", file.remote),
            Some(false) => {
                let _ = std::fs::remove_file(&local);
                return Err(format!(
                    "контрольная сумма не совпала для {}. Файл удалён.",
                    file.remote
                ));
            }
            None => {
                log::warn!(
                    "[system1] {} скачан, но хэш не сверялся (нет эталонного значения)",
                    file.remote
                );
            }
        }
    }

    Ok(())
}

/// Загрузка уже идёт: повторный вход обязан отказать, а не начать второй
/// поток на 646 МБ. `compare_exchange` вместо `Mutex`, потому что здесь нечего
/// блокировать — ждать не нужно, нужен только отказ.
static DOWNLOADING: AtomicBool = AtomicBool::new(false);

/// Доставить недостающие артефакты System-1, если автоскачивание разрешено.
///
/// ## Почему это здесь, а не в хосте
///
/// Граф workflow исполняется в **синхронном** потоке (`execute_node`,
/// `run_workflow` — sync), а доставка асинхронная. Если бы плагин отдавал
/// только `async`-функции, пришлось бы сделать `async` весь конвейер графа
/// ради одной загрузки. Поэтому здесь ровно одна синхронная точка входа для
/// движка, а внутри — отдельный поток со своим tokio-runtime: блокировка
/// допустима (граф последователен, пока эта нода ждёт, делать всё равно
/// нечего), и при этом ничего не блокирует пул Tokio хоста.
pub fn ensure_artifacts(model_id: &str) -> Result<(), String> {
    if !state::auto_download() {
        return Ok(());
    }

    let entry = state::resolve_model(model_id)?;
    let runtime_missing = !crate::runtime::state().present;
    let model = catalog::model_state(&entry);

    if !runtime_missing && model.present {
        return Ok(());
    }

    let mut wanted = Vec::new();
    if runtime_missing {
        wanted.push("ONNX Runtime".to_string());
    }
    wanted.extend(model.missing.iter().cloned());
    log::info!(
        "[system1] автоскачивание: не хватает [{}], начинаю доставку",
        wanted.join(", ")
    );

    download_missing(&entry, runtime_missing, !model.present)
}

/// Скачать недостающее в отдельном потоке с собственным tokio-runtime.
fn download_missing(
    entry: &ModelEntry,
    need_runtime: bool,
    need_model: bool,
) -> Result<(), String> {
    if DOWNLOADING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(
            "System-1 уже скачивается в другой сессии — дождитесь окончания загрузки"
                .to_string(),
        );
    }

    let entry = entry.clone();
    let outcome = std::thread::Builder::new()
        .name("system1-download".to_string())
        .spawn(move || -> Result<(), String> {
            let net = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| format!("не создать сетевой поток: {}", error))?;

            net.block_on(async move {
                if need_runtime {
                    crate::runtime::ensure_downloaded().await?;
                }
                if need_model {
                    download(&entry).await?;
                }
                Ok(())
            })
        })
        .map_err(|error| format!("не запустить поток загрузки: {}", error))?
        .join();

    // Флаг снимается на любом исходе, включая панику в потоке: иначе одна
    // неудачная загрузка навсегда блокировала бы автодоставку.
    DOWNLOADING.store(false, Ordering::SeqCst);

    match outcome {
        Ok(result) => result,
        Err(_) => Err("поток загрузки System-1 завершился аварийно".to_string()),
    }
}