//! Зеркало журнала шлюза (`EventLog`) в лог приложения-хоста.
//!
//! ## Зачем
//!
//! Проверка ключей и отказы admin-API исполняются **в процессе шлюза**: их
//! причины живут в его RAM-журнале и `gateway.log` (`KingOrchData/gateway/`).
//! Хостовый `king_orch.log` и вкладка «Логи» об этом молчали — пользователь
//! видел тост «неизвестно 5» и нигде не находил деталей (запрос 25.10.2026).
//!
//! ## Как
//!
//! Плагин раз в [`POLL_INTERVAL`] читает `GET /api/events?since={seq}` у
//! запущенного шлюза (тот же курсорный протокол, что у дашборда) и пишет
//! события в `log::info!/warn!/error!` с префиксом `gateway:`. Ошибки опроса
//! молчаливы: шлюз вправе быть остановлен или перезапущен — это норма, а не
//! отказ (core rules §2.2 требует логировать отказы, а не нормальные паузы).
//!
//! Обратной связи нет: записи плагина не попадают в `EventLog` шлюза
//! (разные процессы), цикл невозможен. Первое чтение синхронизирует курсор
//! без зеркала бэклога — история и так в `gateway.log`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Deserialize;
use tauri::AppHandle;

use crate::spec::RouterId;
use crate::router::config;

/// Пауза между опросами. Дашборд использует 3 с при видимой вкладке журнала.
const POLL_INTERVAL: Duration = Duration::from_secs(3);

/// Таймаут одного опроса: шлюз локальный, долгие зависания бессмысленны.
const HTTP_TIMEOUT: Duration = Duration::from_secs(2);

/// Префикс зеркалируемых строк в логе приложения.
const PREFIX: &str = "gateway:";

/// Курсор: номер последнего зеркалированного события.
static LAST_SEQ: AtomicU64 = AtomicU64::new(0);

/// Флаг «курсор уже синхронизирован» — первое чтение бэклог не пишет.
static SYNCED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Событие журнала шлюза (`crates/gateway/src/logbuf.rs::Event`).
#[derive(Debug, Clone, Deserialize)]
pub struct GatewayEvent {
    pub seq: u64,
    #[serde(default)]
    pub level: String,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Default, Deserialize)]
struct EventsResponse {
    #[serde(default)]
    events: Vec<GatewayEvent>,
    #[serde(default)]
    last_seq: u64,
}

/// Уровень события → приоритет лога хоста. Неизвестный уровень — info:
/// незнакомая строка не повод терять её содержимое.
fn log_level(level: &str) -> Option<log::Level> {
    match level {
        "warn" => Some(log::Level::Warn),
        "error" => Some(log::Level::Error),
        "info" => Some(log::Level::Info),
        _ => Some(log::Level::Info),
    }
}

/// Чистое ядро зеркала: по текущему курсору и ответу шлюза решает, что писать
/// в лог и каким будет новый курсор. Отделено от сети ради тестов.
///
/// Правило отката: `last_seq` меньше курсора — шлюз перезапускался (нумерация
/// началась заново); бэклог нового процесса не зеркалим (он весь в
/// `gateway.log`), курсор продолжаем с его хвоста.
pub fn mirror_plan(
    current_seq: u64,
    synced: bool,
    events: Vec<GatewayEvent>,
    last_seq: u64,
) -> (u64, Vec<(log::Level, String)>) {
    if last_seq < current_seq {
        return (last_seq, Vec::new());
    }
    if !synced {
        return (last_seq, Vec::new());
    }
    let mut out = Vec::with_capacity(events.len());
    let mut seq = current_seq;
    for ev in events {
        if ev.seq > seq {
            seq = ev.seq;
        }
        if let Some(level) = log_level(&ev.level) {
            out.push((level, format!("{PREFIX} {}", ev.message)));
        }
    }
    (seq.max(last_seq), out)
}

/// Запустить цикл зеркала (один раз на процесс, идемпотентно).
pub fn spawn_mirror(app: &AppHandle) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        // Один клиент на весь цикл: без новой коннекции на каждый опрос.
        let client = reqwest::Client::builder().timeout(HTTP_TIMEOUT).build().ok();
        loop {
            tokio::time::sleep(POLL_INTERVAL).await;
            poll_once(&client, &handle).await;
        }
    });
}

/// Один опрос шлюза. Любая неудача — тихий пропуск: шлюз остановлен,
/// перезапускается или запись `server.json` устарела; следующий тик повторит.
async fn poll_once(client: &Option<reqwest::Client>, app: &AppHandle) {
    let Some(client) = client else { return };
    let data_dir = config::router_data_dir(app, RouterId::Gateway);
    let Some(rec) = config::read_server_record(&data_dir) else {
        // Шлюз не запущен (записи нет) — зеркалить нечего.
        return;
    };
    let url = format!(
        "http://127.0.0.1:{}/api/events?since={}",
        rec.port,
        LAST_SEQ.load(Ordering::Relaxed)
    );
    let Ok(resp) = client.get(&url).send().await else {
        return;
    };
    let Ok(body) = resp.json::<EventsResponse>().await else {
        return;
    };
    let current = LAST_SEQ.load(Ordering::Relaxed);
    let synced = SYNCED.load(Ordering::Relaxed);
    let (new_seq, lines) = mirror_plan(current, synced, body.events, body.last_seq);
    for (level, line) in lines {
        match level {
            log::Level::Warn => log::warn!("{line}"),
            log::Level::Error => log::error!("{line}"),
            _ => log::info!("{line}"),
        }
    }
    LAST_SEQ.store(new_seq, Ordering::Relaxed);
    SYNCED.store(true, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(seq: u64, level: &str, message: &str) -> GatewayEvent {
        GatewayEvent {
            seq,
            level: level.into(),
            message: message.into(),
        }
    }

    #[test]
    fn first_poll_syncs_cursor_without_backlog() {
        let (seq, lines) = mirror_plan(
            0,
            false,
            vec![ev(1, "info", "a"), ev(2, "warn", "b")],
            2,
        );
        assert_eq!(seq, 2, "курсор перепрыгивает на хвост шлюза");
        assert!(lines.is_empty(), "бэклог не зеркалится: {}", lines.len());
    }

    #[test]
    fn subsequent_polls_mirror_new_events_by_level() {
        let (seq, lines) = mirror_plan(
            2,
            true,
            vec![ev(3, "info", "валиден"), ev(4, "warn", "непонятный ответ"), ev(5, "error", "падение")],
            5,
        );
        assert_eq!(seq, 5);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], (log::Level::Info, "gateway: валиден".into()));
        assert_eq!(lines[1], (log::Level::Warn, "gateway: непонятный ответ".into()));
        assert_eq!(lines[2], (log::Level::Error, "gateway: падение".into()));
    }

    /// Рестарт шлюза: нумерация началась заново, `last_seq` откатился ниже
    /// курсора. Бэклог не зеркалим (иначе каждое зеркало после рестарта
    /// задублировало бы свежую историю), курсор — на новый хвост.
    #[test]
    fn seq_rollback_resets_cursor_silently() {
        let (seq, lines) = mirror_plan(500, true, vec![ev(3, "info", "новая жизнь")], 3);
        assert_eq!(seq, 3);
        assert!(lines.is_empty());
    }

    #[test]
    fn unknown_level_falls_back_to_info() {
        let (_, lines) = mirror_plan(0, true, vec![ev(1, "трейс", "что-то новое")], 1);
        assert_eq!(lines, vec![(log::Level::Info, "gateway: что-то новое".into())]);
    }

    #[test]
    fn empty_poll_keeps_cursor() {
        let (seq, lines) = mirror_plan(7, true, vec![], 7);
        assert_eq!(seq, 7);
        assert!(lines.is_empty());
    }
}
