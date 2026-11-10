//! Мониторинг сетевых ошибок yt-dlp: извлечение CDN-домена из строк «Got error»
//! и детект пропусков HLS-фрагментов.
//!
//! Фон: при блокировке/замедлении googlevideo CDN yt-dlp перемалывает ретраи по
//! каждому фрагменту (десятки минут «зависания»), а домен прячется в спаме
//! «Got error». Плагин поднимает домен в явную одной строкой и обрывает загрузку
//! после нескольких подряд пропущенных фрагментов.

use regex::Regex;

lazy_static::lazy_static! {
    /// `HTTPSConnectionPool(host='rr2---sn-ixh7rn76.googlevideo.com', port=443)`.
    static ref HOST_QUOTED_RE: Regex = Regex::new(r"host='([^']+)'").unwrap();
    /// `Connection to rr2---sn-…googlevideo.com timed out.`
    static ref HOST_TIMED_OUT_RE: Regex = Regex::new(r"Connection to ([\w.-]+) timed out").unwrap();
    /// `[download] fragment not found; Skipping fragment 12 ...`
    static ref SKIPPING_FRAGMENT_RE: Regex = Regex::new(r"Skipping fragment \d+").unwrap();
}

/// Извлекает домен из строки сетевой ошибки yt-dlp («Got error»/«ERROR»).
/// None — если в строке нет узнаваемого узла (503 без host, прогресс и т.п.).
pub fn extract_error_host(line: &str) -> Option<String> {
    if !line.contains("Got error") && !line.contains("ERROR") {
        return None;
    }
    if let Some(m) = HOST_QUOTED_RE.captures(line) {
        return Some(m[1].to_string());
    }
    HOST_TIMED_OUT_RE.captures(line).map(|m| m[1].to_string())
}

/// Строка «Skipping fragment N» (фрагмент не скачался после всех ретраев).
pub fn is_skipping_fragment(line: &str) -> bool {
    SKIPPING_FRAGMENT_RE.is_match(line)
}

/// CDN-домен фрагментов из метаданных yt-dlp (`--dump-json`).
///
/// Запасной источник домена: строки «HTTP Error 503: Service Unavailable»
/// не содержат host, и домен попадается в логе только когда CDN отвечает
/// таймаутом/reset. Для HLS у формата `fragment_base_url` указывает на
/// `rrN---sn-*.googlevideo.com`, для DASH — сам `url`. Метаданные идут через
/// youtube.com и доступны даже при заблокированном CDN.
pub fn cdn_host_from_info(info: &serde_json::Value) -> Option<String> {
    let formats = info.get("formats")?.as_array()?;
    for key in ["fragment_base_url", "url"] {
        for f in formats {
            let Some(u) = f.get(key).and_then(|v| v.as_str()) else { continue };
            let Some(rest) = u.strip_prefix("https://").or_else(|| u.strip_prefix("http://"))
            else {
                continue;
            };
            let end = rest.find('/').unwrap_or(rest.len());
            let host = &rest[..end];
            if host.ends_with("googlevideo.com") {
                return Some(host.to_string());
            }
        }
    }
    None
}

/// Текст явного предупреждения о домене (единая формулировка для stdout/stderr).
pub fn host_warning(host: &str) -> String {
    format!(
        "CDN-домен не отвечает: {host} — проверь его через обход блокировок. \
         Если обход — локальный прокси, укажи его в настройках yt-dlp."
    )
}

/// Текст ошибки, когда CDN-домен не отвечает (единая формулировка).
pub fn abort_message(host: Option<&str>) -> String {
    let host = host.unwrap_or("CDN-домен");
    format!(
        "Скачивание не удалось: {host} не отвечает (сеть или блокировка провайдера). \
         Настрой обход блокировок или укажи прокси в настройках yt-dlp."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // Реальные строки из test/last_logs.txt (см. задачу 10.11.26).
    const REAL_STDOUT_RETRY: &str = "[download] Got error: HTTPSConnectionPool(host='rr2---sn-ixh7rn76.googlevideo.com', port=443): Read timed out. (read timeout=60.0). Retrying (1/10)...";
    const REAL_STDOUT_RESET: &str = "[download] Got error: ('Connection aborted.', ConnectionResetError(10054, 'Удаленный хост принудительно разорвал соединение', None, 10054, None)). Retrying (1/10)...";
    const REAL_STDOUT_TIMEOUT2: &str = "[download] Got error: (<HTTPSConnection(host='rr2---sn-ixh7rn76.googlevideo.com', port=443) at 0x1bbdd337850>, 'Connection to rr2---sn-ixh7rn76.googlevideo.com timed out. (connect timeout=60.0)'). Retrying (2/10)...";
    const REAL_STDERR_GIVEUP: &str =
        "ERROR: [download] Got error: HTTP Error 503: Service Unavailable. Giving up after 10 retries";
    const REAL_SKIP: &str = "[download] fragment not found; Skipping fragment 12 ...";
    const REAL_PROGRESS: &str = "[download]  46.3% of ~ 434.16MiB at 7.26MiB/s ETA 00:41";
    const REAL_DEST: &str = r"[download] Destination: E:\Downloads\Drone shot.f299.mp4";

    #[test]
    fn extracts_host_from_real_log_lines() {
        assert_eq!(
            extract_error_host(REAL_STDOUT_RETRY),
            Some("rr2---sn-ixh7rn76.googlevideo.com".to_string())
        );
        // Reset-строка без host='...' — None (но не паникует)
        assert_eq!(extract_error_host(REAL_STDOUT_RESET), None);
        // В двойной обёртке берём первый host='...'
        assert_eq!(
            extract_error_host(REAL_STDOUT_TIMEOUT2),
            Some("rr2---sn-ixh7rn76.googlevideo.com".to_string())
        );
        // 503 «Giving up» без host — None
        assert_eq!(extract_error_host(REAL_STDERR_GIVEUP), None);
        // Прогресс/Destination не гейтятся как ошибки
        assert_eq!(extract_error_host(REAL_PROGRESS), None);
        assert_eq!(extract_error_host(REAL_DEST), None);
    }

    #[test]
    fn skipping_fragment_detection() {
        assert!(is_skipping_fragment(REAL_SKIP));
        assert!(!is_skipping_fragment(REAL_PROGRESS));
        assert!(!is_skipping_fragment(REAL_DEST));
    }

    #[test]
    fn messages_are_informative() {
        let w = host_warning("rr2---sn-ixh7rn76.googlevideo.com");
        assert!(w.contains("rr2---sn-ixh7rn76.googlevideo.com"));
        assert!(w.contains("обход"));
        let m = abort_message(Some("rr3---sn-ixh7rn76.googlevideo.com"));
        assert!(m.contains("rr3---sn-ixh7rn76.googlevideo.com"));
        assert!(m.contains("прокси"));
        let m2 = abort_message(None);
        assert!(m2.contains("CDN-домен"));
    }

    #[test]
    fn cdn_host_from_dump_json() {
        let info: serde_json::Value = serde_json::json!({
            "formats": [
                {"format_id": "625", "url": "https://manifest.googlevideo.com/api/manifest/hls_playlist/x"},
                {"format_id": "999", "fragment_base_url": "https://rr3---sn-xguxaxjvh-8v1s.googlevideo.com/videoplayback/x"}
            ]
        });
        // fragment_base_url приоритетнее url: именно он — хост фрагментов
        assert_eq!(
            cdn_host_from_info(&info),
            Some("rr3---sn-xguxaxjvh-8v1s.googlevideo.com".to_string())
        );
        let url_only: serde_json::Value =
            serde_json::json!({ "formats": [{"url": "https://rr2---sn-ixh7rn76.googlevideo.com/videoplayback/x"}] });
        assert_eq!(
            cdn_host_from_info(&url_only),
            Some("rr2---sn-ixh7rn76.googlevideo.com".to_string())
        );
        let no_cdn: serde_json::Value = serde_json::json!({ "formats": [{"url": "https://youtube.com/x"}] });
        assert_eq!(cdn_host_from_info(&no_cdn), None);
        let empty: serde_json::Value = serde_json::json!({});
        assert_eq!(cdn_host_from_info(&empty), None);
    }
}
