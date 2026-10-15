//! Коды возврата процесса — единый контракт «тул ↔ вызывающий агент».
//!
//! Агент принимает решение по числу, а не по тексту: кодировка сообщений может
//! меняться, а смысл кода — нет. Поэтому каждый исход инструмента обязан
//! сводиться к одному из [`Exit`].

use std::process::ExitCode;

/// Исход выполнения инструмента.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// Успех: ответ получен, счётчик вызовов израсходован.
    Ok = 0,
    /// Лимит вызовов исчерпан — запрос НЕ выполнен, агент обязан остановиться.
    LimitExceeded = 1,
    /// Ошибка ввода/валидации: нет файла отчёта, он пуст, отчёт слишком велик.
    Input = 2,
    /// Ошибка сети или API шлюза: шлюз недоступен, 4xx/5xx, нечитаемый ответ.
    Network = 3,
    /// Внутренняя ошибка инструмента: не прочитано/записано состояние.
    Internal = 4,
}

impl Exit {
    /// Числовой код для документов и тестов (выход `main` даёт то же самое).
    pub fn code(self) -> u8 {
        self as u8
    }
}

impl From<Exit> for ExitCode {
    fn from(exit: Exit) -> Self {
        ExitCode::from(exit.code())
    }
}

impl std::fmt::Display for Exit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Exit::Ok => "успех",
            Exit::LimitExceeded => "лимит исчерпан",
            Exit::Input => "ошибка ввода",
            Exit::Network => "ошибка сети/API",
            Exit::Internal => "внутренняя ошибка",
        };
        f.write_str(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_match_the_agent_contract() {
        assert_eq!(Exit::Ok.code(), 0);
        assert_eq!(Exit::LimitExceeded.code(), 1);
        assert_eq!(Exit::Input.code(), 2);
        assert_eq!(Exit::Network.code(), 3);
        assert_eq!(Exit::Internal.code(), 4);
    }

    #[test]
    fn every_exit_converts_to_process_exit_code() {
        for exit in [
            Exit::Ok,
            Exit::LimitExceeded,
            Exit::Input,
            Exit::Network,
            Exit::Internal,
        ] {
            let _code: ExitCode = exit.into();
        }
    }
}