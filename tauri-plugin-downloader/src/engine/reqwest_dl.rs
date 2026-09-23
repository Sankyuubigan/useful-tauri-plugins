//! Уровень 1: reqwest — multi-chunk при поддержке Range, stall-детект, resume, ретраи.
//! НЕТ общего timeout на весь запрос (причина старого бага 120s) — только stall.

use super::progress::TaskHandle;
use futures_util::StreamExt;
use std::cmp::min;
use std::fs::File;
use std::io::SeekFrom;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const CHUNK_COUNT: usize = 8;
const MAX_CHUNK_RETRIES: usize = 10;
const RETRY_BASE_MS: u64 = 500;
const STALL_TIMEOUT: Duration = Duration::from_secs(30);
const PART_SUFFIX: &str = ".part";
const GUI_EMIT_INTERVAL: Duration = Duration::from_millis(200);
const MIN_VALID_SIZE: u64 = 1;

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

const HF_MIRRORS: &[(&str, &str)] = &[
    ("hf.co", "hf-mirror.com"),
    ("huggingface.co", "hf-mirror.com"),
];

pub fn mirror_url(url: &str) -> Option<String> {
    for (from, to) in HF_MIRRORS {
        let prefix = format!("https://{}", from);
        if let Some(stripped) = url.strip_prefix(&prefix) {
            return Some(format!("https://{}{}", to, stripped));
        }
    }
    None
}

/// HTTP-клиент БЕЗ общего timeout (только redirect + UA). Stall ловим в стриме.
fn make_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(UA)
        .redirect(reqwest::redirect::Policy::limited(10))
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Ошибка создания HTTP-клиента: {}", e))
}

/// Резолв: финальный URL, точный размер, поддержка Range.
async fn resolve_target(client: &reqwest::Client, url: &str) -> Result<(String, u64, bool), String> {
    let resp = client
        .get(url)
        .header("User-Agent", UA)
        .header("Accept", "*/*")
        .header("Range", "bytes=0-0")
        .send()
        .await
        .map_err(|e| format!("Ошибка подключения: {}", e))?;

    let status = resp.status();
    if !status.is_success() && status != reqwest::StatusCode::PARTIAL_CONTENT {
        let body = resp.text().await.unwrap_or_default();
        let preview: String = body.chars().take(300).collect();
        return Err(format!("Ошибка загрузки (HTTP {}): {}", status, preview));
    }

    let final_url = resp.url().to_string();
    let supports_range = status == reqwest::StatusCode::PARTIAL_CONTENT;
    let total = if supports_range {
        resp.headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|cr| cr.to_str().ok())
            .and_then(|s| s.rsplit('/').next())
            .and_then(|t| t.trim().parse::<u64>().ok())
            .unwrap_or(0)
    } else {
        resp.content_length().unwrap_or(0)
    };
    Ok((final_url, total, supports_range))
}

/// Полный проход: резолв → multi-chunk / single → finalize.
/// Возвращает (bytes_written, total).
pub async fn download(task: &TaskHandle, url: &str, dest: &Path) -> Result<(u64, u64), String> {
    let client = make_client()?;
    let part_path = part_path(dest);

    // Валидно уже на диске? (только если размер > 0)
    if let Ok(meta) = std::fs::metadata(dest) {
        if meta.len() >= MIN_VALID_SIZE && !part_exists(&part_path) {
            // Файл уже есть — но только если это не мусор от прошлой попытки.
            // Оставляем на усмотрение вызывающего (expected_size сверяется в chain).
        }
    }

    let (final_url, total, supports_range) = resolve_target(&client, url).await?;

    if total == 0 || !supports_range {
        let bytes = download_single(task, &client, url, dest, &part_path, total).await?;
        return Ok((bytes, total.max(bytes)));
    }

    // Sparse .part
    {
        let f = File::create(&part_path).map_err(|e| format!("Ошибка создания файла: {}", e))?;
        f.set_len(total)
            .map_err(|e| format!("Ошибка выделения места: {}", e))?;
    }

    let total = Arc::new(total);
    let final_url = Arc::new(tokio::sync::Mutex::new(final_url));
    let downloaded = Arc::new(AtomicU64::new(0));
    let abort = Arc::new(AtomicBool::new(false));
    let client = Arc::new(client.clone());
    let orig_url = url.to_string();

    // Репортер прогресса (скорость через дельту).
    let rep_task_id = task.id.clone();
    let rep_label = task.label.clone();
    let rep_kind = task.kind.clone();
    let rep_url = task.url.clone();
    let rep_dest = task.dest.clone();
    let rep_total = total.clone();
    let rep_dl = downloaded.clone();
    let rep_cancel = task.cancel.clone();
    let reporter = tokio::spawn(async move {
        let mut last = Instant::now();
        let mut last_bytes = rep_dl.load(Ordering::SeqCst);
        loop {
            tokio::time::sleep(GUI_EMIT_INTERVAL).await;
            if rep_cancel.load(Ordering::SeqCst) {
                break;
            }
            let now = Instant::now();
            let cur = rep_dl.load(Ordering::SeqCst);
            let dt = now.duration_since(last).as_secs_f64();
            let speed = if dt > 0.0 {
                (cur.saturating_sub(last_bytes)) as f64 / dt
            } else {
                0.0
            };
            emit_external(&rep_task_id, &rep_label, &rep_kind, &rep_url, &rep_dest, cur, *rep_total, speed);
            last = now;
            last_bytes = cur;
            if cur >= *rep_total {
                break;
            }
        }
    });

    let chunk_size = (*total + CHUNK_COUNT as u64 - 1) / CHUNK_COUNT as u64;
    let mut set: tokio::task::JoinSet<Result<(), String>> = tokio::task::JoinSet::new();

    for i in 0..CHUNK_COUNT {
        let client = client.clone();
        let final_url = final_url.clone();
        let total = total.clone();
        let downloaded = downloaded.clone();
        let abort = abort.clone();
        let part_path = part_path.clone();
        let orig_url = orig_url.clone();
        let cancel = task.cancel.clone();

        set.spawn(async move {
            if abort.load(Ordering::SeqCst) || cancel.load(Ordering::SeqCst) {
                return Err(format!("чанк {} отменён", i));
            }
            let start0 = (i as u64) * chunk_size;
            let end = min(start0 + chunk_size, *total);
            let mut written: u64 = 0;
            let mut attempt: usize = 0;

            loop {
                if abort.load(Ordering::SeqCst) || cancel.load(Ordering::SeqCst) {
                    return Err(format!("чанк {} отменён", i));
                }
                let range_start = start0 + written;
                if range_start >= end {
                    return Ok(());
                }

                let url_now = final_url.lock().await.clone();
                let resp = match client
                    .get(&url_now)
                    .header("User-Agent", UA)
                    .header("Accept", "*/*")
                    .header("Range", format!("bytes={}-{}", range_start, end - 1))
                    .send()
                    .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        attempt += 1;
                        if attempt > MAX_CHUNK_RETRIES {
                            return Err(format!("чанк {}: сетевая ошибка: {}", i, e));
                        }
                        sleep_backoff(attempt).await;
                        continue;
                    }
                };

                let status = resp.status();
                if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::UNAUTHORIZED
                {
                    attempt += 1;
                    if attempt > MAX_CHUNK_RETRIES {
                        return Err(format!("чанк {}: HTTP {}", i, status));
                    }
                    if let Ok((nu, _, _)) = resolve_target(&client, &orig_url).await {
                        *final_url.lock().await = nu;
                    }
                    sleep_backoff(attempt).await;
                    continue;
                }
                if !status.is_success() && status != reqwest::StatusCode::PARTIAL_CONTENT {
                    attempt += 1;
                    if attempt > MAX_CHUNK_RETRIES {
                        return Err(format!("чанк {}: HTTP {}", i, status));
                    }
                    sleep_backoff(attempt).await;
                    continue;
                }

                let mut stream = resp.bytes_stream();
                let mut file = match tokio::fs::OpenOptions::new()
                    .write(true)
                    .open(&part_path)
                    .await
                {
                    Ok(f) => f,
                    Err(e) => return Err(format!("чанк {}: ошибка файла: {}", i, e)),
                };

                let mut stalled = false;
                while let Some(item) =
                    tokio::time::timeout(STALL_TIMEOUT, stream.next()).await.unwrap_or(None)
                {
                    match item {
                        Ok(bytes) => {
                            if bytes.is_empty() {
                                continue;
                            }
                            if let Err(e) = file.seek(SeekFrom::Start(range_start + written)).await {
                                return Err(format!("чанк {}: seek: {}", i, e));
                            }
                            if let Err(e) = file.write_all(&bytes).await {
                                return Err(format!("чанк {}: запись: {}", i, e));
                            }
                            written += bytes.len() as u64;
                            downloaded.fetch_add(bytes.len() as u64, Ordering::SeqCst);
                        }
                        Err(_) => {
                            stalled = true;
                            break;
                        }
                    }
                }

                if stalled {
                    attempt += 1;
                    if attempt > MAX_CHUNK_RETRIES {
                        return Err(format!("чанк {}: неоднократный обрыв потока", i));
                    }
                    sleep_backoff(attempt).await;
                    continue;
                }

                if written >= (end - start0) {
                    return Ok(());
                }
                attempt += 1;
                if attempt > MAX_CHUNK_RETRIES {
                    return Err(format!("чанк {}: не докачан до конца", i));
                }
                sleep_backoff(attempt).await;
            }
        });
    }

    let mut fatal: Option<String> = None;
    while let Some(join) = set.join_next().await {
        match join {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                abort.store(true, Ordering::SeqCst);
                if fatal.is_none() {
                    fatal = Some(e);
                }
            }
            Err(e) => {
                abort.store(true, Ordering::SeqCst);
                if fatal.is_none() {
                    fatal = Some(format!("паника таска: {}", e));
                }
            }
        }
    }

    reporter.abort();

    if task.is_cancelled() {
        let _ = std::fs::remove_file(&part_path);
        return Err("Отменено".into());
    }

    if let Some(e) = fatal {
        let _ = std::fs::remove_file(&part_path);
        return Err(e);
    }

    finalize(&part_path, dest, *total)?;
    Ok((*total, *total))
}

async fn download_single(
    task: &TaskHandle,
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    part_path: &Path,
    known_total: u64,
) -> Result<u64, String> {
    for attempt in 1..=MAX_CHUNK_RETRIES {
        if task.is_cancelled() {
            return Err("Отменено".into());
        }
        if attempt > 1 {
            sleep_backoff(attempt).await;
        }

        let resp = match client
            .get(url)
            .header("User-Agent", UA)
            .header("Accept", "*/*")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                if attempt == MAX_CHUNK_RETRIES {
                    return Err(format!("Ошибка подключения: {}", e));
                }
                continue;
            }
        };
        if !resp.status().is_success() {
            if attempt == MAX_CHUNK_RETRIES {
                return Err(format!("HTTP {}", resp.status()));
            }
            continue;
        }

        let total = resp.content_length().unwrap_or(known_total);
        let mut file = match tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(part_path)
            .await
        {
            Ok(f) => f,
            Err(e) => return Err(format!("Ошибка создания файла: {}", e)),
        };

        let mut stream = resp.bytes_stream();
        let mut downloaded: u64 = 0;
        let mut last = Instant::now();
        let mut last_bytes: u64 = 0;
        let mut ok = true;

        while let Some(item) =
            tokio::time::timeout(STALL_TIMEOUT, stream.next()).await.unwrap_or(None)
        {
            match item {
                Ok(bytes) => {
                    if bytes.is_empty() {
                        continue;
                    }
                    if let Err(e) = file.write_all(&bytes).await {
                        return Err(format!("Ошибка записи: {}", e));
                    }
                    downloaded += bytes.len() as u64;
                    let now = Instant::now();
                    let dt = now.duration_since(last).as_secs_f64();
                    if dt >= 0.2 {
                        let speed = (downloaded - last_bytes) as f64 / dt;
                        task.emit_progress(downloaded, total, speed, "", false);
                        last = now;
                        last_bytes = downloaded;
                    }
                }
                Err(_) => {
                    ok = false;
                    break;
                }
            }
        }

        if task.is_cancelled() {
            let _ = std::fs::remove_file(part_path);
            return Err("Отменено".into());
        }

        if ok && (total == 0 || downloaded >= total) {
            task.emit_progress(downloaded, total.max(downloaded), 0.0, "", true);
            finalize(part_path, dest, total.max(downloaded))?;
            return Ok(downloaded);
        }
    }
    Err("Не удалось докачать файл (однопоточный режим)".into())
}

fn finalize(part_path: &Path, dest: &Path, total: u64) -> Result<(), String> {
    let meta = std::fs::metadata(part_path)
        .map_err(|e| format!("Не удалось прочитать скачанный файл: {}", e))?;
    if total > 0 && meta.len() != total {
        let _ = std::fs::remove_file(part_path);
        return Err(format!(
            "Размер не совпадает: скачано {} из {} байт",
            meta.len(),
            total
        ));
    }
    if meta.len() < MIN_VALID_SIZE {
        let _ = std::fs::remove_file(part_path);
        return Err("Скачанный файл пуст".into());
    }
    std::fs::rename(part_path, dest)
        .map_err(|e| format!("Ошибка переименования: {}", e))?;
    Ok(())
}

async fn sleep_backoff(attempt: usize) {
    let ms = RETRY_BASE_MS.saturating_mul(1u64 << (attempt.saturating_sub(1)).min(6));
    tokio::time::sleep(Duration::from_millis(ms)).await;
}

pub fn part_path(dest: &Path) -> std::path::PathBuf {
    let mut os = dest.as_os_str().to_owned();
    os.push(PART_SUFFIX);
    std::path::PathBuf::from(os)
}

fn part_exists(part: &Path) -> bool {
    part.exists()
}

/// Эмит из репортера — дублирует TaskHandle, но TaskHandle не Clone.
/// Используем глобальный реестр по task_id.
fn emit_external(
    task_id: &str,
    label: &str,
    kind: &str,
    url: &str,
    dest: &str,
    downloaded: u64,
    total: u64,
    speed_bps: f64,
) {
    use tauri::Emitter;
    let eta = if speed_bps > 1.0 && total > downloaded {
        ((total - downloaded) as f64 / speed_bps).round()
    } else {
        -1.0
    };
    let payload = super::progress::ProgressPayload {
        task_id: task_id.to_string(),
        label: label.to_string(),
        kind: kind.to_string(),
        url: url.to_string(),
        dest: dest.to_string(),
        downloaded,
        total,
        speed_bps,
        eta_s: eta,
        level: 1,
        level_name: "reqwest".into(),
        status: "running".into(),
        message: String::new(),
    };
    if let Some(app) = super::progress::app_handle() {
        let _ = app.emit(super::progress::PROGRESS_EVENT, payload);
    }
}
