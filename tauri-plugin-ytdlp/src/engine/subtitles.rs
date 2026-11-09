//! Субтитры YouTube: подготовка транскрипта (yt-dlp → VTT-парсер → чистка
//! спонсорских → чанки) и выдача чанков.
//!
//! SSOT-механика перенесена 1:1 из хоста (ранее Deno MCP `youtube_mcp.ts`;
//! перенос — PRP «ytdlp субтитры SSOT»): парсер VTT, вырезание спонсорских
//! по таймкодам, дедуп строк и чанкование НЕ изменены. Отличия только в
//! механике исполнения: yt-dlp — управляемый плагином бинарь (dir параметр),
//! sponsor.ajay.app — через reqwest (с таймаутом 10с, у curl его не было),
//! кеш чанков — процессное состояние модуля (переживает между вызовами
//! тулов, умирает с процессом — как прежний in-memory кеш Deno-процесса).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use regex::Regex;
use serde::Serialize;

use super::ytdlp_manager;

/// Результат `prepare_transcript`.
#[derive(Serialize, Clone, Debug)]
pub struct PreparedTranscript {
    pub video_id: String,
    pub chunks_total: usize,
    /// true — транскрипт уже был в кеше (повторный prepare того же url).
    pub cached: bool,
}

/// Результат `get_transcript_chunk`.
#[derive(Serialize, Clone, Debug)]
pub struct TranscriptChunk {
    /// 1-based номер чанка.
    pub index: usize,
    pub chunks_total: usize,
    pub text: String,
}

/// Ошибка субтитров: класс несёт потребителю честный маппинг
/// (прецедент — `SearchError { Input, Network }` крейта web-tools).
/// Display — только причина; классифицирующий префикс добавляет хост
/// (ToolError), UI-команды берут `message()`.
#[derive(Debug, Clone)]
pub enum SubtitlesError {
    /// Аргументная проблема: URL без ID видео, нет prepare, неверный индекс.
    Usage(String),
    /// Состояние источника: yt-dlp не установлен, субтитры не нашлись.
    NotFound(String),
    /// Инфраструктура: temp-каталог, недоступный кеш.
    Io(String),
}

impl SubtitlesError {
    /// Текст причины без префикса класса.
    pub fn message(&self) -> &str {
        match self {
            SubtitlesError::Usage(m) | SubtitlesError::NotFound(m) | SubtitlesError::Io(m) => m,
        }
    }
}

impl std::fmt::Display for SubtitlesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message())
    }
}

/// Размер части по умолчанию (рекомендация в промпте агента — 12000).
pub const DEFAULT_CHUNK_SIZE: usize = 12000;

const SPONSOR_API: &str = "https://sponsor.ajay.app/api/skipSegments";
/// Категории вырезания — дословно прежние (sponsor/selfpromo/interaction).
const SPONSOR_CATEGORIES: &str = "[\"sponsor\",\"selfpromo\",\"interaction\"]";
const SPONSOR_TIMEOUT_SECS: u64 = 10;
const CACHE_BROKEN: &str = "Кэш транскриптов недоступен (повреждён лок-контейнер)";

fn cache() -> &'static Mutex<HashMap<String, Vec<String>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Vec<String>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_seq() -> usize {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    SEQ.fetch_add(1, Ordering::Relaxed)
}

/// ID видео из ссылки — дословный порт TS-регэкспа
/// `(?:v=|youtu\.be\/|embed\/|shorts\/)([a-zA-Z0-9_-]{11})`.
pub fn extract_video_id(url: &str) -> Option<String> {
    let re = Regex::new(r"(?:v=|youtu\.be/|embed/|shorts/)([a-zA-Z0-9_-]{11})").ok()?;
    re.captures(url).map(|c| c[1].to_string())
}

/// Подготовить транскрипт: кеш → спонсорские → yt-dlp → VTT → чанки.
///
/// `dir` — каталог движка (бинать yt-dlp: `get_exe_path(dir)`); при
/// отсутствии бинаря — честная ошибка с подсказкой установки.
pub fn prepare_transcript(
    dir: &Path,
    url: &str,
    chunk_size: usize,
    cookies_file: Option<&Path>,
) -> Result<PreparedTranscript, SubtitlesError> {
    let chunk_size = if chunk_size == 0 { DEFAULT_CHUNK_SIZE } else { chunk_size };
    {
        let map = cache().lock().map_err(|_| SubtitlesError::Io(CACHE_BROKEN.into()))?;
        if let Some(chunks) = map.get(url) {
            return Ok(PreparedTranscript {
                video_id: extract_video_id(url).unwrap_or_default(),
                chunks_total: chunks.len(),
                cached: true,
            });
        }
    }

    let video_id = extract_video_id(url)
        .ok_or_else(|| SubtitlesError::Usage("Не удалось извлечь ID видео.".into()))?;
    let sponsors = fetch_sponsors(&video_id);
    let vtt = download_vtt(dir, url, cookies_file)?;
    let clean = parse_vtt(&vtt, &sponsors);
    let chunks = chunk_text(&clean, chunk_size);

    let mut map = cache().lock().map_err(|_| SubtitlesError::Io(CACHE_BROKEN.into()))?;
    map.insert(url.to_string(), chunks.clone());
    Ok(PreparedTranscript {
        video_id,
        chunks_total: chunks.len(),
        cached: false,
    })
}

/// Текст чанка (1-based index) — дословный порт TS `getChunk`.
pub fn get_transcript_chunk(url: &str, index: usize) -> Result<TranscriptChunk, SubtitlesError> {
    let map = cache().lock().map_err(|_| SubtitlesError::Io(CACHE_BROKEN.into()))?;
    let Some(chunks) = map.get(url) else {
        return Err(SubtitlesError::Usage(
            "Видео не найдено в кэше. Сначала вызови prepare_video.".into(),
        ));
    };
    if index < 1 || index > chunks.len() {
        return Err(SubtitlesError::Usage(format!(
            "Неверный индекс. Доступны части от 1 до {}.",
            chunks.len()
        )));
    }
    Ok(TranscriptChunk {
        index,
        chunks_total: chunks.len(),
        text: chunks[index - 1].clone(),
    })
}

/// Спонсорские сегменты [start_sec, end_sec]. Сеть/парсинг недоступны →
/// пусто (паритет TS: ошибки глотались, видео выдавалось без вырезания).
fn fetch_sponsors(video_id: &str) -> Vec<(f64, f64)> {
    let client = match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(SPONSOR_TIMEOUT_SECS))
        .build()
    {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let segments: Vec<serde_json::Value> = match client
        .get(SPONSOR_API)
        .query(&[("videoID", video_id), ("categories", SPONSOR_CATEGORIES)])
        .send()
    {
        Ok(resp) => match resp.text() {
            Ok(text) => {
                if text.contains("Not Found") {
                    return Vec::new();
                }
                serde_json::from_str(&text).unwrap_or_default()
            }
            Err(_) => return Vec::new(),
        },
        Err(_) => return Vec::new(),
    };
    segments
        .into_iter()
        .filter_map(|item| {
            let seg = item.get("segment")?;
            let start = seg.get(0)?.as_f64()?;
            let end = seg.get(1)?.as_f64()?;
            Some((start, end))
        })
        .collect()
}

/// Скачать субтитры через yt-dlp: temp-каталог → запуск → найти .vtt →
/// прочитать → удалить каталог. Ошибки запуска глотаются (паритет TS):
/// гейт успеха — наличие .vtt; но отсутствие бинаря — честная ошибка.
fn download_vtt(dir: &Path, url: &str, cookies_file: Option<&Path>) -> Result<String, SubtitlesError> {
    let exe = ytdlp_manager::get_exe_path(dir);
    if !ytdlp_manager::is_installed(dir) {
        return Err(SubtitlesError::NotFound(format!(
            "yt-dlp не установлен (ожидался {}). Установите его в разделе «Движки» приложения (кнопка установки yt-dlp).",
            exe.display()
        )));
    }

    let tmp: PathBuf = std::env::temp_dir()
        .join(format!("yt-{}-{}", std::process::id(), next_seq()));
    std::fs::create_dir_all(&tmp)
        .map_err(|e| SubtitlesError::Io(format!("Не удалось создать temp-каталог: {e}")))?;
    let out_tmpl = tmp.join("%(id)s.%(ext)s");

    let mut cmd = Command::new(&exe);
    if let Some(cookies) = cookies_file {
        cmd.arg("--cookies").arg(cookies);
    }
    cmd.args([
        "--skip-download",
        "--write-auto-subs",
        "--write-subs",
        "--sub-langs",
        "ru.*,en.*,ru,en",
        "--convert-subs",
        "vtt",
        "--output",
    ])
    .arg(&out_tmpl)
    .arg(url)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null());
    no_window(&mut cmd);
    let _ = cmd.status(); // паритет TS (stdio ignore): гейт — наличие .vtt

    let mut vtt_text = None;
    if let Ok(entries) = std::fs::read_dir(&tmp) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("vtt") {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    vtt_text = Some(text);
                    break;
                }
            }
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    vtt_text.ok_or_else(|| SubtitlesError::NotFound("Субтитры не найдены.".into()))
}

fn no_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}

/// VTT → текст `[M:SS] строка\n` с вырезанием спонсорских и дедупом
/// подряд идущих одинаковых строк. Дословный порт TS-цикла.
fn parse_vtt(vtt: &str, sponsors: &[(f64, f64)]) -> String {
    let mut clean = String::new();
    let mut current_start: u64 = 0;
    // Владеющий дедуп-буфер (строка умирает в конце итерации — borrow не продержать).
    let mut last_text: Option<String> = None;

    for raw_line in vtt.split('\n') {
        let line = raw_line.trim();
        // Первый таймкод в строке вида HH:MM:SS.mmm обновляет currentStart.
        if let Some(secs) = first_timecode_secs(line) {
            current_start = secs;
            continue;
        }
        if !line.is_empty()
            && !line.contains("WEBVTT")
            && !line.contains("-->")
            && !line.bytes().all(|b| b.is_ascii_digit())
        {
            let is_sponsor = sponsors
                .iter()
                .any(|&(start, end)| (current_start as f64) >= start && (current_start as f64) <= end);
            if !is_sponsor {
                // Снять inline-теги VTT (<c>, <00:00:01.000> и т.п.).
                let text = strip_tags(line);
                let text = text.trim();
                if !text.is_empty() && last_text.as_deref() != Some(text) {
                    let m = current_start / 60;
                    let s = format!("{:02}", current_start % 60);
                    clean.push_str(&format!("[{m}:{s}] {text}\n"));
                    last_text = Some(text.to_string());
                }
            }
        }
    }
    clean
}

/// Первый таймкод `HH:MM:SS.mmm` в строке → секунды (доли секунды
/// отбрасываются, паритет parseInt в TS). Нет — None.
fn first_timecode_secs(line: &str) -> Option<u64> {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 12 <= bytes.len() {
        if bytes[i].is_ascii_digit()
            && bytes[i + 1].is_ascii_digit()
            && bytes[i + 2] == b':'
            && bytes[i + 3].is_ascii_digit()
            && bytes[i + 4].is_ascii_digit()
            && bytes[i + 5] == b':'
            && bytes[i + 6].is_ascii_digit()
            && bytes[i + 7].is_ascii_digit()
            && bytes[i + 8] == b'.'
            && (i + 11 < bytes.len()
                && bytes[i + 9].is_ascii_digit()
                && bytes[i + 10].is_ascii_digit()
                && bytes[i + 11].is_ascii_digit())
        {
            let h: u64 = line[i..i + 2].parse().ok()?;
            let m: u64 = line[i + 3..i + 5].parse().ok()?;
            let s: u64 = line[i + 6..i + 8].parse().ok()?;
            return Some(h * 3600 + m * 60 + s);
        }
        i += 1;
    }
    None
}

/// `line.replace(/<[^>]+>/g, '')` — снятие тегов.
fn strip_tags(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_tag = false;
    for ch in line.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// Разбивка на чанки по символам: накопление строк до `chunk_size`
/// (паритет TS-цикла, `.length` → счёт символов Unicode).
fn chunk_text(clean: &str, chunk_size: usize) -> Vec<String> {
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;
    for line in clean.split('\n') {
        let line_len = line.chars().count();
        if current_len + line_len > chunk_size && current_len > 0 {
            chunks.push(std::mem::take(&mut current));
            current_len = 0;
        }
        current.push_str(line);
        current.push('\n');
        current_len += line_len + 1;
    }
    if !current.trim().is_empty() {
        chunks.push(current);
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYNTHETIC_VTT: &str = "WEBVTT\n\n\
00:00:05.000 --> 00:00:08.000\n\
Первый заголовок\n\n\
00:00:10.000 --> 00:00:12.000\n\
Второй заголовок\n\n\
00:00:30.000 --> 00:00:34.000\n\
<c>Спонсор</c> предложения\n\n\
00:01:05.000 --> 00:01:07.000\n\
Дубль\n\n\
00:01:10.000 --> 00:01:12.000\n\
Дубль\n\n\
00:01:20.000 --> 00:01:25.000\n\
Финал\n";

    #[test]
    fn parse_vtt_strips_tags_dedups_and_formats_timecodes() {
        let out = parse_vtt(SYNTHETIC_VTT, &[]);
        assert!(out.contains("[0:05] Первый заголовок\n"), "got: {out}");
        assert!(out.contains("[0:10] Второй заголовок\n"), "got: {out}");
        assert!(out.contains("[1:20] Финал\n"), "got: {out}");
        // Теги сняты (<c>…</c>), текст строки дошёл до чистки (без спонсоров).
        assert!(out.contains("[0:30] Спонсор предложения\n"), "got: {out}");
        // Строка С inline-таймкодом <00:00:30.500> трактуется как таймкод-строка
        // и пропускается целиком (паритет TS-регэкспа, ловящего таймкод в любой позиции).
        let with_inline_ts = "00:00:01.000 --> 00:00:02.000\n<00:00:01.500>текст\n";
        let out2 = parse_vtt(with_inline_ts, &[]);
        assert_eq!(out2, "");
        // Дедуп: два подряд «Дубль» → один.
        assert_eq!(out.matches("Дубль").count(), 1);
        assert!(!out.contains("WEBVTT"));
    }

    #[test]
    fn sponsor_window_cuts_lines_by_timecode() {
        let out = parse_vtt(SYNTHETIC_VTT, &[(25.0, 40.0)]);
        assert!(!out.contains("Спонсор"), "спонсорский сегмент обязан вырезаться: {out}");
        assert!(out.contains("Первый заголовок"));
        assert!(out.contains("Финал"));
    }

    #[test]
    fn timecode_extraction_matches_ts_regex() {
        assert_eq!(extract_video_id("https://www.youtube.com/watch?v=dQw4w9WgXcQ").as_deref(), Some("dQw4w9WgXcQ"));
        assert_eq!(extract_video_id("https://youtu.be/dQw4w9WgXcQ").as_deref(), Some("dQw4w9WgXcQ"));
        assert_eq!(extract_video_id("https://www.youtube.com/shorts/dQw4w9WgXcQ").as_deref(), Some("dQw4w9WgXcQ"));
        assert_eq!(extract_video_id("https://www.youtube.com/embed/dQw4w9WgXcQ").as_deref(), Some("dQw4w9WgXcQ"));
        assert!(extract_video_id("https://example.com/nothing-here").is_none());
        assert!(extract_video_id("https://youtu.be/короткий").is_none());
    }

    #[test]
    fn chunking_splits_by_char_budget_and_keeps_order() {
        let lines: String = (0..10)
            .map(|i| format!("[{i}:00] строка-{}\n", "х".repeat(50)))
            .collect();
        let chunks = chunk_text(&lines, 200);
        // Строка = 62 символа; 3 строки = 189 ≤ 200, 4-я даёт 251 > 200 → чанки по 3 строки.
        assert_eq!(chunks.len(), 4, "10 строк при бюджете 200 → чанки по 3 строки (3+3+3+1), got {}", chunks.len());
        assert!(chunks[0].starts_with("[0:00]"));
        assert!(chunks[3].contains("[9:00]"), "последняя строка — в 4-м чанке: {:?}", chunks[3]);
    }

    #[test]
    fn chunk_of_empty_text_is_empty_vec() {
        assert!(chunk_text("", 100).is_empty());
        assert!(chunk_text("\n", 100).is_empty());
    }

    #[test]
    fn get_chunk_without_prepare_is_honest_error() {
        let err = get_transcript_chunk("https://youtu.be/aaaaaaaaaaa-not-prepared", 1)
            .unwrap_err();
        assert!(matches!(err, SubtitlesError::Usage(_)));
        assert!(err.message().contains("Сначала вызови prepare_video"), "got: {err}");
    }

    #[test]
    fn prepare_without_ytdlp_is_honest_error_with_hint() {
        let dir = std::env::temp_dir().join(format!("ytdlp-missing-{}", std::process::id()));
        let err = prepare_transcript(&dir, "https://youtu.be/dQw4w9WgXcQ", 12_000, None)
            .unwrap_err();
        assert!(matches!(err, SubtitlesError::NotFound(_)), "got: {err:?}");
        assert!(err.message().contains("yt-dlp не установлен"), "got: {err}");
        assert!(err.message().contains("Движки"), "подсказка установки обязана быть в ошибке: {err}");
    }
}
