//! Инструмент `consult` — эскалация отчёта рабочего агента к умной модели.
//!
//! Концепция: дешёвый рабочий агент заходит в тупик, пишет отчёт и (с согласия
//! пользователя) вызывает этот инструмент. Тот отправляет отчёт умной модели
//! через локальный OpenAI-совместимый шлюз 9Router, печатает её ответ и
//! выходит. Каждый вызов стоит денег, поэтому вызовы жёстко ограничены счётчиком
//! в `.consult_state.json`; при исчерпании лимита инструмент **не выполняет**
//! запрос, печатает директиву остановки и завершает процесс с кодом 1.
//!
//! Поток выполнения (`agent-tools consult report.md`):
//!
//! ```text
//! 1. служебные флаги (--status/--reset/--set-limit) → ответ и выход, сеть не трогаем
//! 2. чтение отчёта (файл / `-` / stdin) → пусто или слишком велик: код 2
//! 3. reserve() — атомарный резерв вызова; лимит исчерпан → баннер в stderr, код 1
//! 4. POST /v1/chat/completions → ошибка сети/API: refund(), код 3
//! 5. успех → шапка с расходом + чистый ответ модели в stdout, код 0
//! ```

pub mod cli;
pub mod client;
pub mod state;

use std::io::Read;

use clap::Parser;

use crate::exit::Exit;

use self::client::RouterConfig;
use self::state::{ConsultState, Reservation};

/// Системный промпт умной модели (ТЗ).
pub const SYSTEM_PROMPT: &str = "Ты — ведущий эксперт-архитектор. Рабочий агент зашел в тупик и прислал тебе отчет. Проанализируй проблему и дай четкое, исчерпывающее решение или готовый код без лишней воды.";

/// Предохранитель на размер отчёта (байт). 250k токенов — это единицы мегабайт;
/// ограничение защищает от бесконечного stdin и от файла-ошибки на десятки гигабайт.
pub const MAX_REPORT_BYTES: u64 = 64 * 1024 * 1024;

/// Разобрать `argv` и выполнить инструмент. Единая точка входа для
/// [`crate::tools::Tool`] — отсюда возвращается и код процесса.
pub fn run(argv: Vec<String>) -> Exit {
    match cli::ConsultParser::try_parse_from(argv) {
        Ok(parsed) => match parsed.command {
            cli::ConsultSub::Consult(args) => execute(&args),
        },
        Err(err) => {
            // clap сам печатает справку/ошибку в нужный поток (--help → stdout).
            let _ = err.print();
            if err.exit_code() == 0 {
                Exit::Ok
            } else {
                Exit::Input
            }
        }
    }
}

/// Сам инструмент: разбор намерения и весь сценарий выполнения.
fn execute(args: &cli::ConsultArgs) -> Exit {
    let path = args.state_path();

    match args.intent() {
        cli::Intent::Status => report_status(&path),
        cli::Intent::Reset => {
            let mut state = ConsultState::load_or_default(&path);
            state.reset();
            match state.save(&path) {
                Ok(()) => {
                    log::info!("consult: счётчик сброшен в {}", path.display());
                    println!("Счётчик вызовов сброшен. {}", state.status_line());
                    Exit::Ok
                }
                Err(err) => internal_error(&err),
            }
        }
        cli::Intent::SetLimit(limit) => {
            let mut state = ConsultState::load_or_default(&path);
            state.set_limit(limit);
            match state.save(&path) {
                Ok(()) => {
                    log::info!("consult: лимит вырос до {} в {}", limit, path.display());
                    println!("Лимит вызовов: {}. {}", limit, state.status_line());
                    Exit::Ok
                }
                Err(err) => internal_error(&err),
            }
        }
        cli::Intent::Ask { report } => {
            let report = match read_report(report.as_deref()) {
                Ok(text) => text,
                Err(err) => {
                    log::error!("consult: отчёт не принят: {}", err);
                    eprintln!("[ОШИБКА ВВОДА] {}", err);
                    return Exit::Input;
                }
            };
            let config = RouterConfig::resolve(
                args.base_url.as_deref(),
                args.model.as_deref(),
                None,
            );
            ask(&path, &config, &report)
        }
    }
}

/// Запрос к умной модели с резервом, отказом и возвратом вызова при ошибке.
fn ask(path: &std::path::Path, config: &RouterConfig, report: &str) -> Exit {
    let state = match ConsultState::reserve(path) {
        Ok(Reservation::Reserved(state)) => state,
        Ok(Reservation::Exhausted(state)) => {
            log::warn!(
                "consult: лимит исчерпан ({}/{}), запрос не выполнен",
                state.calls,
                state.limit
            );
            eprint!("{}", stop_directive(&state));
            return Exit::LimitExceeded;
        }
        Err(err) => return internal_error(&err),
    };

    log::info!(
        "consult: вызов {}/{}, модель {}, шлюз {}",
        state.calls,
        state.limit,
        config.model,
        config.chat_url()
    );

    let answer = match client::ask(config, SYSTEM_PROMPT, report) {
        Ok(answer) => answer,
        Err(err) => {
            // Неудачная попытка не должна стоить лимита: вызов возвращается.
            if let Err(refund_err) = ConsultState::refund(path) {
                log::error!("consult: не удалось вернуть вызов: {}", refund_err);
            }
            log::error!("consult: запрос к эксперту не удался: {}", err);
            eprintln!("[ОШИБКА СЕТИ/API] {}", err);
            return Exit::Network;
        }
    };

    println!(
        "[ОТВЕТ ЭКСПЕРТА | Использовано вызовов: {}/{}, осталось: {}]",
        state.calls,
        state.limit,
        state.remaining()
    );
    println!();
    println!("{}", answer);
    Exit::Ok
}

fn report_status(path: &std::path::Path) -> Exit {
    match ConsultState::load(path) {
        Ok(state) => {
            println!("{}", state.status_line());
            Exit::Ok
        }
        Err(err) => {
            eprintln!("[ОШИБКА СОСТОЯНИЯ] {}", err);
            eprintln!("Починить: agent-tools consult --reset");
            Exit::Internal
        }
    }
}

fn internal_error(err: &state::StateError) -> Exit {
    log::error!("consult: внутренняя ошибка состояния: {}", err);
    eprintln!("[ОШИБКА СОСТОЯНИЯ] {}", err);
    Exit::Internal
}

/// Директива остановки — то, что читает агент вместо решения (ТЗ).
fn stop_directive(state: &ConsultState) -> String {
    format!(
        "====================== КРИТИЧЕСКАЯ ОСТАНОВКА ======================\n\
         [LIMIT_EXCEEDED]: Лимит обращений к эксперту исчерпан ({}/{})!\n\
         ДИРЕКТИВА ДЛЯ АГЕНТА:\n\
         Тебе СТРОГО ЗАПРЕЩЕНО предпринимать дальнейшие действия по задаче.\n\
         Ты зашел в тупик. Прекрати выполнение bash-команд и правок кода.\n\
         Немедленно напиши пользователю отчет о тупике и жди его решения.\n\
         ===================================================================\n",
        state.calls, state.limit
    )
}

/// Ошибка чтения отчёта — всегда ввод (exit 2).
#[derive(Debug, thiserror::Error)]
pub enum ReportError {
    #[error("файл отчёта не найден: {0}")]
    MissingFile(String),
    #[error("не удалось прочитать {what}: {reason}")]
    Read { what: String, reason: String },
    #[error("отчёт ({what}) не является текстом UTF-8")]
    NotUtf8 { what: String },
    #[error("отчёт пуст ({what}) — нечего отправлять эксперту")]
    Empty { what: String },
    #[error("отчёт ({what}) больше {max} байт ({size} байт) — уменьшите его")]
    TooLarge {
        what: String,
        max: u64,
        size: u64,
    },
}

/// Прочитать отчёт: из файла, из `-` или из stdin. Пустой отчёт — ошибка ввода.
fn read_report(source: Option<&str>) -> Result<String, ReportError> {
    let report = match source.map(str::trim).filter(|s| !s.is_empty()) {
        None => read_stdin()?,
        Some("-") => read_stdin()?,
        Some(path) => read_file(path)?,
    };

    if report.trim().is_empty() {
        return Err(ReportError::Empty {
            what: source.unwrap_or("<stdin>").to_string(),
        });
    }
    Ok(report.trim().to_string())
}

fn read_file(path: &str) -> Result<String, ReportError> {
    let display = path.to_string();
    let metadata = std::fs::metadata(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => ReportError::MissingFile(display.clone()),
        _ => ReportError::Read {
            what: display.clone(),
            reason: e.to_string(),
        },
    })?;

    if metadata.is_dir() {
        return Err(ReportError::Read {
            what: display,
            reason: "это каталог, а не файл".to_string(),
        });
    }
    if metadata.len() > MAX_REPORT_BYTES {
        return Err(ReportError::TooLarge {
            what: display,
            max: MAX_REPORT_BYTES,
            size: metadata.len(),
        });
    }

    // Размер известен заранее — выделяем буфер целиком, без роста с перевыделением.
    let mut text = String::with_capacity(metadata.len() as usize);
    let read = std::fs::File::open(path).and_then(|mut file| file.read_to_string(&mut text));
    read.map_err(|e| ReportError::Read {
        what: display,
        reason: e.to_string(),
    })?;
    Ok(text)
}

fn read_stdin() -> Result<String, ReportError> {
    let what = "<stdin>";
    // take(MAX+1): лишний байт отличает «ровно лимит» от «больше лимита» и не
    // даёт втянуть в память бесконечный поток.
    let mut buffer = Vec::new();
    std::io::stdin()
        .lock()
        .take(MAX_REPORT_BYTES + 1)
        .read_to_end(&mut buffer)
        .map_err(|e| ReportError::Read {
            what: what.to_string(),
            reason: e.to_string(),
        })?;

    if buffer.len() as u64 > MAX_REPORT_BYTES {
        return Err(ReportError::TooLarge {
            what: what.to_string(),
            max: MAX_REPORT_BYTES,
            size: buffer.len() as u64,
        });
    }
    String::from_utf8(buffer).map_err(|_| ReportError::NotUtf8 {
        what: what.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(calls: u32, limit: u32) -> ConsultState {
        ConsultState { calls, limit }
    }

    #[test]
    fn stop_directive_contains_contract_markers() {
        let text = stop_directive(&state(5, 5));

        assert!(text.starts_with("====================== КРИТИЧЕСКАЯ ОСТАНОВКА ====="));
        assert!(text.contains("[LIMIT_EXCEEDED]: Лимит обращений к эксперту исчерпан (5/5)!"));
        assert!(text.contains("ДИРЕКТИВА ДЛЯ АГЕНТА:"));
        assert!(text.contains("Тебе СТРОГО ЗАПРЕЩЕНО"));
        assert!(text.contains("Прекрати выполнение bash-команд и правок кода."));
        assert!(text.trim_end().ends_with("=================================================================="));
    }

    #[test]
    fn stop_directive_reports_actual_numbers() {
        let text = stop_directive(&state(3, 3));
        assert!(text.contains("(3/3)"));
    }

    #[test]
    fn report_limit_fits_large_prompts() {
        // 250k токенов — единицы мегабайт; предохранитель обязан быть заметно выше.
        const _: () = assert!(MAX_REPORT_BYTES > 8 * 1024 * 1024);
    }

    #[test]
    fn system_prompt_is_the_specified_one() {
        assert!(SYSTEM_PROMPT.starts_with("Ты — ведущий эксперт-архитектор."));
        assert!(SYSTEM_PROMPT.contains("без лишней воды"));
    }

    #[test]
    fn missing_file_is_an_input_error() {
        let err = read_report(Some("definitely-missing-report.md")).expect_err("файла нет");
        assert!(matches!(err, ReportError::MissingFile(_)));
    }

    #[test]
    fn directory_is_not_a_report() {
        let dir = std::env::temp_dir();
        let err = read_report(Some(&dir.display().to_string())).expect_err("каталог — не отчёт");
        assert!(matches!(err, ReportError::Read { .. }));
    }

    #[test]
    fn read_file_roundtrip() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let path = std::env::temp_dir().join(format!(
            "agent_tools_report_{}_{nanos}.md",
            std::process::id()
        ));
        std::fs::write(&path, "  отчёт агента  ").expect("записать временный отчёт");

        let text = read_report(Some(&path.display().to_string())).expect("прочитать отчёт");

        assert_eq!(text, "отчёт агента", "крайние пробелы срезаются");
        let _ = std::fs::remove_file(&path);
    }
}