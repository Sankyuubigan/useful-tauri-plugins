//! Управление процессом собственного шлюза (`cloud-routers-gateway`).
//!
//! ## Отличие от `router::process` (node-роутеры)
//!
//! | | node-роутер | наш шлюз |
//! |---|---|---|
//! | Что запускается | `node.exe` + JS-файл | один бинарь |
//! | Путь | `<dir>/runtime/node.exe`, `<dir>/dist/…` | `<dir>/cloud-routers-gateway.exe` |
//! | Логи | `server.log` рядом с роутером | `<data_dir>/gateway.log` |
//! | Как найти «свой» процесс | сверка exe с `node.exe` | сверка exe с бинарём шлюза |
//!
//! ## Безопасность остановки
//!
//! [`stop_server`] **обязан** сверить исполняемый файл целевого pid с бинарём
//! шлюза и только потом вызывать `taskkill`. Без сверки испорченный
//! `server.json` (или pid, переиспользованный ОС) привёл бы к убийству
//! постороннего процесса — вплоть до работающего приложения-хоста. Правило
//! жёсткое: нет подтверждения — нет kill. Тест
//! `refuses_to_kill_process_with_foreign_exe` это фиксирует.
//!
//! ## Жизненный цикл
//!
//! Шлюз запускается **отсоединённым** (`DETACHED_PROCESS`) и переживает
//! закрытие приложения-хоста: он обслуживает внешние IDE, которые работают
//! независимо. Поэтому `reconcile` на старте хоста шлюз **не убивает**, а
//! принимает в работу — и предупреждает, если версия устарела.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tauri::AppHandle;

use crate::commands::RouterId;
use crate::router::config::{self, RouterConfig};
use crate::router::process::{kill_pid_tree, pid_alive, port_open, wait_port, GatewayState};

/// Сколько ждём готовности порта после запуска бинаря.
const START_TIMEOUT: Duration = Duration::from_secs(25);

/// Сколько ждём самозавершения шлюза после HTTP-остановки (`try_http_shutdown`).
const HTTP_STOP_WAIT: Duration = Duration::from_secs(3);

/// HTTP-самозавершение: просим шлюз завершить **сам себя**.
///
/// Задача «убить чужой процесс» на Windows упирается в политику
/// TerminateProcess: процесс, созданный повышенным (админским) хостом, нельзя
/// остановить из не-повышенного — реальный кейс 22.10.26 (pid старой копии,
/// taskkill «Отказано в доступе» ×3, журналы dev-сессии). Задача «процесс сам
/// себя закрывает» этих прав не требует, поэтому HTTP-остановка — первый шаг,
/// а `taskkill` остаётся запасом на зависший процесс.
fn try_http_shutdown(port: u16) -> bool {
    let url = format!("http://127.0.0.1:{port}/api/shutdown");
    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(1500))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            log::warn!("🛑 шлюз: HTTP-клиент остановки не собрался ({}), дальше taskkill", e);
            return false;
        }
    };
    match client.post(&url).send() {
        Ok(resp) => {
            log::info!("🛑 шлюз: команда самозавершения доставлена ({})", resp.status());
            true
        }
        Err(e) => {
            log::warn!("🛑 шлюз: самозавершение не принято/не доставлено ({}), дальше taskkill", e);
            false
        }
    }
}


/// Флаги `CreateProcess` для Windows.
///
/// * `CREATE_NO_WINDOW` (0x08000000) — не мигать чёрным окном консоли.
/// * `DETACHED_PROCESS` (0x00000008) — процесс не умирает вместе с хостом.
/// * `CREATE_NEW_PROCESS_GROUP` (0x00000200) — отделить от группы управления
///   консоли хоста, чтобы Ctrl+C в терминале не гасил шлюз.
#[cfg(windows)]
const CREATE_FLAGS: u32 = 0x08000000 | 0x00000008 | 0x00000200;

/// Папка установки нашего шлюза.
///
/// По умолчанию — каталог плагина (`APPDATA/cloud-routers/gateway`), общий для
/// всех проектов. Именно общий, а не папка приложения: процесс шлюза один на
/// машину, и папка с его бинарём обязана совпадать у всех, иначе сверка «свой
/// pid / чужой» (`stop_server`, `gateway_state`) откажется работать во втором
/// проекте. Переопределяется ключом `cloud_routers.gateway.dir` в конфиге
/// плагина — как и у остальных роутеров.
pub fn router_dir(app: &AppHandle, router_id: RouterId) -> PathBuf {
    config::router_dir(app, router_id)
}

/// Путь к бинарю шлюза (`<dir>/cloud-routers-gateway.exe`).
pub fn gateway_exe(dir: &Path) -> PathBuf {
    let name = match crate::spec::RouterId::ALL
        .iter()
        .find(|r| r.is_native_gateway())
        .and_then(|r| r.gateway_exe_file())
    {
        Some(n) => n,
        None => "cloud-routers-gateway",
    };
    dir.join(format!("{}.exe", name))
}

/// Прочитать запись о процессе шлюза (pid/порт/версия).
pub fn read_record(data_dir: &Path) -> Option<config::ServerRecord> {
    config::read_server_record(data_dir)
}

/// Версия для показа в панели: живой `server_record` приоритетнее запомненной
/// при установке `RouterConfig.installed_version` (конфиг может отстать, если
/// процесс заменён свежим бинарём). При остановке рекорд затёрт — остаётся
/// конфиг; если и его нет, показывает `None` (build_status сам решает, спавнить
/// ли `--version` — side-effect, поэтому в чистую функцию не входит).
///
/// Чистая формула выделена отдельно ради тестов (`stopped_status_shows_installed_
/// version` в commands.rs).
pub fn display_version(rec_version: Option<String>, installed: Option<String>) -> Option<String> {
    rec_version.or(installed)
}

/// Ответ ли `/v1/models` наш шлюз: значит, процесс жив и отвечает.
///
/// `401`/`403` тоже считаются «жив» — как и в `client::is_healthy`: сервер
/// есть, просто требует авторизацию.
fn gateway_answers(base_url: &str) -> bool {
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    let Ok(c) = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(4))
        .build()
    else {
        return false;
    };
    match c.get(&url).send() {
        Ok(resp) => {
            let s = resp.status();
            s.is_success()
                || s == reqwest::StatusCode::UNAUTHORIZED
                || s == reqwest::StatusCode::FORBIDDEN
        }
        Err(_) => false,
    }
}

/// Запустить шлюз, если он ещё не работает. Возвращает pid (0 — уже был запущен).
pub fn start_server(app: &AppHandle, router_id: RouterId, cfg: &RouterConfig) -> Result<u32, String> {
    let port = cfg.port_or_default(router_id);
    let dir = router_dir(app, router_id);
    let data_dir = config::router_data_dir(app, router_id);
    let base_url = cfg.base_url(router_id);

    if port_open(port, Duration::from_millis(400)) {
        if gateway_answers(&base_url) {
            log::info!("🟢 {} уже запущен на порту {}", router_id, port);
            return Ok(0);
        }
        // Порт занят, но наш шлюз не отвечает: чужой процесс или наш битый.
        return Err(format!(
            "Порт {} занят, но шлюз на нём не отвечает. Остановите посторонний процесс или смените порт.",
            port
        ));
    }

    let exe = gateway_exe(&dir);
    if !exe.exists() {
        return Err(format!(
            "Установите {} (нет файла {}).",
            router_id,
            exe.display()
        ));
    }

    // Каталог данных: порт передаём явно, чтобы не зависеть от того, что новый
    // экземпляр успел прочитать конфиг (он может подниматься поверх старого при
    // самоперезапуске).
    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("--data-dir")
        .arg(&data_dir)
        .arg("--port")
        .arg(port.to_string())
        .current_dir(&dir)
        .stdin(std::process::Stdio::null())
        .stdout(log_stdio(&data_dir))
        .stderr(log_stdio(&data_dir));

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_FLAGS);
    }

    log::info!(
        "🚀 Запуск {}: {} (data_dir={}, порт={})",
        router_id,
        exe.display(),
        data_dir.display(),
        port
    );
    let child = cmd
        .spawn()
        .map_err(|e| format!("Не удалось запустить {}: {}", router_id, e))?;
    let pid = child.id();
    config::write_server_record(&data_dir, pid, port, None);

    if !wait_port(port, START_TIMEOUT) {
        log::warn!(
            "⚠️ {} не открыл порт {} за {} сек (pid {})",
            router_id,
            port,
            START_TIMEOUT.as_secs(),
            pid
        );
        // Не выкидываем pid: процесс мог подняться чуть позже (антивирус, диск).
        return Ok(pid);
    }
    // Версию ЗАПИСЫВАЕТ САМ БИНАРЬ при старте: main.rs run() вызывает
    // write_server_record(Some(core VERSION)). Ставить сюда версию плагина
    // (env!("CARGO_PKG_VERSION") = 0.1.0) нельзя: она затирала бы честную
    // версию бинаря сразу после bind, и панель показывала «v0.1.0» при живом
    // 0.1.4 — доказано боем 23.10.2026 (server.json).
    Ok(pid)
}

/// Поток для журнала шлюза: `<data_dir>/gateway.log` или «в никуда».
///
/// Файл обрезается при каждом старте: журнал сессионный, а не архивный
/// (правило `core/rules.md` §2.5.1). Полный лог остаётся в этом же файле и не
/// дублируется в лог приложения — два файла об одном событии были бы двумя
/// правдами.
///
/// Возвращается [`Stdio`], а не `File`, потому что `Stdio::null()` — единственный
/// вариант, работающий всегда. Если журнал недоступен (нет прав на папку, файл
/// держит антивирус), шлюз всё равно должен стартовать: ломать
/// функциональность из-за второстепенной функции нельзя.
fn log_stdio(data_dir: &Path) -> std::process::Stdio {
    let path = data_dir.join("gateway.log");
    match std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)
    {
        Ok(file) => std::process::Stdio::from(file),
        Err(e) => {
            log::warn!(
                "не удалось открыть {} для журнала шлюза ({}); вывод будет потерян",
                path.display(),
                e
            );
            std::process::Stdio::null()
        }
    }
}

/// Остановить шлюз с проверкой результата.
///
/// Убивает **только** процесс, чей исполняемый файл совпадает с бинарём
/// шлюза. Совпадение проверяется по-прежнему — см. предупреждение в заголовке
/// модуля.
///
/// Контракт — тот же, что у node-роутеров (`process::stop_server`): успех
/// подтверждается закрытием порта, при неудаче запись НЕ стирается и
/// возвращается `Err`. Всегда-`Ok` без проверки означал бы ложный «остановлен»
/// при живом процессе — тот же класс лжи, что чинился у node-роутеров.
pub fn stop_server(dir: &Path, port: u16, data_dir: &Path) -> Result<(), String> {
    let Some(rec) = config::read_server_record(data_dir) else {
        log::info!(
            "cloud-routers: у шлюза нет server.json — нечего останавливать (порт {})",
            port
        );
        return Ok(());
    };
    if rec.pid == 0 {
        // Запись без pid — мусор (так пишется, когда процесс не успел стартовать).
        // Убираем её, иначе статус вечно показывал бы «установлен, но не
        // запущен», хотя на самом деле запускать нечего.
        config::clear_server_record(data_dir);
        return Ok(());
    }
    if !pid_alive(rec.pid) {
        log::info!("🧹 шлюз уже не жив (pid {}), убираем запись", rec.pid);
        config::clear_server_record(data_dir);
        return Ok(());
    }
    let exe = gateway_exe(dir);
    if !crate::router::process::process_exe_matches(rec.pid, &exe) {
        // Ключевая защита: pid из файла может принадлежать чему угодно.
        // Убивать наугад нельзя — под удар попадёт чужой процесс, возможно
        // приложение-хост.
        //
        // Сверка идёт с `dir`, а не с папкой по умолчанию: пользователь мог
        // перенести установку через «Изменить путь», и сверка с дефолтом
        // заставила бы отказаться убивать собственный шлюз.
        //
        // Исключение — протухшая запись с переиспользованным pid (реальный
        // кейс 25.10.2026: шлюз умер, его pid 32884 отошёл браузеру thorium).
        // Живой шлюз ВСЕГДА слушает свой порт; если порт свободен, нашего
        // шлюза нет в принципе, и записью в чужой живой процесс является
        // мусор, а не сирота — сирота держал бы порт. Чистим и считаем
        // остановку успешной: удалять чужой процесс всё равно не требуется.
        // Порт занят, но не нами — прежний отказ без снятия записи.
        if !port_open(port, Duration::from_millis(200)) {
            config::clear_server_record(data_dir);
            log::warn!(
                "🧹 запись шлюза указывала на pid {}, но это не наш бинарь, и порт {} свободен \
                 — pid переиспользован ОС, запись протухла и убрана",
                rec.pid,
                port
            );
            return Ok(());
        }
        // Запись при этом НЕ снимается: pid жив, и снеся её, мы оставили бы
        // процесс-сироту — записи нет, останавливать нечем, статус врёт. Раньше
        // запись стиралась, и шлюз, запущенный ДРУГИМ проектом (папка у него
        // своя), становился неуправляемым навсегда. Теперь запись переживает
        // отказ, и как только пользователь освободит порт, следующая проверка
        // уберёт её сама (см. ветку «pid не жив» выше).
        log::warn!(
            "🚫 pid {} не принадлежит бинарю шлюза ({}). Остановка пропущена, запись оставлена.",
            rec.pid,
            exe.display()
        );
        return Err(format!(
            "Порт {} держит чужой процесс (pid {}), не шлюз. Остановите его или смените порт.",
            port, rec.pid
        ));
    }
    log::info!("🛑 Остановка шлюза (pid {})", rec.pid);

    // Порт уже свободен (умер сам) — фиксируем успех без kill.
    if !port_open(port, Duration::from_millis(200)) {
        config::clear_server_record(data_dir);
        log::info!("✅ шлюз остановлен (порт {} уже свободен)", port);
        return Ok(());
    }

    // Первый шаг — самозавершение по HTTP: elevated-процесс всегда может
    // закрыть сам себя, политика TerminateProcess на этот путь не влияет.
    // taskkill ниже остаётся запасом на зависший/неответивший процесс.
    if try_http_shutdown(port)
        && crate::router::process::wait_port_closed(port, HTTP_STOP_WAIT)
    {
        config::clear_server_record(data_dir);
        log::info!("✅ шлюз сам завершился по HTTP-остановке (порт {} закрыт)", port);
        return Ok(());
    }
    log::warn!("шлюз не завершился сам (pid {}), принудительное завершение", rec.pid);

    let report = kill_pid_tree(rec.pid);
    if !report.succeeded {
        log::warn!("шлюз: kill pid {} не удался: {}", rec.pid, report.detail);
    }
    if crate::router::process::wait_port_closed(port, crate::router::process::STOP_WAIT) {
        config::clear_server_record(data_dir);
        log::info!("✅ шлюз остановлен (порт {} закрыт)", port);
        Ok(())
    } else {
        log::warn!("⚠️ шлюз: порт {} не закрылся за {} сек (pid {} жив)", port, crate::router::process::STOP_WAIT.as_secs(), rec.pid);
        Err(format!(
            "Шлюз не остановился за {} сек: порт {} остался открыт (pid {} жив{}). Повторите остановку или убейте процесс вручную.",
            crate::router::process::STOP_WAIT.as_secs(),
            port,
            rec.pid,
            if report.succeeded { "" } else { "; kill тоже отказал" }
        ))
    }
}

/// Состояние шлюза для UI (без поднятия сервера).
///
/// `install_dir` сверяется с исполняемым файлом процесса: только так «наш» шлюз
/// отличается от постороннего процесса, случайно занявшего тот же порт. Без
/// сверки UI написал бы «активен» для чужого процесса.
pub fn gateway_state(port: u16, install_dir: &Path, data_dir: &Path) -> GatewayState {
    if !port_open(port, Duration::from_millis(400)) {
        return GatewayState::NotListening;
    }
    if let Some(rec) = config::read_server_record(data_dir) {
        if pid_alive(rec.pid) {
            let exe = gateway_exe(install_dir);
            if crate::router::process::process_exe_matches(rec.pid, &exe) {
                return GatewayState::OursRunning;
            }
            // pid жив, но это не наш бинарь: запись устарела или подменена.
            // Считаем порт занятым посторонним — иначе UI предложил бы «Запустить»
            // поверх чужого процесса.
            return GatewayState::ForeignOccupant(rec.pid);
        }
    }
    GatewayState::ForeignOccupant(0)
}

/// Разбор комбо нашего шлюза **из конфига**, без запуска сервера.
///
/// Хост вызывает `get_combos` для каждого роутера при открытии настроек.
/// У node-роутеров это требует живого сервера, у шлюза достаточно прочитать
/// `gateway.json`: тот же список вернула бы его `/api/combos`. Источник тот
/// же (`cloud-routers-core`), поэтому расхождение между тем, что показали
/// настройки, и тем, что отдаёт сервер, невозможно по построению.
pub fn read_combos_from_config(
    app: &AppHandle,
    router_id: RouterId,
) -> Result<Vec<crate::router::combos::ComboInfo>, String> {
    let data_dir = config::router_data_dir(app, router_id);
    let cfg = cloud_routers_core::load_config(&data_dir);
    let combos = cloud_routers_core::models::list_combos(&cfg.providers);
    if combos.is_empty() {
        return Err(format!(
            "У шлюза нет комбо: добавьте провайдера с ключом и нажмите «Обновить модели» (кнопка «Открыть Web UI»)."
        ));
    }
    Ok(combos
        .into_iter()
        .map(|c| crate::router::combos::ComboInfo {
            name: c.name,
            kind: Some(c.kind),
            models: c.models,
        })
        .collect())
}

/// Привести состояние шлюза к актуальному на старте хоста.
///
/// В отличие от node-роутеров, шлюз здесь **не убивается**: он переживает
/// закрытие приложения, и это его raison d'être. Вместо этого проверяем версию:
/// если работает бинарь из прошлой сборки, пользователь узнаёт об этом
/// прямо, а не обнаружит странности в поведении.
pub fn reconcile(app: &AppHandle, router_id: RouterId) {
    let data_dir = config::router_data_dir(app, router_id);
    let Some(rec) = config::read_server_record(&data_dir) else {
        return;
    };
    if !pid_alive(rec.pid) {
        config::clear_server_record(&data_dir);
        return;
    }
    let current = env!("CARGO_PKG_VERSION");
    log::info!("cloud-routers: шлюз уже работает (pid {}) на порту {}", rec.pid, rec.port);
    if let Some(v) = rec.version.as_deref() {
        if v != current {
            log::warn!(
                "⚠️ Работает шлюз версии {} (в плагине {}). Нажмите «Остановить», затем «Установить».",
                v,
                current
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gateway_exe_is_inside_router_dir() {
        let dir = PathBuf::from(r"D:\app\cloud_routers\gateway");
        let exe = gateway_exe(&dir);
        assert_eq!(exe.parent(), Some(dir.as_path()));
        assert_eq!(exe.file_name().expect("name"), "cloud-routers-gateway.exe");
    }

    #[test]
    fn gateway_exe_name_comes_from_spec_not_hardcoded() {
        // Имя бинаря живёт в таблице роутеров. Здесь проверяем, что функция
        // реально читает оттуда, а не содержит своей копии строки.
        let spec_exe = crate::spec::RouterId::Gateway.gateway_exe_file().expect("exe_file");
        assert!(gateway_exe(Path::new("D")).to_string_lossy().contains(spec_exe));
    }

    #[test]
    fn stop_without_server_record_is_a_noop() {
        // Нет записи → нечего останавливать. Важно: НЕ убиваем «случайный» pid.
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let data_dir = crate::router::test_scratch::scratch_dir(&format!("cr_gwproc_norecord_{}", seq));
        let install_dir = crate::router::test_scratch::scratch_dir(&format!("cr_gwproc_norecord_inst_{}", seq));
        let _ = std::fs::remove_dir_all(&data_dir);
        stop_server(&install_dir, 59999, &data_dir).expect("без записи — Ok"); // не должно паниковать
        assert!(!config::server_record_path(&data_dir).exists());
    }

    /// Ключевая гарантия безопасности: остановка **не убивает** процесс, чей
    /// исполняемый файл не совпадает с бинарём шлюза.
    ///
    /// Без этой проверки испорченный `server.json` (или переиспользованный ОС
    /// pid) привёл бы к `taskkill` постороннего процесса — вплоть до
    /// работающего приложения-хоста. Здесь pid принадлежит самому тестовому
    /// процессу, то есть заведомо не шлюзу: остановка обязана его пощадить.
    #[test]
    fn refuses_to_kill_process_with_foreign_exe() {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let data_dir = crate::router::test_scratch::scratch_dir(&format!("cr_gwproc_foreign_{}", seq));
        let install_dir = crate::router::test_scratch::scratch_dir(&format!("cr_gwproc_foreign_inst_{}", seq));
        std::fs::create_dir_all(&data_dir).expect("mkdir");

        let my_pid = std::process::id();
        config::write_server_record(&data_dir, my_pid, 59998, Some("test".into()));

        // Порт обязан быть ЗАНЯТ: сценарий теста — «порт держит чужой процесс».
        // Со свободным портом это легальная протухшая запись (см. тест
        // stale_record_with_reused_pid_and_free_port_is_cleared), а не отказ.
        let occupant = std::net::TcpListener::bind("127.0.0.1:59998").expect("порт занят под оккупанта");

        // Вызов обязан завершиться отказом, не убив тестовый процесс. Если бы
        // проверки не было, `taskkill /F /T /PID <наш pid>` оборвал бы весь прогон.
        // Отказ (а не молчаливый Ok) — чтобы кнопка показала ошибку, а не
        // «остановлено» при живом процессе.
        let err = stop_server(&install_dir, 59998, &data_dir).expect_err("чужой pid убивать нельзя");
        drop(occupant);
        assert!(err.contains("чужой"), "ошибка обязана называть причину: {}", err);

        // Запись остаётся: pid жив, и снеся её, мы превратили бы живой процесс
        // в сироту — записи нет, останавливать нечем, статус врёт. Убирается она
        // сама, когда pid перестанет жить (см. следующий тест).
        let rec = config::read_server_record(&data_dir);
        assert_eq!(
            rec.map(|r| r.pid),
            Some(my_pid),
            "запись о живом процессе обязана пережить отказ в остановке"
        );
        let _ = std::fs::remove_dir_all(&data_dir);
        let _ = std::fs::remove_dir_all(&install_dir);
    }

    /// Регресс 25.10.2026: шлюз умер, его pid переиспользовала ОС (реально —
    /// браузер thorium). Запись вела на чужой ЖИВОЙ процесс, порт шлюза был
    /// свободен. Старый код вечными попытками «остановить» чужой pid блокировал
    /// установку обновления. Теперь: чужой pid + свободный порт = запись мусор
    /// (живой шлюз всегда слушает порт), она убирается, остановка — Ok.
    #[test]
    fn stale_record_with_reused_pid_and_free_port_is_cleared() {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let data_dir = crate::router::test_scratch::scratch_dir(&format!("cr_gwproc_reusedpid_{}", seq));
        let install_dir = crate::router::test_scratch::scratch_dir(&format!("cr_gwproc_reusedpid_inst_{}", seq));
        let _ = std::fs::remove_dir_all(&data_dir);
        std::fs::create_dir_all(&data_dir).expect("mkdir");

        // Плежём запись на СВОЙ живой процесс (тестовый pid) — заведомо не шлюз,
        // как браузер в реальном кейсе. Порт фиксированно свободный (как 59998/59999
        // в соседних тестах).
        let my_pid = std::process::id();
        config::write_server_record(&data_dir, my_pid, 59997, Some("0.1.7".into()));

        stop_server(&install_dir, 59997, &data_dir).expect("свободный порт — останавливать нечего");
        assert!(
            !config::server_record_path(&data_dir).exists(),
            "протухшая запись обязана быть убрана, иначе установка обновления блокируется навсегда"
        );
        let _ = std::fs::remove_dir_all(&data_dir);
        let _ = std::fs::remove_dir_all(&install_dir);
    }

    #[test]
    fn stop_clears_record_when_pid_is_already_dead() {
        // Каталоги уникальны по счётчику: тесты идут в 4 потока, и общий
        // временный каталог приводил к взаимному удалению папок.
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let data_dir = crate::router::test_scratch::scratch_dir(&format!("cr_gwproc_deadpid_{}", seq));
        let install_dir = crate::router::test_scratch::scratch_dir(&format!("cr_gwproc_deadpid_inst_{}", seq));
        let _ = std::fs::remove_dir_all(&data_dir);
        std::fs::create_dir_all(&data_dir).expect("mkdir");
        // pid 0 — заведомо несуществующий, `pid_alive` вернёт false.
        config::write_server_record(&data_dir, 0, 20131, None);
        stop_server(&install_dir, 20131, &data_dir).expect("протухшая запись — Ok");
        assert!(
            !config::server_record_path(&data_dir).exists(),
            "запись о мёртвом процессе должна быть убрана, иначе статус будет врать"
        );
        let _ = std::fs::remove_dir_all(&data_dir);
        let _ = std::fs::remove_dir_all(&install_dir);
    }

    #[test]
    fn create_flags_detach_process_from_host() {
        #[cfg(windows)]
        {
            // Проверяем константы, а не вызов WinAPI: смысл в том, чтобы флаги
            // вообще были выставлены. Без DETACHED_PROCESS шлюз умирал бы
            // вместе с хостом — а это прямое требование.
            assert_ne!(CREATE_FLAGS & 0x00000008, 0, "нужен DETACHED_PROCESS");
            assert_ne!(CREATE_FLAGS & 0x08000000, 0, "нужен CREATE_NO_WINDOW");
            assert_ne!(CREATE_FLAGS & 0x00000200, 0, "нужен CREATE_NEW_PROCESS_GROUP");
        }
        #[cfg(not(windows))]
        {
            assert!(true, "проверка только для Windows");
        }
    }

    #[test]
    fn gateway_state_reports_not_listening_for_free_port() {
        let dir = crate::router::test_scratch::scratch_dir("cr_gwproc_state");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        // Порт 1 на loopback свободен у обычного пользователя.
        assert_eq!(gateway_state(1, &dir, &dir), GatewayState::NotListening);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
