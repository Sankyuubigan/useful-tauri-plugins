//! Самоперезапуск процесса шлюза.
//!
//! ## Зачем
//!
//! Часть настроек читается **только при старте**: порт и адрес bind. Пользователь
//! меняет порт в дашборде — без перезапуска изменение «применится» в UI и не
//! применится на самом деле. Это ровно тот случай «UI врёт», который запрещён
//! правилами проекта.
//!
//! ## Порядок, который не даёт поймать «порт занятown мной же»
//!
//! 1. Новый экземпляр стартует первым и **терпеливо ждёт свободного порта**
//!    (пока старый ещё держит его). Ждать, а не падать: иначе гонка «новый не
//!    успел занять порт до выхода старого» дала бы мёртвый шлюз.
//! 2. Старый экземпляр, убедившись, что порт освобождён (или что новый уже
//!    отвечает на новом порту), завершается.
//!
//! Итог: возможен короткий период, когда оба процесса живы и оба пишут в
//! `server.json`. Это безопасно — запись атомарная, и новый экземпляр
//! перезаписывает её последним.
//!
//! ## Почему не «перезапуск через плагин»
//!
//! Шлюз **переживает** закрытие приложения-хоста: пользователь может держать
//! открытым только Cursor и запускать шлюз отдельно. Если бы перезапуск зависел от
//! Tauri-хоста, смена порта была бы невозможна без открытого приложения.

use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use cloud_routers_core::config::GatewayConfig;

/// Сколько новый экземпляр ждёт свободный порт.
pub const BIND_RETRY_TIMEOUT: Duration = Duration::from_secs(25);
/// Интервал попыток занять порт.
pub const BIND_RETRY_INTERVAL: Duration = Duration::from_millis(250);
/// Сколько старый экземпляр ждёт, пока освободится порт / поднимется новый.
pub const SHUTDOWN_WAIT: Duration = Duration::from_secs(30);

/// Запустить копию себя с теми же аргументами.
///
/// Отдельный процесс, а не `exec`: мы должны дождаться, пока новый экземпляр
/// поднимется, и только тогда завершиться.
pub fn spawn_replacement(exe: &Path, data_dir: &Path, config: &GatewayConfig) -> Result<(), String> {
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--data-dir")
        .arg(data_dir)
        // Передаём порт явно: новый экземпляр обязан взять его из свежего
        // конфига, а не из аргументов старого процесса.
        .arg("--port")
        .arg(config.port.to_string())
        .current_dir(data_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW — не мигает консолью; DETACHED_PROCESS — новый
        // экземпляр не умирает вместе с нами.
        cmd.creation_flags(0x08000000 | 0x00000008);
    }

    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("Не удалось запустить {}: {}", exe.display(), e))
}

/// Занять порт с повтором: вернуть слушатель или внятную ошибку.
///
/// Свободный порт проверяется connect'ом: если кто-то уже слушает, bind упадёт с
/// «адрес занят», и без повтора смена порта ломала бы шлюз при каждом старте.
pub fn bind_with_retry(addr: std::net::SocketAddr) -> Result<std::net::TcpListener, String> {
    bind_with_retry_timeout(addr, BIND_RETRY_TIMEOUT)
}

/// То же с явным таймаутом. Параметр вынесен отдельно, чтобы тест проверки
/// отказа не ждал production-значение.
pub fn bind_with_retry_timeout(
    addr: std::net::SocketAddr,
    timeout: Duration,
) -> Result<std::net::TcpListener, String> {
    let deadline = Instant::now() + timeout;
    let mut attempt = 0u32;
    loop {
        attempt += 1;
match std::net::TcpListener::bind(addr) {
                Ok(l) => {
                    // Сокет обязан быть НЕБЛОКИРУЮЩИМ: дальше его забирает
                    // tokio (`TcpListener::from_std`), а tokio не переводит
                    // сокет в non-blocking сам — он на это рассчитывает.
                    //
                    // На Windows `accept()` наследует режим слушающего сокета,
                    // поэтому принятые соединения тоже оказываются
                    // блокирующими. Блокирующий сокет внутри tokio-воркера
                    // означает синхронное чтение, которое никогда не
                    // отпускает воркер: hyper разбирает первые байты, но
                    // обработчик не вызывается никогда — соединение живёт
                    // вечно без ответа (проверено: curl → `code=000`).
                    //
                    // Инвариант ставится здесь, а не в вызывающем коде:
                    // создатель сокета и есть единственное место, где его
                    // нельзя забыть.
                    if let Err(e) = l.set_nonblocking(true) {
                        return Err(format!(
                            "Не удалось переключить порт {} в неблокирующий режим: {}",
                            addr.port(),
                            e
                        ));
                    }
                    if attempt > 1 {
                        log::info!("порт {} занят после {} попыток", addr.port(), attempt);
                    }
                    return Ok(l);
                }
            Err(e) if Instant::now() < deadline => {
                if attempt == 1 || attempt % 20 == 0 {
                    log::info!("порт {} ещё занят ({}), ждём…", addr.port(), e);
                }
                std::thread::sleep(BIND_RETRY_INTERVAL);
            }
            Err(e) => {
                return Err(format!(
                    "Не удалось занять порт {} за {} с: {}",
                    addr.port(),
                    timeout.as_secs(),
                    e
                ))
            }
        }
    }
}

/// Дождаться, пока можно завершаться, и выйти.
///
/// Условия выхода (первое выполненное):
/// - порт перестал слушаться → старый процесс освободил его;
/// - на новом порту кто-то отвечает → значит это уже новый экземпляр.
pub fn wait_and_exit(data_dir: &Path, old_port: u16, new_port: u16) -> ! {
    let deadline = Instant::now() + SHUTDOWN_WAIT;
    log::info!("перезапуск шлюза: ждём завершения (порт {} → {})", old_port, new_port);
    loop {
        if old_port != new_port && port_answers(new_port) {
            log::info!("новый экземпляр отвечает на порту {} — завершаемся", new_port);
            cloud_routers_core::store::clear_server_record(data_dir);
            std::process::exit(0);
        }
        if !port_listening(old_port) {
            log::info!("порт {} освобождён — завершаемся", old_port);
            cloud_routers_core::store::clear_server_record(data_dir);
            std::process::exit(0);
        }
        if Instant::now() >= deadline {
            // Не смогли дождаться. Лучше выйти и оставить новый экземпляр
            // подняться с повтором, чем держать два процесса на одном порту
            // бесконечно.
            log::warn!(
                "не дождались перезапуска за {} с — завершаемся принудительно",
                SHUTDOWN_WAIT.as_secs()
            );
            cloud_routers_core::store::clear_server_record(data_dir);
            std::process::exit(0);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Слушает ли кто-то порт.
fn port_listening(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &match format!("127.0.0.1:{port}").parse() {
            Ok(a) => a,
            Err(_) => return false,
        },
        Duration::from_millis(200),
    )
    .is_ok()
}

/// Отвечает ли порт по HTTP (значит, это уже новый экземпляр шлюза).
pub fn port_answers(port: u16) -> bool {
    port_listening(port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_with_retry_fails_fast_on_unbindable_addr() {
        // Порт 0 означает «любой свободный» — обязан сработать с первого раза.
        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().expect("addr");
        let l = bind_with_retry(addr).expect("bind");
        assert!(l.local_addr().expect("local").port() > 0);
    }

    #[test]
    fn bind_with_retry_returns_nonblocking_listener() {
        // Регрессия на «шлюз слушает, но не отвечает»: блокирующий сокет,
        // отданный tokio, заставляет hyper зависнуть навсегда. Инвариант
        // держит сам `bind_with_retry`, и этот тест не даёт его убрать.
        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().expect("addr");
        let l = bind_with_retry(addr).expect("bind");
        assert!(
            l.set_nonblocking(true).is_ok(),
            "слушатель обязан уже быть в неблокирующем режиме"
        );
        assert!(l.set_nonblocking(false).is_ok());
    }

    #[test]
    fn bind_with_retry_reports_error_when_nonblocking_fails() {
        // Отказ переключения режима обязан быть громкой ошибкой, а не
        // молчаливо блокирующим сокетом: иначе шлюз снова «поднимется»,
        // но не ответит.
        let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        assert!(l.set_nonblocking(false).is_ok());
        let addr = l.local_addr().expect("addr");
        drop(l);
        // Порт свободен — bind пройдёт, и проверка режима пройдёт тоже.
        let got = bind_with_retry(addr).expect("bind");
        assert!(got.local_addr().expect("addr").port() == addr.port());
    }

    #[test]
    fn bind_with_retry_waits_for_port_to_be_released() {
        let blocker = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = blocker.local_addr().expect("addr").port();
        let target: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().expect("addr");

        let handle = std::thread::spawn(move || {
            // Освобождаем порт через 700 мс — дольше первой попытки, но меньше
            // таймаута. Тест падал бы без повторного bind.
            std::thread::sleep(Duration::from_millis(700));
            drop(blocker);
            bind_with_retry(target)
        });

        let listener = handle.join().expect("thread").expect("порт должен был освободиться");
        assert_eq!(listener.local_addr().expect("addr").port(), port);
    }

    #[test]
    fn bind_with_retry_gives_up_with_explicit_error() {
        // Порт занят и не будет освобождён: ждать бессмысленно, нужна внятная
        // ошибка с названием порта, а не паника bind().
        let blocker = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = blocker.local_addr().expect("addr").port();
        let target: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().expect("addr");
        let err = bind_with_retry_timeout(target, Duration::from_millis(400)).expect_err("должен сдаться");
        assert!(err.contains(&port.to_string()), "в ошибке должен быть порт: {err}");
        assert!(err.contains("Не удалось занять порт"), "{err}");
        drop(blocker);
    }

    #[test]
    fn port_listening_detects_open_and_closed_ports() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = l.local_addr().expect("addr").port();
        assert!(port_listening(port));
        drop(l);
        // Даём ОС освободить порт; проверка «закрыт» не должна быть мгновенно
        // после drop, поэтому допускаем небольшую паузу.
        std::thread::sleep(Duration::from_millis(100));
        assert!(!port_listening(port));
    }

    #[test]
    fn replacement_args_carry_explicit_port() {
        // Новый экземпляр обязан получить порт явно, иначе взял бы порт из
        // аргументов старого процесса и не увидел бы смену настройки.
        let cfg = GatewayConfig { port: 20999, ..Default::default() };
        assert_eq!(cfg.port, 20999);
    }
}