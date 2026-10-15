//! Точка входа: разбор аргументов и диспетчеризация инструментов.
//!
//! Здесь нет бизнес-логики инструментов — только поиск нужного модуля в
//! [`agent_tools::tools`] и передача ему полного `argv`. Новый инструмент
//! добавляется в реестр `tools/mod.rs` и не требует правок этого файла.
//!
//! Коды возврата: 0 — успех, 1 — лимит исчерпан, 2 — ошибка ввода, 3 — сеть/API,
//! 4 — внутренняя ошибка (см. `agent_tools::exit`).

use agent_tools::exit::Exit;
use agent_tools::tools::{Tool, ALL};

fn main() -> std::process::ExitCode {
    logging_init();
    let argv: Vec<String> = std::env::args().collect();
    tool_exit(argv).into()
}

fn logging_init() {
    agent_tools::logging::init();
}

/// Выбрать инструмент и выполнить его; код возврата — контракт с агентом.
fn tool_exit(argv: Vec<String>) -> Exit {
    let rest: &[String] = argv.get(1..).unwrap_or(&[]);

    match rest.first().map(String::as_str) {
        None => {
            print_usage();
            Exit::Ok
        }
        Some("--help" | "-h" | "help") => {
            print_usage();
            Exit::Ok
        }
        Some("--version" | "-V") => {
            println!("agent-tools {}", env!("CARGO_PKG_VERSION"));
            Exit::Ok
        }
        _ => match Tool::from_args(rest) {
            Some(tool) => tool.run(argv),
            None => {
                eprintln!("Неизвестный инструмент: {}", rest[0]);
                print_usage();
                Exit::Input
            }
        },
    }
}

fn print_usage() {
    println!(
        "{}\n\nUSAGE:\n    agent-tools <инструмент> [аргументы]\n\nИНСТРУМЕНТЫ:",
        env!("CARGO_PKG_DESCRIPTION")
    );
    for tool in ALL {
        println!("    {:<10}{}", tool.name(), tool.about());
    }
    println!("\n    agent-tools consult --help — справка по инструменту");
}