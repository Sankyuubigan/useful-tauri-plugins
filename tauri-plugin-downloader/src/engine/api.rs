//! Публичный Rust API плагина (для других крейтов/плагинов/хоста).

use super::chain;
use super::progress::TaskHandle;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct DownloadOptions {
    /// Человекочитаемая метка для UI («Движок llama.cpp», «Модель», …).
    pub label: String,
    /// Категория: "engine" | "model" | "mmproj" | "app" | "bin" | "9router" | "file".
    pub kind: String,
    /// Ожидаемый размер (байт) — для прогресс-бара, если известен.
    pub expected_size: Option<u64>,
    /// Опциональная валидация: первые N байт должны совпасть с magic
    /// (например b"GGUF" для моделей).
    pub magic: Option<Vec<u8>>,
    /// Минимальный валидный размер (защита от HTML-страницы-заглушки).
    pub min_size: Option<u64>,
    /// Не удалять .part при ошибке (кросс-сессионный resume).
    pub keep_partial: bool,
}

impl DownloadOptions {
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }
    pub fn with_kind(mut self, kind: impl Into<String>) -> Self {
        self.kind = kind.into();
        self
    }
    pub fn with_expected_size(mut self, size: u64) -> Self {
        self.expected_size = Some(size);
        self
    }
    pub fn gguf() -> Self {
        Self {
            label: "Модель".into(),
            kind: "model".into(),
            magic: Some(b"GGUF".to_vec()),
            min_size: Some(1024 * 1024),
            ..Default::default()
        }
    }
}

/// Скачать файл по URL в dest (async). Бросает Err(String) при неудаче/отмене.
/// Эмитит события `downloader:progress` (если AppHandle зарегистрирован).
pub async fn download(
    url: &str,
    dest: impl AsRef<Path>,
    opts: DownloadOptions,
    on_log: Option<&(dyn Fn(String) + Send + Sync)>,
) -> Result<(), String> {
    let dest = dest.as_ref();
    let log = |msg: String| {
        if let Some(cb) = on_log {
            cb(msg.clone());
        }
        log::info!("[downloader] {}", msg);
    };

    let label = if opts.label.is_empty() {
        dest.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Загрузка".into())
    } else {
        opts.label.clone()
    };
    let kind = if opts.kind.is_empty() {
        "file".into()
    } else {
        opts.kind.clone()
    };

    // Уже есть и валиден?
    if let Some(expected) = opts.min_size {
        if let Ok(meta) = std::fs::metadata(dest) {
            if meta.len() >= expected && validate_magic(dest, opts.magic.as_deref()).is_ok() {
                log(format!("Файл уже скачан: {}", dest.display()));
                return Ok(());
            }
        }
    }

    let task = TaskHandle::new(&label, &kind, url, &dest.to_string_lossy());
    log(format!("Старт: {} -> {}", url, dest.display()));

    let outcome = chain::download_with_fallback(
        &task,
        url,
        dest,
        opts.expected_size,
        &log,
    )
    .await;

    match outcome {
        Ok(o) => {
            // Валидация magic/min_size после успеха.
            if let Some(min) = opts.min_size {
                let len = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
                if len < min {
                    let _ = std::fs::remove_file(dest);
                    let msg = format!("Скачанный файл подозрительно мал ({} < {} байт)", len, min);
                    task.emit_terminal("error", &msg, len, o.total);
                    return Err(msg);
                }
            }
            if let Err(msg) = validate_magic(dest, opts.magic.as_deref()) {
                let _ = std::fs::remove_file(dest);
                task.emit_terminal("error", &msg, o.bytes, o.total);
                return Err(msg);
            }
            log(format!(
                "Готово (уровень {}, {} байт) -> {}",
                o.level_name,
                o.bytes,
                dest.display()
            ));
            task.emit_terminal("done", "Готово", o.bytes, o.total.max(o.bytes));
            Ok(())
        }
        Err(e) => {
            if !opts.keep_partial && e != "Отменено" {
                let part = super::reqwest_dl::part_path(dest);
                let _ = std::fs::remove_file(&part);
            }
            let status = if e == "Отменено" { "cancelled" } else { "error" };
            task.emit_terminal(status, &e, 0, opts.expected_size.unwrap_or(0));
            Err(e)
        }
    }
}

/// Синхронная обёртка (для spawn_blocking / синхронного кода).
pub fn download_blocking(
    url: &str,
    dest: impl AsRef<Path>,
    opts: DownloadOptions,
    on_log: Option<&(dyn Fn(String) + Send + Sync)>,
) -> Result<(), String> {
    // Выделяем 'static для on_log через box, если есть — иначе работаем без колбэка.
    // Проще: строим runtime и блокируемся.
    let dest = dest.as_ref().to_path_buf();
    let url = url.to_string();
    // on_log lifetime ограничен вызовом — копируем через Arc<dyn Fn> нельзя без 'static.
    // Для blocking API принимаем on_log: Option<Arc<dyn Fn(String) + Send + Sync>>.
    // Упрощённо: если on_log есть — оборачиваем в Arc здесь через unsafe lifetime
    // НЕ будем; вместо этого blocking-версия без on_log, логи идут через log::*.
    let _ = on_log;
    download_blocking_inner(&url, &dest, opts)
}

fn download_blocking_inner(
    url: &str,
    dest: &Path,
    opts: DownloadOptions,
) -> Result<(), String> {
    // Если уже внутри tokio runtime — block_in_place; иначе создаём runtime.
    match tokio::runtime::Handle::try_current() {
        Ok(_) => tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(download(url, dest, opts, None))
        }),
        Err(_) => {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|e| format!("runtime: {}", e))?;
            rt.block_on(download(url, dest, opts, None))
        }
    }
}

/// Скачать во временный файл и вернуть байты (для JSON/манифестов и т.п.).
pub async fn download_bytes_to_vec(
    url: &str,
    opts: DownloadOptions,
    on_log: Option<&(dyn Fn(String) + Send + Sync)>,
) -> Result<Vec<u8>, String> {
    let tmp = unique_temp_path();
    let mut o = opts;
    if o.label.is_empty() {
        o.label = "Загрузка".into();
    }
    if o.kind.is_empty() {
        o.kind = "file".into();
    }
    let r = download(url, &tmp, o, on_log).await;
    match r {
        Ok(()) => {
            let bytes = std::fs::read(&tmp).map_err(|e| format!("Чтение temp: {}", e))?;
            let _ = std::fs::remove_file(&tmp);
            Ok(bytes)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Байты, синхронно.
pub fn download_bytes_blocking(url: &str, opts: DownloadOptions) -> Result<Vec<u8>, String> {
    match tokio::runtime::Handle::try_current() {
        Ok(_) => tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(download_bytes_to_vec(url, opts, None))
        }),
        Err(_) => {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|e| format!("runtime: {}", e))?;
            rt.block_on(download_bytes_to_vec(url, opts, None))
        }
    }
}

fn unique_temp_path() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("king_dl_{}_{}.bin", std::process::id(), nanos))
}

fn validate_magic(path: &Path, magic: Option<&[u8]>) -> Result<(), String> {
    let Some(m) = magic else { return Ok(()) };
    let mut head = vec![0u8; m.len()];
    let mut f = std::fs::File::open(path).map_err(|e| format!("Открытие файла: {}", e))?;
    use std::io::Read;
    f.read_exact(&mut head)
        .map_err(|e| format!("Чтение magic: {}", e))?;
    if head != m {
        return Err(format!(
            "Файл не является ожидаемым форматом (magic {:?} != {:?})",
            String::from_utf8_lossy(&head),
            String::from_utf8_lossy(m)
        ));
    }
    Ok(())
}
