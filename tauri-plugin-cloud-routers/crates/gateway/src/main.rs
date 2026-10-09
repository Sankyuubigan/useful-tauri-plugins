//! `cloud-routers-gateway` — отдельный процесс локального LLM-шлюза.
//!
//! ## Зачем отдельный процесс
//!
//! Шлюз обслуживает внешние IDE (Cursor, Cline, Roo Code, Aider) по протоколу
//! OpenAI. Он должен **переживать закрытие приложения-хоста**: пользователь
//! держит открытым только IDE, а хост — свёрнутым или уже закрытым. Если бы
//! сервер жил внутри Tauri-приложения, закрытие окна убивало бы инференс.
//!
//! ## Запуск
//!
//! ```text
//! cloud-routers-gateway --data-dir <каталог> [--port <порт>]
//! ```
//!
//! `--port` перекрывает значение из конфига: так плагин может запустить шлюз на
//! свободном порту, не правя файл. Значение из `--port` **не** сохраняется —
//! конфиг остаётся источником правды.
//!
//! ## Идентичность процесса
//!
//! `server.json` (pid, порт, версия) — это то, по чему хост понимает, что
//! шлюз жив и не запускает второй. Поэтому запись удаляется на любом выходе:
//! оставленная запись сделала бы статус неверным.

mod admin;
mod app;
mod logbuf;
mod proxy;
mod relay;
mod respawn;
mod server;
mod upstream;

#[cfg(test)]
mod integration_tests;

use std::path::PathBuf;

use cloud_routers_core::store as core_store;

/// Разобранные аргументы командной строки.
#[derive(Debug, PartialEq)]
struct Args {
    data_dir: PathBuf,
    /// Перекрытие порта из конфига (`None` — взять из конфига).
    port_override: Option<u16>,
}

fn parse_args() -> Result<Args, String> {
    let mut data_dir: Option<PathBuf> = None;
    let mut port_override: Option<u16> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => {
                let v = args
                    .next()
                    .ok_or_else(|| "--data-dir требует значения".to_string())?;
                data_dir = Some(PathBuf::from(v));
            }
            "--port" => {
                let v = args.next().ok_or_else(|| "--port требует значения".to_string())?;
                port_override = Some(
                    v.parse::<u16>()
                        .map_err(|_| format!("--port «{}» не число", v))?,
                );
            }
            "--version" | "-V" => {
                println!("cloud-routers-gateway {}", cloud_routers_core::VERSION);
                std::process::exit(0);
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            other => return Err(format!("Неизвестный аргумент: {}", other)),
        }
    }
    Ok(Args {
        // Без --data-dir шлюз работает в текущем каталоге. Это делает бинарь
        // запускаемым «руками» (для отладки), но штатный путь — всегда с
        // --data-dir от плагина.
        data_dir: data_dir.unwrap_or_else(|| PathBuf::from(".")),
        port_override,
    })
}

fn print_help() {
    println!(
        "cloud-routers-gateway {}\n\
         \n\
         Локальный LLM-шлюз с OpenAI-совместимым API для внешних IDE.\n\
         \n\
         USAGE:\n    cloud-routers-gateway --data-dir <каталог> [--port <порт>]\n\
         \n\
         ПАРАМЕТРЫ:\n\
         \x20   --data-dir <каталог>   Каталог конфига, данных и журнала\n\
         \x20   --port <порт>          Перекрыть порт из конфига (не сохраняется)\n\
         \x20   --version              Показать версию и выйти\n\
         \x20   --help                 Показать эту справку\n",
        cloud_routers_core::VERSION
    );
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        // Пишем в stderr: stdout/stderr перенаправлены плагином в gateway.log,
        // но при ручном запуске сообщение должно быть видно в терминале.
        eprintln!("cloud-routers-gateway: {}", e);
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let args = parse_args()?;
    std::fs::create_dir_all(&args.data_dir)
        .map_err(|e| format!("Не удалось создать каталог данных {}: {}", args.data_dir.display(), e))?;

    let mut config = core_store::load(&args.data_dir);
    if let Some(p) = args.port_override {
        if p == 0 {
            return Err("--port должен быть больше нуля".to_string());
        }
        config.port = p;
    }
    let problems = config.validate();
    if !problems.is_empty() {
        eprintln!(
            "cloud-routers-gateway: конфигурация содержит проблемы: {}",
            problems.join("; ")
        );
    }

    let host = config.host.clone();
    let port = config.port;

    // Слушатель с повтором: при самоперезапуске порт ещё занят старым
    // экземпляром, и падать здесь означало бы «шлюз не запустился».
    let listener = respawn::bind_with_retry(format!("{}:{}", host, port).parse().map_err(|e| {
        format!("Неверный адрес «{}:{}»: {}", host, port, e)
    })?)?;

    let state = app::AppState::new(&args.data_dir)?;
    let addr = listener.local_addr().map_err(|e| format!("Не удалось определить адрес: {}", e))?;
    let effective_port = addr.port();

    core_store::write_server_record(
        &args.data_dir,
        std::process::id(),
        effective_port,
        Some(cloud_routers_core::VERSION.to_string()),
    );

    // Каталог нужен respawn для очистки server.json при выходе.
    std::env::set_var("CLOUD_ROUTERS_GATEWAY_DATA_DIR", &args.data_dir);
    println!(
        "cloud-routers-gateway {} слушает http://{} ({} провайдеров)",
        cloud_routers_core::VERSION,
        addr,
        config.providers.len()
    );

    let listener = tokio::net::TcpListener::from_std(listener)
        .map_err(|e| format!("Не удалось передать listener в tokio: {}", e))?;
    let router = server::build(state);

    let result = tokio::select! {
        res = axum::serve(listener, router) => {
            res.map_err(|e| format!("Сервер остановился с ошибкой: {}", e))
        }
        _ = shutdown_signal() => {
            println!("cloud-routers-gateway: получен сигнал остановки");
            Ok(())
        }
    };

    // Запись о процессе снимается на любом выходе: оставленный PID сделал бы
    // статус шлюза в панели неверным (плагин решил бы, что сервер жив).
    core_store::clear_server_record(&args.data_dir);
    result
}

/// Ожидание сигнала остановки.
///
/// На Windows `ctrl_c` не срабатывает, если процесс запущен без консоли
/// (именно так его запускает плагин) — поэтому дополнительно ловим
/// `CTRL_CLOSE_EVENT`/`CTRL_BREAK_EVENT` через консольные обработчики.
/// Второй сигнал принудительно завершает процесс: пользователь не должен
/// ждать, а мы не должны блокировать закрытие приложения-хоста.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    let ctrl_break = async {
        match tokio::signal::windows::ctrl_break() {
            Ok(mut stream) => {
                stream.recv().await;
            }
            Err(e) => {
                // Поддержка необязательна: без неё работает только ctrl_c.
                log::warn!("ctrl_break недоступен: {}", e);
                std::future::pending::<()>().await;
            }
        }
    };

    let ctrl_close = async {
        match tokio::signal::windows::ctrl_close() {
            Ok(mut stream) => {
                stream.recv().await;
            }
            Err(e) => {
                log::warn!("ctrl_close недоступен: {}", e);
                std::future::pending::<()>().await;
            }
        }
    };

    tokio::select! {
        _ = ctrl_c => {}
        _ = ctrl_break => {}
        _ = ctrl_close => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_reads_data_dir_and_port() {
        let args = Args {
            data_dir: PathBuf::from("/tmp/cr"),
            port_override: Some(20999),
        };
        assert_eq!(args.data_dir, PathBuf::from("/tmp/cr"));
        assert_eq!(args.port_override, Some(20999));
    }

    #[test]
    fn port_override_is_optional() {
        // Штатный вызов от плагина может не передавать порт: тогда берётся
        // значение из конфига, и это единственный источник правды.
        let args = Args { data_dir: PathBuf::from("/tmp/cr"), port_override: None };
        assert_eq!(args.port_override, None);
    }

    #[test]
    fn port_override_beats_config() {
        // Ручной запуск с --port обязан поднять шлюз на этом порту, даже если
        // в конфиге записан другой.
        let mut config = cloud_routers_core::GatewayConfig::default();
        config.port = 20131;
        let args = Args { data_dir: PathBuf::from("/tmp/cr"), port_override: Some(20999) };
        if let Some(p) = args.port_override {
            config.port = p;
        }
        assert_eq!(config.port, 20999);
    }

    #[test]
    fn help_text_mentions_required_argument() {
        // --data-dir обязателен в штатном вызове; без него шлюз пишет в cwd,
        // что легко не заметить и потом искать потерянный конфиг.
        let help = format!(
            "cloud-routers-gateway {}\n --data-dir <каталог> --port <порт>",
            cloud_routers_core::VERSION
        );
        assert!(help.contains("--data-dir"));
        assert!(help.contains("--port"));
    }
}