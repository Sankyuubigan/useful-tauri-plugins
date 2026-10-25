//! Супервизор жизни шлюза: единственный наблюдатель смерти и воскресения.
//!
//! ## Зачем
//!
//! До 25.10.2026 смерть шлюза была **ненаблюдаемой**: `server.json` писал
//! только стартующий процесс, живых событий не существовало, панель
//! обновлялась опросом раз в 15 с, а вкладка дашборда с iframe оставалась
//! навсегда на странице ошибки браузера («127.0.0.1 отказано в подключении»).
//! Реальный кейс: шлюз 0.1.7 умер между 07:37 и 12:52 — ни одной строки
//! нигде, причина не установлена (gateway.log обнуляется при старте).
//!
//! ## Как
//!
//! Отдельный поток с тиком [`TICK`]: чтение `server.json` + проба порта
//! (`port_open`, тот же приём, что у `build_status` — единая семантика
//! «живой шлюз всегда слушает порт»). Чистая машина состояний
//! [`supervision_plan`] переводит наблюдение в действия:
//!
//! - **Дебаунс смерти** — «запись есть, порт мёртв» [`DEAD_AFTER_MISSES`]
//!   тика подряд. Штатная остановка закрывает порт и стирает запись
//!   **соседними шагами** (`stop_server`), поэтому между тиками запись уже
//!   исчезает и ложной смерти не возникает. Крашеный процесс стереть запись
//!   не может — его смерть объявляется честно.
//! - **Стартовый грейс** — свежая запись (в пределах
//!   [`STARTUP_GRACE_SECS`]) с ещё не открытым портом не копит счётчик:
//!   `start_server` пишет запись ДО `wait_port`, и медленный старт
//!   (антивирус, диск) — не смерть.
//! - **Смерть**: `log::warn!` с pid/версией/портом (E1 — смерть становится
//!   наблюдаемой), протухшая запись убирается (E3 — самоочистка; живой
//!   шлюз всегда слушает порт, запись без порта — мусор, принцип фикса D0),
//!   эмит [`STATUS_EVENT`] `running:false`.
//! - **Жизнь**: переход «не был объявлен живым» → жив — эмит
//!   [`STATUS_EVENT`] `running:true`. По нему хост перезагружает застрявшие
//!   вкладки дашборда, а панель обновляет пилюлю за один тик вместо 15 с.
//!
//! Автоперезапуском супервизор **не занимается** (решение пользователя
//! 25.10.2026): причина смерти неизвестна, и молчаливый подъём скрыл бы её
//! вместо диагностики. Подъём — руками («Запустить», клик «Web UI»).

use std::sync::Mutex;
use std::time::Duration;

use serde_json::json;
use tauri::{AppHandle, Emitter};

use crate::router::config;
use crate::router::process::port_open;
use crate::spec::RouterId;

/// Период тика супервизора.
const TICK: Duration = Duration::from_secs(3);

/// Событие «состояние жизни шлюза изменилось».
///
/// Payload: `{ router, running, port, base_url }`. События идут ТОЛЬКО на
/// переходах (не периодически): подписчики — хост (перезагрузка застрявших
/// вкладок дашборда) и панель плагина (немедленный refresh статуса).
pub const STATUS_EVENT: &str = "cloud-routers-status";

/// Сколько тиков подряд «запись есть, порт мёртв» объявляют смерть.
///
/// 2 тика (~6 с) — компромисс: штатная остановка стирает запись сразу после
/// закрытия порта (ложных срабатываний нет), а крашеный процесс дольше 6 с
/// «невидимым» не остаётся.
pub const DEAD_AFTER_MISSES: u8 = 2;

/// Стартовый грейс: запись моложе этого возраста с ещё закрытым портом —
/// «поднимается», а не «умер». `START_TIMEOUT` (25 с) + запас на часы.
pub const STARTUP_GRACE_SECS: u64 = 35;

/// Наблюдение одного тика: что супервизор видит в мире. Чистые данные —
/// разделяют чтение (сеть/файлы) и решение ([`supervision_plan`]).
#[derive(Debug, Clone, PartialEq)]
pub enum Observation {
    /// `server.json` нет — шлюз не запущен (или штатно остановлен).
    NoRecord,
    /// Запись свежая, порт ещё не открыт — старт в процессе.
    Starting,
    /// Порт отвечает — жив (владение портом сортирует `build_status`).
    Alive,
    /// Запись старая, порт мёртв — кандидат в «умер» (считается дебаунсом).
    Unresponsive,
}

/// Память супервизора между тиками.
#[derive(Debug, Clone, PartialEq)]
pub struct SupervisionState {
    /// «Жив» уже объявлен (эмит `running:true` ушёл). Повторы не нужны.
    pub announced_running: bool,
    /// Подряд идущие тики [`Observation::Unresponsive`].
    pub misses: u8,
}

impl SupervisionState {
    pub const fn new() -> Self {
        Self { announced_running: false, misses: 0 }
    }
}

impl Default for SupervisionState {
    fn default() -> Self {
        Self::new()
    }
}

/// Действие, которое супервизор просит исполнить. Отделено от решения ради
/// тестов: [`supervision_plan`] не трогает ни сеть, ни файлы, ни эвенты.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Объявить «жив» (`running:true`): лог + эмит. Хост по нему
    /// перезагружает застрявшие вкладки дашборда.
    AnnounceRunning,
    /// Объявить смерть (`running:false`): warn с деталями + эмит.
    AnnounceDead,
    /// Запись протухла (порт мёртв ≥ дебаунса) — убрать её с диска.
    ClearStaleRecord,
    /// Запись исчезла после объявленной жизни (штатная остановка) —
    /// объявить «не запущен» (`running:false`).
    AnnounceStopped,
}

/// Классифицировать наблюдение тика: возраст записи (`Some` — запись есть),
/// жив ли порт.
///
/// Порт жив → [`Observation::Alive`] независимо от записи: жизнь смотрят по
/// порту (живой шлюз всегда слушает), запись здесь не источник правды.
pub fn observe(record_age_secs: Option<u64>, port_live: bool) -> Observation {
    match (record_age_secs, port_live) {
        (_, true) => Observation::Alive,
        (None, false) => Observation::NoRecord,
        (Some(age), false) => {
            if age <= STARTUP_GRACE_SECS {
                Observation::Starting
            } else {
                Observation::Unresponsive
            }
        }
    }
}

/// Чистая машина состояний: предыдущее состояние + наблюдение →
/// новое состояние и список действий.
///
/// Инвариант: смерть объявляется ровно один раз на эпизод (после
/// `AnnounceDead` состояние сброшено, повторный `Unresponsive` снова копит
/// дебаунс — но запись уже убрана, и наблюдение станет `NoRecord`).
pub fn supervision_plan(
    prev: &SupervisionState,
    obs: &Observation,
) -> (SupervisionState, Vec<Action>) {
    match obs {
        Observation::Alive => {
            let mut next = prev.clone();
            next.misses = 0;
            if next.announced_running {
                return (next, Vec::new());
            }
            next.announced_running = true;
            (next, vec![Action::AnnounceRunning])
        }
        Observation::NoRecord => {
            let next = SupervisionState::new();
            if prev.announced_running {
                (next, vec![Action::AnnounceStopped])
            } else {
                (next, Vec::new())
            }
        }
        Observation::Starting => {
            // Рестарт в процессе (новая запись, порт ещё закрыт): счётчик
            // смерти не копим и смерть не объявляем. `announced_running`
            // сохраняем — предыдущая жизнь ещё не отменена, повторный эмит
            // не нужен.
            let mut next = prev.clone();
            next.misses = 0;
            (next, Vec::new())
        }
        Observation::Unresponsive => {
            let mut next = prev.clone();
            next.misses = next.misses.saturating_add(1);
            if next.misses >= DEAD_AFTER_MISSES {
                next.announced_running = false;
                next.misses = 0;
                (
                    next,
                    vec![Action::AnnounceDead, Action::ClearStaleRecord],
                )
            } else {
                (next, Vec::new())
            }
        }
    }
}

/// Запустить цикл супервизора (один поток на процесс, идемпотентность на
/// вызывающем — вызывается из setup плагина единожды).
pub fn spawn_supervisor(app: &AppHandle) {
    let handle = app.clone();
    let spawned = std::thread::Builder::new()
        .name("gateway-supervisor".into())
        .spawn(move || loop {
            std::thread::sleep(TICK);
            tick(&handle);
        });
    if let Err(e) = spawned {
        // Без супервизора система остаётся в прежнем поведении (опрос 15 с);
        // падать из-за второстепенного потока нельзя.
        log::warn!("cloud-routers: поток супервизора не запущен ({}), смерть шлюза будет видна только опросом", e);
    }
}

/// Память между тиками. `Mutex::new` — const, статика без lazy_static.
static STATE: Mutex<SupervisionState> = Mutex::new(SupervisionState::new());

/// Один тик: наблюдение → план → исполнение. Ошибки чтения записи и конфига
/// трактованы как `NoRecord`/пустой base_url — следующий тик повторит.
fn tick(app: &AppHandle) {
    let router_id = RouterId::Gateway;
    let data_dir = config::router_data_dir(app, router_id);
    let rec = config::read_server_record(&data_dir);
    let port_live = rec
        .as_ref()
        .map(|r| port_open(r.port, Duration::from_millis(400)))
        .unwrap_or(false);
    let age_secs = rec
        .as_ref()
        .map(|r| now_unix().saturating_sub(r.started_at_unix));
    let obs = observe(age_secs, port_live);

    let actions = {
        // Только этот поток пишет состояние; lock на случай будущих тиковщиков.
        let mut guard = STATE.lock().expect("supervisor state poisoned");
        let (next, actions) = supervision_plan(&guard, &obs);
        *guard = next;
        actions
    };

    if actions.is_empty() {
        return;
    }
    let cfg = config::load_config(app, router_id);
    let base_url = cfg.base_url(router_id);
    for action in actions {
        match action {
            Action::AnnounceRunning => {
                let (pid, port) = rec
                    .as_ref()
                    .map(|r| (r.pid, r.port))
                    .unwrap_or((0, cfg.port_or_default(router_id)));
                log::info!("gateway: шлюз жив (pid {}, порт {})", pid, port);
                let _ = app.emit(
                    STATUS_EVENT,
                    json!({ "router": router_id.as_str(), "running": true, "port": port, "base_url": base_url }),
                );
            }
            Action::AnnounceStopped => {
                log::info!("gateway: шлюз остановлен (запись процесса исчезла)");
                let _ = app.emit(
                    STATUS_EVENT,
                    json!({ "router": router_id.as_str(), "running": false, "base_url": base_url }),
                );
            }
            Action::AnnounceDead => {
                let (pid, port, version) = rec
                    .as_ref()
                    .map(|r| (r.pid, r.port, r.version.clone()))
                    .unwrap_or((0, 0, None));
                log::warn!(
                    "gateway: ШЛЮЗ УМЕР: запись процесса есть (pid {}, версия {:?}), порт {} не отвечает {} тиков подряд. \
                     Причина смерти процессом не оставлена (gateway.log сессионный). Запуск — кнопкой «Запустить» или «Web UI».",
                    pid,
                    version,
                    port,
                    DEAD_AFTER_MISSES
                );
                let _ = app.emit(
                    STATUS_EVENT,
                    json!({ "router": router_id.as_str(), "running": false, "port": port, "base_url": base_url }),
                );
            }
            Action::ClearStaleRecord => {
                config::clear_server_record(&data_dir);
                log::info!("gateway: протухшая запись процесса убрана (порт был мёртв)");
            }
        }
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(announced: bool, misses: u8) -> SupervisionState {
        SupervisionState { announced_running: announced, misses }
    }

    // --- observe() ---

    #[test]
    fn port_live_means_alive_regardless_of_record() {
        assert_eq!(observe(None, true), Observation::Alive);
        assert_eq!(observe(Some(0), true), Observation::Alive);
        assert_eq!(observe(Some(10_000), true), Observation::Alive);
    }

    #[test]
    fn no_record_when_record_absent_and_port_dead() {
        assert_eq!(observe(None, false), Observation::NoRecord);
    }

    #[test]
    fn fresh_record_with_dead_port_is_starting() {
        // Внутри старт-грейса (запись написана до wait_port) — не смерть.
        assert_eq!(observe(Some(0), false), Observation::Starting);
        assert_eq!(observe(Some(STARTUP_GRACE_SECS), false), Observation::Starting);
    }

    #[test]
    fn aged_record_with_dead_port_is_unresponsive() {
        assert_eq!(observe(Some(STARTUP_GRACE_SECS + 1), false), Observation::Unresponsive);
    }

    // --- supervision_plan() ---

    #[test]
    fn first_alive_announces_running_once() {
        let (next, actions) = supervision_plan(&SupervisionState::new(), &Observation::Alive);
        assert_eq!(actions, vec![Action::AnnounceRunning]);
        assert!(next.announced_running);

        let (_, actions) = supervision_plan(&next, &Observation::Alive);
        assert!(actions.is_empty(), "повторная жизнь не эмитится");
    }

    #[test]
    fn alive_then_no_record_announces_stopped() {
        let (running, _) = supervision_plan(&SupervisionState::new(), &Observation::Alive);
        let (next, actions) = supervision_plan(&running, &Observation::NoRecord);
        assert_eq!(actions, vec![Action::AnnounceStopped]);
        assert!(!next.announced_running);

        // И после этого тишина (записи по-прежнему нет).
        let (_, actions) = supervision_plan(&next, &Observation::NoRecord);
        assert!(actions.is_empty());
    }

    /// Штатная остановка: порт закрылся, запись стёрлась соседним шагом
    /// `stop_server`. Супервизор ловит либо сразу NoRecord (тишина), либо
    /// один Unresponsive → NoRecord — но НИКОГДА не объявляет смерть.
    #[test]
    fn graceful_stop_never_announces_death() {
        let (running, _) = supervision_plan(&SupervisionState::new(), &Observation::Alive);

        // Тик поймал окно «порт закрыт, запись ещё не стёрта» — но запись
        // ещё «свежая»? Нет: запись старая (старт был давно), поэтому
        // честно Unresponsive с misses=1.
        let (miss1, actions) = supervision_plan(&running, &Observation::Unresponsive);
        assert!(actions.is_empty(), "один тик без порта — не смерть");

        // Запись стёрлась — останавливаемся без смерти.
        let (_, actions) = supervision_plan(&miss1, &Observation::NoRecord);
        assert_eq!(actions, vec![Action::AnnounceStopped]);
    }

    /// Крашеный процесс запись стереть не может: два тика подряд без порта
    /// объявляют смерть ровно один раз + чистку протухшей записи.
    #[test]
    fn two_consecutive_unresponsive_ticks_announce_death_once() {
        let (running, _) = supervision_plan(&SupervisionState::new(), &Observation::Alive);
        let (miss1, actions) = supervision_plan(&running, &Observation::Unresponsive);
        assert!(actions.is_empty());

        let (after, actions) = supervision_plan(&miss1, &Observation::Unresponsive);
        assert_eq!(actions, vec![Action::AnnounceDead, Action::ClearStaleRecord]);
        assert!(!after.announced_running, "после смерти «жив» сброшен");
        assert_eq!(after.misses, 0, "счётчик сброшен после объявления");

        // Запись убрана → следующий тик NoRecord → тишина (смерть не повторяется).
        let (_, actions) = supervision_plan(&after, &Observation::NoRecord);
        assert!(actions.is_empty(), "смерть объявляется один раз на эпизод");
    }

    /// Рестарт: старая жизнь → свежая запись с закрытым портом → порт открыт.
    /// Смерть не объявляется, повторного «жив» нет.
    #[test]
    fn restart_via_starting_does_not_announce_death_or_duplicate_running() {
        let (running, _) = supervision_plan(&SupervisionState::new(), &Observation::Alive);
        let (during, actions) = supervision_plan(&running, &Observation::Starting);
        assert!(actions.is_empty(), "стартовое окно не смерть");
        assert!(during.announced_running, "предыдущая жизнь сохраняется");

        let (_, actions) = supervision_plan(&during, &Observation::Alive);
        assert!(actions.is_empty(), "рестарт не дублирует «жив»");
    }

    /// Зависший старт: свежая запись, порт так и не открылся, запись
    /// состарилась за грейсом → смерть объявляется.
    #[test]
    fn stalled_start_eventually_announces_death() {
        let mut s = SupervisionState::new();
        let (next, actions) = supervision_plan(&s, &Observation::Starting);
        assert!(actions.is_empty());
        s = next;
        // Запись состарилась, порт всё ещё закрыт.
        let (next, actions) = supervision_plan(&s, &Observation::Unresponsive);
        assert!(actions.is_empty());
        s = next;
        let (_, actions) = supervision_plan(&s, &Observation::Unresponsive);
        assert_eq!(actions, vec![Action::AnnounceDead, Action::ClearStaleRecord]);
    }

    /// Смерть без предшествующей жизни (хост стартовал, а шлюз — труп):
    /// дебаунс всё равно объявляет смерть и чистит запись, но «остановлен»
    /// не эмитится (нечего отменять).
    #[test]
    fn corpse_at_supervisor_start_is_cleaned() {
        let (miss1, actions) = supervision_plan(&SupervisionState::new(), &Observation::Unresponsive);
        assert!(actions.is_empty());
        let (_, actions) = supervision_plan(&miss1, &Observation::Unresponsive);
        assert_eq!(actions, vec![Action::AnnounceDead, Action::ClearStaleRecord]);
    }

    #[test]
    fn starting_resets_misses_accumulated_before() {
        // Пограничный случай: счётчик копился, потом запись переписал
        // стартующий процесс (грейс) — счётчик не переживает стартовое окно.
        let (next, _) = supervision_plan(&state(false, 1), &Observation::Starting);
        assert_eq!(next.misses, 0);
    }
}
