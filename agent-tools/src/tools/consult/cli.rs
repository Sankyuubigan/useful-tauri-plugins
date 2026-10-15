//! Аргументы сабкоманды `consult`.
//!
//! Отчёт принимается позиционным аргументом (`agent-tools consult report.md`),
//! при его отсутствии читается stdin (`cat report.md | agent-tools consult`).
//! Явное `-` означает «читать stdin» — привычная для Unix-инструментов форма,
//! которой удобно пользоваться агент.
//!
//! Флаги `--model/--base-url/--state-file` перекрывают одноимённые переменные
//! окружения; остальное (ключ, лимит) живёт в окружении и файле состояния.

use clap::{Args, Parser, Subcommand};

use super::state::{ConsultState, STATE_FILE_NAME};

/// Способ разбора одной сабкоманды: имя программы + имя инструмента + аргументы.
///
/// Отдельная обёртка нужна потому, что `Parser` требует собственного `Command`,
/// а `consult` — лишь `Args` (его разбирает `Tool::run` как раз подкоманду).
#[derive(Debug, Parser)]
#[command(name = "agent-tools", bin_name = "agent-tools")]
pub struct ConsultParser {
    #[command(subcommand)]
    pub command: ConsultSub,
}

/// Подкоманды инструмента — пока одна, но список уже открыт для расширения.
#[derive(Debug, Subcommand)]
pub enum ConsultSub {
    /// Отправить отчёт рабочего агента умной модели.
    #[command(
        about = ABOUT,
        long_about = "Отправляет отчёт рабочего агента умной модели через локальный \
OpenAI-совместимый шлюз 9Router. Каждый вызов тратит лимит: при его исчерпании запрос \
не выполняется, инструмент печатает директиву остановки и завершает процесс с кодом 1."
    )]
    Consult(ConsultArgs),
}

#[derive(Debug, Args, Clone)]
pub struct ConsultArgs {
    /// Файл с отчётом рабочего агента. `-` или отсутствие аргумента — stdin.
    #[arg(value_name = "REPORT")]
    pub report: Option<String>,

    /// Базовый адрес шлюза 9Router (перекрывает ROUTER_BASE_URL).
    #[arg(long, value_name = "URL")]
    pub base_url: Option<String>,

    /// Имя умной модели/combo (перекрывает SMART_MODEL_NAME).
    #[arg(long, value_name = "MODEL")]
    pub model: Option<String>,

    /// Файл состояния (по умолчанию ./.consult_state.json).
    #[arg(long, value_name = "PATH")]
    pub state_file: Option<String>,

    /// Установить новый лимит вызовов (служебная команда, сеть не трогает).
    #[arg(long, value_name = "N")]
    pub set_limit: Option<u32>,

    /// Сбросить счётчик вызовов в 0 (служебная команда, сеть не трогает).
    #[arg(long, conflicts_with = "set_limit")]
    pub reset: bool,

    /// Показать статус: «Использовано X из Y. Осталось: Z».
    #[arg(long, conflicts_with_all = ["set_limit", "reset"])]
    pub status: bool,
}

/// Что должен сделать вызов: служебная команда или сам запрос к модели.
///
/// Разбор флагов в типе, а не в рантайме: невозможно случайно выполнить
/// запрос вместо `--status` или совместить взаимоисключающие `--reset/--set-limit`
/// (последнее проверяет clap).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    /// Показать статус и выйти.
    Status,
    /// Сбросить счётчик и выйти.
    Reset,
    /// Установить лимит и выйти.
    SetLimit(u32),
    /// Отправить отчёт умной модели.
    Ask { report: Option<String> },
}

impl ConsultArgs {
    /// Разобрать намерение из аргументов.
    pub fn intent(&self) -> Intent {
        if self.status {
            return Intent::Status;
        }
        if self.reset {
            return Intent::Reset;
        }
        if let Some(limit) = self.set_limit {
            return Intent::SetLimit(limit);
        }
        Intent::Ask {
            report: self.report.clone(),
        }
    }

    /// Путь к файлу состояния: явный флаг, иначе `.consult_state.json` в CWD.
    pub fn state_path(&self) -> std::path::PathBuf {
        match self.state_file.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
            Some(path) => std::path::PathBuf::from(path),
            None => ConsultState::default_path(),
        }
    }

    /// Имя файла состояния для справки и сообщений об ошибках.
    pub fn state_file_name(&self) -> String {
        self.state_path().display().to_string()
    }
}

/// Справка по инструменту (используется в тексте ошибок ввода).
pub const ABOUT: &str = "Отправить отчёт рабочего агента умной модели через шлюз 9Router";

/// Имя файла состояния для текстов, где путь уже подставлен.
pub fn default_state_name() -> &'static str {
    STATE_FILE_NAME
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(extra: &[&str]) -> Result<ConsultArgs, clap::Error> {
        let mut argv = vec!["agent-tools", "consult"];
        argv.extend_from_slice(extra);
        ConsultParser::try_parse_from(argv).map(|parsed| match parsed.command {
            ConsultSub::Consult(args) => args,
        })
    }

    #[test]
    fn report_is_positional() {
        let args = parse(&["report.md"]).expect("разбор отчёта");
        assert_eq!(args.report.as_deref(), Some("report.md"));
        assert_eq!(args.intent(), Intent::Ask { report: Some("report.md".into()) });
    }

    #[test]
    fn without_report_asks_stdin() {
        let args = parse(&[]).expect("разбор без отчёта");
        assert_eq!(args.intent(), Intent::Ask { report: None });
    }

    #[test]
    fn dash_means_stdin() {
        let args = parse(&["-"]).expect("разбор `-`");
        assert_eq!(args.report.as_deref(), Some("-"));
    }

    #[test]
    fn management_flags_pick_the_intent() {
        assert_eq!(parse(&["--status"]).expect("status").intent(), Intent::Status);
        assert_eq!(parse(&["--reset"]).expect("reset").intent(), Intent::Reset);
        assert_eq!(
            parse(&["--set-limit", "12"]).expect("set-limit").intent(),
            Intent::SetLimit(12)
        );
    }

    #[test]
    fn management_flags_win_over_report_argument() {
        // Иначе `consult report.md --status` молча потратил бы вызов.
        let args = parse(&["report.md", "--status"]).expect("status с отчётом");
        assert_eq!(args.intent(), Intent::Status);
    }

    #[test]
    fn conflicting_management_flags_are_rejected() {
        assert!(parse(&["--reset", "--set-limit", "3"]).is_err());
        assert!(parse(&["--status", "--reset"]).is_err());
    }

    #[test]
    fn overrides_are_parsed() {
        let args = parse(&[
            "--model",
            "google-low-tier",
            "--base-url",
            "http://127.0.0.1:20128/v1",
            "--state-file",
            "state.json",
        ])
        .expect("разбор перекрытий");

        assert_eq!(args.model.as_deref(), Some("google-low-tier"));
        assert_eq!(args.base_url.as_deref(), Some("http://127.0.0.1:20128/v1"));
        assert_eq!(
            args.state_path(),
            std::path::PathBuf::from("state.json")
        );
    }

    #[test]
    fn state_path_defaults_to_cwd_file() {
        let args = parse(&[]).expect("разбор без флагов");
        assert_eq!(
            args.state_path(),
            std::path::PathBuf::from(STATE_FILE_NAME)
        );
        assert_eq!(args.state_file_name(), STATE_FILE_NAME);
    }

    #[test]
    fn blank_state_file_is_ignored() {
        let args = parse(&["--state-file", "  "]).expect("разбор пустого пути");
        assert_eq!(
            args.state_path(),
            std::path::PathBuf::from(STATE_FILE_NAME)
        );
    }

    #[test]
    fn zero_limit_is_allowed() {
        assert_eq!(
            parse(&["--set-limit", "0"]).expect("лимит 0").intent(),
            Intent::SetLimit(0)
        );
    }

    #[test]
    fn negative_limit_is_rejected() {
        assert!(parse(&["--set-limit", "-1"]).is_err());
    }

    #[test]
    fn non_numeric_limit_is_rejected() {
        assert!(parse(&["--set-limit", "много"]).is_err());
    }

    #[test]
    fn about_is_not_empty() {
        assert!(!ABOUT.is_empty());
        assert_eq!(default_state_name(), STATE_FILE_NAME);
    }
}