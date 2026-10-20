//! Реестр инструментов набора.
//!
//! Каждый инструмент — модуль с собственным CLI и своим состоянием. Чтобы
//! добавить новый (например `review` или `testgen`), достаточно:
//! 1. создать `tools/<name>/` с `cli.rs` и `run(args) -> Exit`;
//! 2. добавить вариант в [`Tool`] и его разбор в [`Tool::from_args`].
//!
//! `main.rs` о содержимом инструментов не знает и меняться не должен.

use crate::exit::Exit;

pub mod codemap;
pub mod consult;
pub mod websearch;

/// Реестр инструментов: единственный список, по которому печатается справка
/// и который обходится при добавлении нового инструмента.
pub const ALL: &[Tool] = &[Tool::Consult, Tool::Codemap, Tool::Websearch];

/// Инструмент набора.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    /// Эскалация отчёта рабочего агента к умной модели (первый инструмент).
    Consult,
    /// Карта репозитория: AST → лексика → граф → сжатая карта под бюджет.
    Codemap,
    /// Мульти-движковый поиск в интернете без API-ключей (порт web_search.ts).
    Websearch,
}

impl Tool {
    /// Имя инструмента в CLI.
    pub fn name(self) -> &'static str {
        match self {
            Tool::Consult => "consult",
            Tool::Codemap => "codemap",
            Tool::Websearch => "websearch",
        }
    }

    /// Короткое описание для `--help`.
    pub fn about(self) -> &'static str {
        match self {
            Tool::Consult => {
                "Отправить отчёт рабочего агента умной модели (эскалация) с жёстким лимитом вызовов"
            }
            Tool::Codemap => {
                "Построить карту репозитория: AST, граф, ранжирование символов, карта под бюджет токенов"
            }
            Tool::Websearch => {
                "Поиск в интернете по 11 движкам без API-ключей (chain/parallel, multi-query, MCP-режим)"
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
            Some("codemap") => Some(Tool::Codemap),
            Some("websearch") => Some(Tool::Websearch),
            _ => None,
        }
    }

    /// Выполнить инструмент. Все инструменты возвращают [`Exit`] — тот же
    /// контракт, что у процесса.
    pub fn run(self, args: Vec<String>) -> Exit {
        match self {
            Tool::Consult => consult::run(args),
            Tool::Codemap => codemap::run(args),
            Tool::Websearch => websearch::run(args),
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
        assert_eq!(Tool::from_args(&args(&["codemap"])), Some(Tool::Codemap));
        assert_eq!(Tool::from_args(&args(&["websearch"])), Some(Tool::Websearch));
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
        assert_eq!(Tool::Codemap.name(), "codemap");
        assert!(!Tool::Codemap.about().is_empty());
        assert_eq!(Tool::Websearch.name(), "websearch");
        assert!(!Tool::Websearch.about().is_empty());
    }

    #[test]
    fn registry_lists_every_tool() {
        assert_eq!(
            ALL,
            &[Tool::Consult, Tool::Codemap, Tool::Websearch],
            "новый инструмент обязан попасть в ALL"
        );
    }
}