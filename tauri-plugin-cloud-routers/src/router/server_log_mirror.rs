//! Зеркало `server.log` node-роутеров в лог приложения-хоста.
//!
//! ## Зачем
//!
//! Причины отказов облачных запросов (502 ECONNRESET, «high demand» 503,
//! `terminated`-обрывы уже начатых стримов) живут в консоли/`server.log`
//! самого роутера — хостовый `king_orch.log` об этом молчал. Инцидент
//! 25.10.2026: чтобы диагностировать «пустой ответ 200» у юзера, владельцу
//! пришлось отдельно просить выгрузку логов 9router. Теперь значимые строки
//! журнала роутера дублируются в лог хоста с префиксом `9router:` —
//! диагностика помещается в одну выгрузку.
//!
//! ## Как
//!
//! Фоновый поток раз в [`POLL_INTERVAL`] дочитывает прирост `server.log`
//! каждого node-роутера (stdout/stderr процесса редиректятся туда в
//! `process.rs::start_server`) и пишет значимые строки через `log::`
//! (единый логгер хоста). Первый взгляд пропускает бэклог (стартовый шум не
//! нужен, история — в самом `server.log`); рестарт роутера обнуляет файл —
//! это детектится усечением, прирост после него зеркалится с нуля.
//! Ошибки чтения молчаливы: файл вправе отсутствовать — это норма.

use std::io::{Read, Seek, SeekFrom};
use std::time::Duration;

use tauri::AppHandle;

use crate::router::config;
use crate::spec::RouterId;

/// Пауза между дочитками (как у gateway-зеркала: 3 с).
const POLL_INTERVAL: Duration = Duration::from_secs(3);

/// Максимум символов одной зеркалируемой строки: диагностика, не дамп.
const MAX_LINE_CHARS: usize = 500;

/// Позиции чтения по индексу роутера. `u64::MAX` — «ещё не синхронизирован»:
/// первый взгляд на файл пропускает бэклог. Gateway сюда не входит — у него
/// свой канал событий (`gateway_events.rs`).
static POSITIONS: std::sync::Mutex<[u64; 4]> = std::sync::Mutex::new([u64::MAX; 4]);

/// Значимая строка журнала роутера → уровень лога хоста. Остальное — шум
/// (aptabase-поллинг, modelCatalog, INFO-строки старта): оно не должно
/// топить полезное в логе хоста.
pub fn classify_line(line: &str) -> Option<log::Level> {
    // Обрывы и отказы: причина инцидентов живёт именно в этих строках.
    if line.contains("terminated")
        || line.contains("ERROR")
        || line.contains("✗")
        || line.contains("❌")
        || line.contains("⚠️")
        || line.contains("FALLBACK")
        || line.contains("UNAVAILABLE")
    {
        return Some(log::Level::Warn);
    }
    // Метрики завершённого запроса (TTFT/IN/OUT) и переключения комбо — info.
    if line.contains("DONE") || line.contains("[COMBO]") {
        return Some(log::Level::Info);
    }
    None
}

/// Позиция после сверки с текущей длиной файла: усечение (рестарт роутера
/// обнуляет `server.log`) сбрасывает позицию — прирост новой сессии
/// зеркалится с нуля.
pub fn adjusted_position(position: u64, file_len: u64) -> u64 {
    if file_len < position {
        file_len
    } else {
        position
    }
}

/// Чистое ядро тейла: по позиции и приросту байтов решает, какие ПОЛНЫЕ
/// строки готовы к зеркалу и какой будет новая позиция. Неполный хвост
/// (последняя строка без `\n`) остаётся на следующий тик — строки не
/// рвутся границей чанка. Позиция — байтовая, поэтому двигается только по
/// границу полной строки.
pub fn tail_plan(position: u64, chunk: &[u8]) -> (u64, Vec<String>) {
    let Some(last_nl) = chunk.iter().rposition(|&b| b == b'\n') else {
        return (position, Vec::new());
    };
    let complete = &chunk[..last_nl];
    let lines = String::from_utf8_lossy(complete)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    (position + last_nl as u64 + 1, lines)
}

/// Обрезка длинной строки: тело 503-ошибки Google полезно, но не целиком.
fn clamp_line(line: &str) -> String {
    if line.chars().count() <= MAX_LINE_CHARS {
        line.to_string()
    } else {
        let cut: String = line.chars().take(MAX_LINE_CHARS).collect();
        format!("{cut}…")
    }
}

/// Запустить цикл зеркала (один раз на процесс хоста).
pub fn spawn_mirror(app: &AppHandle) {
    let handle = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(POLL_INTERVAL);
        poll_once(&handle);
    });
}

/// Один тик: дочитать прирост каждого node-роутера и зеркалить значимое.
/// Любая неудача — тихий пропуск: роутер вправе быть не установлен и не
/// запущен, файл — отсутствовать; следующий тик повторит.
fn poll_once(app: &AppHandle) {
    let mut positions = POSITIONS.lock().unwrap_or_else(|p| p.into_inner());
    for router in RouterId::ALL {
        if !router.is_node_bundle() {
            continue;
        }
        let dir = config::router_dir(app, *router);
        let path = dir.join("server.log");
        let Ok(mut file) = std::fs::File::open(&path) else {
            continue;
        };
        let Ok(meta) = file.metadata() else {
            continue;
        };
        let len = meta.len();
        let slot = &mut positions[router.index()];
        if *slot == u64::MAX {
            // Первый взгляд: бэклог не зеркалим — стартовый шум не нужен.
            *slot = len;
            continue;
        }
        *slot = adjusted_position(*slot, len);
        if len == *slot {
            continue;
        }
        if file.seek(SeekFrom::Start(*slot)).is_err() {
            continue;
        }
        let mut chunk = Vec::new();
        if file.read_to_end(&mut chunk).is_err() {
            continue;
        }
        let (new_pos, lines) = tail_plan(*slot, &chunk);
        *slot = new_pos;
        for line in lines {
            if let Some(level) = classify_line(&line) {
                let text = clamp_line(&line);
                match level {
                    log::Level::Warn => log::warn!("{}: {}", router.as_str(), text),
                    log::Level::Error => log::error!("{}: {}", router.as_str(), text),
                    _ => log::info!("{}: {}", router.as_str(), text),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Значимые строки инцидента 25.10.2026 (реальная консоль 9router):
    /// обрывы, фолбэки, метрики — зеркалятся; INFO-шум — нет.
    #[test]
    fn classify_line_matches_incident_console() {
        assert_eq!(
            classify_line("[18:01:22] ⚪ ✗ ERROR 502 · gemini/gemini-3.8-flash · 54588ms"),
            Some(log::Level::Warn)
        );
        assert_eq!(
            classify_line("TypeError: terminated at Fetch.onAborted (node:internal/deps/undici)"),
            Some(log::Level::Warn)
        );
        assert_eq!(
            classify_line("[18:02:13] ⚠️ [FALLBACK] ⇄ ACC:vbvbbvbv UNAVAILABLE (502) → NEXT ACCOUNT"),
            Some(log::Level::Warn)
        );
        assert_eq!(
            classify_line("❌ gemini [502]: [502]: fetch failed (cause: ECONNRESET)"),
            Some(log::Level::Warn)
        );
        assert_eq!(
            classify_line("[18:04:44] ⚪ 📊 DONE 12640ms · TTFT 12490ms · IN 2066 · OUT 872"),
            Some(log::Level::Info)
        );
        assert_eq!(
            classify_line("[18:02:13] ℹ️ [COMBO] Trying model 2/3: gemini/gemini-3.7-flash"),
            Some(log::Level::Info)
        );
        assert_eq!(classify_line("[modelCatalog] 5513 models, 25 providers, 309.3KB"), None);
        assert_eq!(classify_line("[18:00:27] ℹ️ [CHAT] Combo \"google-high-tier\" with 3 models"), None);
    }

    /// Полные строки зеркалятся, неполный хвост ждёт следующего тика
    /// (позиция не двигается мимо него — хвост перечитается вместе с приростом).
    #[test]
    fn tail_plan_takes_only_complete_lines() {
        let chunk = b"first line\nsecond line\npartial tail without newli";
        let (pos, lines) = tail_plan(100, chunk);
        assert_eq!(pos, 100 + b"first line\nsecond line\n".len() as u64);
        assert_eq!(lines, vec!["first line".to_string(), "second line".to_string()]);

        // Файл дописан: чтение с позиции вернёт непоглощённый хвост + прирост.
        let mut combined = chunk[(pos - 100) as usize..].to_vec();
        combined.extend_from_slice(b"ne and more\n");
        let (pos2, lines2) = tail_plan(pos, &combined);
        assert_eq!(pos2, pos + combined.len() as u64);
        assert_eq!(lines2, vec!["partial tail without newline and more".to_string()]);
    }

    /// Прирост без перевода строки и пустой прирост — позиция не двигается.
    #[test]
    fn tail_plan_without_newline_keeps_position() {
        assert_eq!(tail_plan(7, b""), (7, Vec::<String>::new()));
        assert_eq!(tail_plan(7, b"still no newline"), (7, Vec::<String>::new()));
    }

    /// Рестарт роутера обнуляет server.log: усечение сбрасывает позицию,
    /// прирост новой сессии зеркалится с нуля.
    #[test]
    fn adjusted_position_resets_on_truncate() {
        assert_eq!(adjusted_position(1000, 40), 40);
        assert_eq!(adjusted_position(1000, 1000), 1000);
        assert_eq!(adjusted_position(1000, 2000), 1000);
    }

    /// Длинное тело ошибки не топит лог: обрезка с явным многоточием.
    #[test]
    fn clamp_line_cuts_long_bodies() {
        let long = "x".repeat(MAX_LINE_CHARS + 50);
        let clamped = clamp_line(&long);
        assert!(clamped.chars().count() <= MAX_LINE_CHARS + 1);
        assert!(clamped.ends_with('…'));
        assert_eq!(clamp_line("короткая"), "короткая");
    }
}
