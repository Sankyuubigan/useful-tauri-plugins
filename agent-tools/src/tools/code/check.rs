//! Команда `code check`: синтаксис ОДНОГО файла.
//!
//! Назначение — быстрая самопроверка агента без запуска компилятора: «ранспорт
//! файла не сломан». Это НЕ линт и не типовая проверка: про стиль и типы
//! команда молчит, и делать вид, что она ими занимается, было бы враньём.
//!
//! Источник истины — тот же слой [`crate::tools::codemap::syntax`], что у
//! карты и lint-gate: те же парсеры, те же диагностики. Дублирующий разбор
//! файла здесь запрещён.
//!
//! ## Исходы (коды выхода)
//!
//! - 0 — файл разобрался без синтаксических ошибок;
//! - 2 — запрос не имеет смысла: файла нет, путь вне корня, файл бинарный;
//! - 4 — проверка провалилась: `SyntaxStatus::Invalid` (ошибка разбора),
//!   `Unsupported` (нет парсера для расширения) или файл нечитаем.

use std::path::PathBuf;

use clap::Args;

use crate::exit::Exit;
use crate::tools::codemap::lookup::LookupError;
use crate::tools::codemap::syntax::{SyntaxStatus, inspect_file};

/// Аргументы `code check`.
#[derive(Debug, Args)]
pub struct CheckArgs {
    /// Корень проекта: база относительных путей (по умолчанию текущий каталог).
    #[arg(long, value_name = "PATH")]
    pub root: Option<PathBuf>,
    /// Файл: путь от корня (или абсолютный внутри корня).
    #[arg(value_name = "FILE")]
    pub file: String,
}

/// Разобрать аргументы и вернуть код процесса.
///
/// Печать здесь не «лог», а единственный ответ агента: `stdout` — успех,
/// `stderr` — причина отказа. Число — то, по чему агент принимает решение;
/// текст — только для человека.
pub fn run(args: CheckArgs) -> Exit {
    let root = args.root.unwrap_or_else(|| PathBuf::from("."));
    let file = args.file;
    run_at(root, file)
}

/// Слой исполнения для тестов: корень уже разрешён.
fn run_at(root: PathBuf, file: String) -> Exit {
    match inspect_file(&root, &file) {
        Ok(SyntaxStatus::Valid) => {
            println!("{file}: синтаксис в порядке");
            Exit::Ok
        }
        Ok(SyntaxStatus::Invalid { line, message }) => {
            eprintln!("{file}: синтаксическая ошибка — строка {line}: {message}");
            Exit::Internal
        }
        Ok(SyntaxStatus::Unsupported) => {
            eprintln!("{file}: синтакс-проверка не поддерживается (нет парсера для этого расширения)");
            Exit::Internal
        }
        Err(LookupError::Missing(reason)) | Err(LookupError::Input(reason)) => {
            eprintln!("code check: {reason}");
            Exit::Input
        }
        Err(LookupError::Failed(reason)) => {
            eprintln!("code check: {reason}");
            Exit::Internal
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn tmpdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("agent_tools_check_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(root: &Path, rel: &str, content: &str) -> PathBuf {
        let path = root.join(rel);
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn valid_rust_file_prints_ok_and_returns_zero() {
        let dir = tmpdir("valid");
        write(&dir, "a.rs", "fn main() {}\n");
        let exit = run_at(dir.clone(), "a.rs".to_string());
        assert_eq!(exit, Exit::Ok);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn broken_file_returns_internal_with_line() {
        let dir = tmpdir("broken");
        write(&dir, "broken.rs", "fn main( {}\n");
        let exit = run_at(dir.clone(), "broken.rs".to_string());
        assert_eq!(exit, Exit::Internal, "ошибка разбора — отказ исполнения (код 4)");
        // Координата строки протестируется на слое syntax.rs (означает, что
        // она приходит в Invalid); здесь проверяется только исход.
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_file_is_an_input_error() {
        let dir = tmpdir("missing");
        let exit = run_at(dir.clone(), "definitely-absent.rs".to_string());
        assert_eq!(exit, Exit::Input, "файла нет — ошибка запроса (код 2)");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unsupported_extension_is_honest_rejection() {
        let dir = tmpdir("unsupported");
        write(&dir, "note.md", "# тренировка\n");
        let exit = run_at(dir.clone(), "note.md".to_string());
        assert_eq!(exit, Exit::Internal, "неподдерживаемый язык — отказ, не успех");
        fs::remove_dir_all(&dir).unwrap();
    }
}
