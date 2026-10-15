//! Реестр инструментов набора.
//!
//! Каждый инструмент — модуль с собственным CLI и своим состоянием. Чтобы
//! добавить новый (например `review` или `testgen`), достаточно:
//! 1. создать `tools/<name>/` с `cli.rs` и `run(args) -> Exit`;
//! 2. добавить вариант в [`Tool`] и его разбор в [`Tool::from_args`].
//!
//! `main.rs` о содержимом инструментов не знает и меняться не должен.

use crate::exit::Exit;

pub mod consult;

/// Реестр инструментов: единственный список, по которому печатается справка
/// и который обходится при добавлении нового инструмента.
pub const ALL: &[Tool] = &[Tool::Consult];

/// Инструмент набора.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    /// Эскалация отчёта рабочего агента к умной модели (первый инструмент).
    Consult,
}

impl Tool {
    /// Имя инструмента в CLI.
    pub fn name(self) -> &'static str {
        match self {
            Tool::Consult => "consult",
        }
    }

    /// Короткое описание для `--help`.
    pub fn about(self) -> &'static str {
        match self {
            Tool::Consult => {
                "Отправить отчёт рабочего агента умной модели (эскалация) с жёстким лимитом вызовов"
            }
        }
    }

    /// Разобрать имя инструмента из аргументов командной строки.
    ///
    /// Пустая строка — не «инструмент по умолчанию», а ошибка ввода: вызов без
    /// подкоманды должен показывать справку, а не молча делать что-то одно.
    pub fn from_args(args: &[String]) -> Option<Self> {
        match args.first().map(String::as_str) {
            Some("consult") => Some(Tool::Consult),
            _ => None,
        }
    }

    /// Выполнить инструмент. Все инструменты возвращают [`Exit`] — тот же
    /// контракт, что у процесса.
    pub fn run(self, args: Vec<String>) -> Exit {
        match self {
            Tool::Consult => consult::run(args),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn registry_parses_known_tool() {
        assert_eq!(Tool::from_args(&args(&["consult"])), Some(Tool::Consult));
    }

    #[test]
    fn registry_rejects_empty_and_unknown() {
        assert_eq!(Tool::from_args(&args(&[])), None);
        assert_eq!(Tool::from_args(&args(&["consolt"])), None);
        assert_eq!(Tool::from_args(&args(&["--help"])), None);
    }

    #[test]
    fn tool_names_are_unique_and_documented() {
        assert_eq!(Tool::Consult.name(), "consult");
        assert!(!Tool::Consult.about().is_empty());
    }

    #[test]
    fn registry_lists_every_tool() {
        assert_eq!(ALL, &[Tool::Consult], "новый инструмент обязан попасть в ALL");
    }
}