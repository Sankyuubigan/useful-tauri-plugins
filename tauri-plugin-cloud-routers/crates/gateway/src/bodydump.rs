//! Файловый дамп сырых тел запрос/ответ — для расследований после факта.
//!
//! ## Зачем
//!
//! EventLog (дашборд) и `gateway.log` хранят только короткие события-выжимки:
//! «ответ 200 · {ок:true…». Когда инцидент требует посмотреть, что **именно**
//! модель получила и что написала (разбор странного ответа провайдера,
//! «HTTP 200, 0 кадров» и т.п.), сырых тел нигде нет — расследование вслепую.
//! Дамп пишет пары запрос/ответ в JSONL, читаемый после рестарта.
//!
//! ## Приватность
//!
//! Тела могут содержать переписку пользователя — файл лежит в `data_dir/logs/`
//! рядом с конфигом, доступа наружу нет. Заголовки НЕ дампятся вовсе: ключ
//! авторизации физически не может попасть в файл. URL пишется без query —
//! у части провайдеров секрет живёт в query-параметре.
//!
//! ## Границы
//!
//! Тело больше [`BODY_CAP_BYTES`] обрезается с честной пометкой. Файл дня
//! больше [`ROTATE_BYTES`] сдвигается в части `.1..`[`MAX_PARTS`] (старшая
//! удаляется). Ошибка записи не роняет запрос: один warn в журнал на процесс.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::task::{Context, Poll};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use futures_util::Stream;

/// Максимальный размер одного тела в дампе (сверх — пометка об обрезке).
pub const BODY_CAP_BYTES: usize = 1024 * 1024;
/// Порог ротации файла дня.
const ROTATE_BYTES: u64 = 20 * 1024 * 1024;
/// Максимальное число ротационных частей (`.1`..`.5`).
const MAX_PARTS: u32 = 5;

/// Пишущий дамп. Один на процесс, живёт в `AppState`.
pub struct Dumper {
    /// Каталог `<data_dir>/logs`.
    root: PathBuf,
    /// Сериализует файловые операции: append и ротация под одним замком.
    io: Mutex<()>,
    /// Warn об ошибке записи — один раз на процесс (без спама).
    warned: AtomicBool,
}

impl Dumper {
    pub fn new(data_dir: &Path) -> Self {
        Self { root: data_dir.join("logs"), io: Mutex::new(()), warned: AtomicBool::new(false) }
    }

    /// Запись запроса клиента→провайдер. Один раз на запрос, до ретраев.
    pub fn request(&self, model: &str, provider: &str, key: &str, url: &str, stream: bool, body: &[u8]) {
        let total = body.len() as u64;
        self.append(json_record(
            now_ms(),
            "req",
            model,
            provider,
            key,
            Some(strip_query(url)),
            None,
            None,
            stream,
            &body_str(body, total),
            None,
        ));
    }

    /// Запись ответа провайдера. Для стримов вызывается один раз — по
    /// завершении/обрыву потока (см. [`TeeBody`]), `completed` честно отличает
    /// доливку от обрыва.
    #[allow(clippy::too_many_arguments)]
    pub fn response(
        &self,
        model: &str,
        provider: &str,
        key: &str,
        status: u16,
        dur_ms: u64,
        stream: bool,
        body: &[u8],
        total: u64,
        completed: bool,
    ) {
        self.append(json_record(
            now_ms(),
            "resp",
            model,
            provider,
            key,
            None,
            Some(status),
            Some(dur_ms),
            stream,
            &body_str(body, total),
            Some(completed),
        ));
    }

    /// Ответа не было: транспортная ошибка до/внутри апстрима.
    pub fn transport_error(&self, model: &str, provider: &str, key: &str, reason: &str, dur_ms: u64) {
        self.append(json_record(
            now_ms(),
            "resp",
            model,
            provider,
            key,
            None,
            Some(0),
            Some(dur_ms),
            false,
            reason,
            Some(false),
        ));
    }

    fn append(&self, record: serde_json::Value) {
        let _io = self.io.lock().unwrap_or_else(|p| p.into_inner());
        if std::fs::create_dir_all(&self.root).is_err() {
            self.warn_once();
            return;
        }
        let ts = now_ms();
        let day = &iso_utc(ts)[..10];
        let day_file = self.root.join(format!("dump_{day}.jsonl"));
        self.rotate_parts_if_needed(&day_file);
        let line = record.to_string();
        let result = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&day_file)
            .and_then(|mut f| writeln!(f, "{line}"));
        if result.is_err() {
            self.warn_once();
        }
    }

    fn rotate_parts_if_needed(&self, day_file: &Path) {
        let size = match std::fs::metadata(day_file) {
            Ok(m) => m.len(),
            Err(_) => return, // файла нет — ротировать нечего
        };
        if size > ROTATE_BYTES {
            self.rotate_parts(day_file);
        }
    }

    /// Чистый сдвиг частей (без гейта по размеру — гейт в вызывающем коде).
    fn rotate_parts(&self, day_file: &Path) {
        // Старшая часть удаляется, остальные сдвигаются вверх, файл дня
        // становится `.1`. Отсутствие промежуточных частей — не ошибка.
        let _ = std::fs::remove_file(part_path(day_file, MAX_PARTS));
        for p in (1..MAX_PARTS).rev() {
            let _ = std::fs::rename(part_path(day_file, p), part_path(day_file, p + 1));
        }
        let _ = std::fs::rename(day_file, part_path(day_file, 1));
    }

    fn warn_once(&self) {
        if !self.warned.swap(true, Ordering::Relaxed) {
            log::warn!(
                "cloud-routers-gateway: дамп тел недоступен ({}), продолжаем без него",
                self.root.display()
            );
        }
    }
}

fn part_path(day_file: &Path, part: u32) -> PathBuf {
    let mut name = day_file.file_name().expect("имя файла есть").to_os_string();
    name.push(format!(".{part}"));
    day_file.with_file_name(name)
}

/// Контекст дампа одного запроса: кто, куда, с какого момента считаем время.
#[derive(Clone)]
pub struct DumpCtx {
    pub dumper: std::sync::Arc<Dumper>,
    pub model: String,
    pub provider: String,
    pub key: String,
    pub started: Instant,
}

/// Tee-обёртка стрима апстрима: клиенту — байт в байт как раньше, в буфер —
/// копия для дампа. Запись happens once: на `None` (доливка) или в `Drop`
/// (клиент отвалился / ошибка середины потока) — с честным `completed`.
pub struct TeeBody<S> {
    inner: std::pin::Pin<Box<S>>,
    buf: Vec<u8>,
    total: u64,
    status: u16,
    ctx: DumpCtx,
    completed: bool,
    written: bool,
}

impl<S> TeeBody<S> {
    pub fn new(inner: S, status: u16, ctx: DumpCtx) -> Self {
        Self {
            inner: Box::pin(inner),
            buf: Vec::new(),
            total: 0,
            status,
            ctx,
            completed: false,
            written: false,
        }
    }

    fn finish(&mut self, completed: bool) {
        if self.written {
            return;
        }
        self.written = true;
        self.completed = completed;
        let dur = self.ctx.started.elapsed().as_millis() as u64;
        self.ctx.dumper.response(
            &self.ctx.model,
            &self.ctx.provider,
            &self.ctx.key,
            self.status,
            dur,
            true,
            &self.buf,
            self.total,
            completed,
        );
    }
}

impl<S> Stream for TeeBody<S>
where
    S: Stream<Item = Result<axum::body::Bytes, reqwest::Error>>,
{
    type Item = S::Item;

    fn poll_next(mut self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        match this.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                this.total += chunk.len() as u64;
                if this.buf.len() < BODY_CAP_BYTES {
                    let take = (BODY_CAP_BYTES - this.buf.len()).min(chunk.len());
                    this.buf.extend_from_slice(&chunk[..take]);
                }
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(Some(Err(e))) => {
                this.finish(false);
                Poll::Ready(Some(Err(e)))
            }
            Poll::Ready(None) => {
                this.finish(true);
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<S> Drop for TeeBody<S> {
    fn drop(&mut self) {
        self.finish(self.completed);
    }
}

/// Query отбрасывается: у части провайдеров ключ живёт в query-параметре.
fn strip_query(url: &str) -> String {
    let no_query = url.split('?').next().unwrap_or(url);
    no_query.trim_end_matches('?').to_string()
}

/// Тело строкой: кап + честная пометка об обрезке.
fn body_str(body: &[u8], total: u64) -> String {
    let mut s = String::from_utf8_lossy(&body[..body.len().min(BODY_CAP_BYTES)]).into_owned();
    if total > BODY_CAP_BYTES as u64 {
        s.push_str(&format!("\n[обрезано, полный размер {total} байт]"));
    }
    s
}

fn json_record(
    ts_ms: u64,
    dir: &str,
    model: &str,
    provider: &str,
    key: &str,
    url: Option<String>,
    status: Option<u16>,
    dur_ms: Option<u64>,
    stream: bool,
    body: &str,
    completed: Option<bool>,
) -> serde_json::Value {
    let mut v = serde_json::json!({
        "ts": iso_utc(ts_ms),
        "dir": dir,
        "model": model,
        "provider": provider,
        "key": key,
        "status": status,
        "dur_ms": dur_ms,
        "stream": stream,
        "body": body,
    });
    if let Some(url) = url {
        v["url"] = serde_json::Value::String(url);
    }
    if let Some(completed) = completed {
        v["completed"] = serde_json::Value::Bool(completed);
    }
    v
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

/// ISO 8601 UTC с миллисекундами, без внешних зависимостей.
/// Алгоритм дней — civil_from_days (Howard Hinnant).
fn iso_utc(ms: u64) -> String {
    let secs = ms / 1000;
    let millis = ms % 1000;
    let days = (secs / 86_400) as i64;
    let tod = secs % 86_400;
    let (h, m, s) = (tod / 3_600, (tod % 3_600) / 60, tod % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mth = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mth <= 2 { y + 1 } else { y };
    format!("{y:04}-{mth:02}-{d:02}T{h:02}:{m:02}:{s:02}.{millis:03}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_formats_epoch_and_known_date() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00.000Z");
        // 2026-01-01 00:00:00 UTC = 1 767 225 600 с.
        assert_eq!(iso_utc(1_767_225_600_000), "2026-01-01T00:00:00.000Z");
        // Миллисекунды не теряются.
        assert_eq!(iso_utc(1_767_225_600_123), "2026-01-01T00:00:00.123Z");
    }

    #[test]
    fn body_cap_appends_honest_marker() {
        let big = vec![b'x'; BODY_CAP_BYTES * 2];
        let s = body_str(&big, big.len() as u64);
        assert!(s.len() > BODY_CAP_BYTES, "кап+пометка больше капа");
        assert!(s.contains("[обрезано, полный размер"), "{s}");
        assert!(s.ends_with(&format!("{} байт]", big.len())));
        // Маленькое тело не трогается.
        assert_eq!(body_str(b"ok", 2), "ok");
    }

    #[test]
    fn strip_query_removes_secret_param() {
        assert_eq!(strip_query("https://host/v1/chat?key=SECRET&x=1"), "https://host/v1/chat");
        assert_eq!(strip_query("https://host/v1/chat"), "https://host/v1/chat");
    }

    fn tmp_root(tag: &str) -> (PathBuf, Dumper) {
        let d = std::env::temp_dir().join(format!("cr_gw_dump_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        (d.clone(), Dumper::new(&d))
    }

    #[test]
    fn request_and_response_lands_as_jsonl_pair() {
        let (dir, dumper) = tmp_root("pair");
        dumper.request("m", "p", "k", "https://host/v1/chat?api_key=SECRET", false, br#"{"q":1}"#);
        dumper.response("m", "p", "k", 200, 42, false, br#"{"a":2}"#, 6, true);
        let files: Vec<PathBuf> = std::fs::read_dir(dir.join("logs"))
            .expect("logs")
            .map(|e| e.expect("e").path())
            .collect();
        assert_eq!(files.len(), 1, "один файл дня");
        let text = std::fs::read_to_string(&files[0]).expect("read");
        let lines: Vec<serde_json::Value> =
            text.lines().map(|l| serde_json::from_str(l).expect("jsonl")).collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["dir"], "req");
        assert_eq!(lines[0]["url"], "https://host/v1/chat", "query с секретом не пишется");
        assert_eq!(lines[0]["body"], "{\"q\":1}");
        assert_eq!(lines[0]["status"], serde_json::Value::Null);
        assert_eq!(lines[1]["dir"], "resp");
        assert_eq!(lines[1]["status"], 200);
        assert_eq!(lines[1]["dur_ms"], 42);
        assert_eq!(lines[1]["completed"], true);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dump_never_contains_secret() {
        // Секрет передаётся в заголовке, а заголовки не дампятся вовсе; даже
        // если провайдер кладёт его в URL — strip_query срезает query.
        let (dir, dumper) = tmp_root("secret");
        dumper.request("m", "p", "k1", "https://host/v1/chat?api_key=sk-TOPSECRET", false, b"{}");
        dumper.transport_error("m", "p", "k1", "connection refused", 5);
        let text = std::fs::read_to_string(dir.join("logs").read_dir().expect("logs").next().expect("f").expect("p").path())
            .expect("read");
        assert!(!text.contains("sk-TOPSECRET"), "секрет утёк: {text}");
        let err: serde_json::Value = serde_json::from_str(text.lines().nth(1).expect("line")).expect("jsonl");
        assert_eq!(err["status"], 0, "транспортная ошибка — статус 0");
        assert_eq!(err["body"], "connection refused");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rotation_shifts_parts_and_drops_oldest() {
        let (dir, _d) = tmp_root("rot");
        let dumper = Dumper::new(&dir);
        let logs = dir.join("logs");
        std::fs::create_dir_all(&logs).expect("mkdir");
        let day = logs.join("dump_2026-11-10.jsonl");
        std::fs::write(&day, "current").expect("write");
        for p in 1..MAX_PARTS {
            std::fs::write(part_path(&day, p), format!("part{p}")).expect("write");
        }
        std::fs::write(part_path(&day, MAX_PARTS), "part5-oldest").expect("write");
        dumper.rotate_parts(&day);
        assert_eq!(
            std::fs::read_to_string(part_path(&day, MAX_PARTS)).expect("p5"),
            "part4",
            "старший контент вытеснен: .5 теперь содержит старую .4"
        );
        assert_eq!(std::fs::read_to_string(part_path(&day, 1)).expect("p1"), "current");
        assert_eq!(std::fs::read_to_string(part_path(&day, 2)).expect("p2"), "part1");
        assert_eq!(std::fs::read_to_string(part_path(&day, 5)).expect("p5"), "part4");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
