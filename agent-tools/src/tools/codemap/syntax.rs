//! Статус синтаксиса ОДНОГО файла — общий слой для `code check` и lint-gate.
//!
//! Зачем отдельный слой: [`super::parse::tags_of`] отвечает на вопрос «какие
//! символы в файле», и для TS осознанно терпит разбор с восстановлением —
//! диагностики уходят в `log::warn`, а карта строится дальше по
//! восстановленному AST. Вопрос проверки — «есть ли синтаксическая ошибка» —
//! на это не отвечает: файл с восстановленным AST дал бы ложный «порядок».
//!
//! Поэтому проверка живёт здесь и использует ТЕ ЖЕ парсеры, что и бэкенды
//! карты (syn / oxc / rustpython), но читает их диагностики ошибок. Второй
//! парсер запрещён так же, как в [`super::lookup`]: проверка и карта обязаны
//! говорить об одном файле одинаково, иначе «= порядок» и «= ошибка» расходятся.
//!
//! Линейка координат: [`LineIndex`] держит 1-based строки по байтовым
//! смещениям; у syn позиция приходит из proc-macro2 (уже строка), у oxc —
//! смещение внутри labels диагностики, у rustpython — байтовый офсет ошибки.

use std::path::Path;

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use rustpython_parser::Parse;

use super::lang::Lang;
use super::lookup::{LookupError, extension_of, resolve};
use super::text::{Content, LineIndex, read_content};
use super::ts_lang::source_type_of;

/// Синтаксический вердикт одного файла.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxStatus {
    /// Файл разбирается без синтаксических ошибок.
    Valid,
    /// Есть ошибка разбора: строка (1-based) и текст первой ошибки.
    Invalid {
        line: u32,
        message: String,
    },
    /// Парсера для этого расширения нет (`.md`, `.json`, …) — проверка
    /// неприменима. Это не «файл сломан», это «мы не обязаны про него судить».
    Unsupported,
}

/// Проверить синтаксис строки исходника выбранным бэкендом.
///
/// Сообщения ошибок не содержат ни имени файла, ни префиксов бэкенда:
/// координату (строку) агенты видят отдельным полем, а путь — той обёрткой,
/// которая знает контекст (`code check`, lint-gate).
pub fn inspect(rel_path: &str, source: &str, lang: Lang) -> SyntaxStatus {
    match lang {
        Lang::Rust => rust_status(source),
        Lang::TsJs => ts_status(rel_path, source),
        Lang::Python => python_status(rel_path, source),
        Lang::Flat => SyntaxStatus::Unsupported,
    }
}

/// Проверить файл проекта по пути (относительному от корня либо абсолютному
/// внутри корня — те же правила, что у [`super::lookup::FileView::open`]).
///
/// Классы ошибок наследуют семантику разведки: [`LookupError::Missing`] —
/// файла нет (для гейта это норма нового файла), `Input` — запрос не имеет
/// смысла (вне корня, бинарник), `Failed` — файл есть, но нечитаем.
pub fn inspect_file(root: &Path, file: &str) -> Result<SyntaxStatus, LookupError> {
    let (path, rel_path) = resolve(root, file)?;
    let source = match read_content(&path) {
        Content::Text(source) => source,
        Content::Binary => {
            return Err(LookupError::Input(format!(
                "{rel_path}: это не текстовый файл — синтакс-проверка применима только к исходникам"
            )));
        }
        Content::Unreadable(reason) => {
            return Err(LookupError::Failed(format!(
                "{rel_path}: не удалось прочитать файл: {reason}"
            )));
        }
    };
    let lang = Lang::of_extension(&extension_of(&path));
    Ok(inspect(&rel_path, &source, lang))
}

/// Rust: `syn` — тот же вызов, что и в [`super::rust_lang::tags`].
///
/// `syn::Error::span().start()` отдаёт позицию первой ошибки прямо строками
/// исходника (proc-macro2 в fallback-режиме держит позицию того буфера, из
/// которого строился разбор): отдельный `LineIndex` не нужен.
fn rust_status(source: &str) -> SyntaxStatus {
    match syn::parse_file(source) {
        Ok(_) => SyntaxStatus::Valid,
        Err(error) => SyntaxStatus::Invalid {
            line: error.span().start().line as u32,
            message: error.to_string(),
        },
    }
}

/// TS/JS: `oxc` — тот же парсер, что в [`super::ts_lang::tags`].
///
/// Отличие в трактовке диагностик: карта терпит восстановление (обязана —
/// иначе один битый файл выкинул бы граф целиком), проверка — нет: любая
/// диагностика уровня `Severity::Error` делает файл невалидным. Символьная
/// семантика (`SemanticBuilder`) здесь не строится: проверка синтаксиса не
/// платит за неё время.
///
/// Ветка `Err` от `source_type_of` недостижима по набору расширений
/// [`Lang::of_extension`]: всё, что даёт `Lang::TsJs`, там же разобрано в
/// `SourceType`. Оставляем честное `Unsupported`, а не панику: если список
/// разъедется, файл не уронит прогон, а войдёт в «не проверяем».
fn ts_status(rel_path: &str, source: &str) -> SyntaxStatus {
    let source_type = match source_type_of(Lang::TsJs, rel_path) {
        Ok(kind) => kind,
        Err(_reason) => return SyntaxStatus::Unsupported,
    };
    let allocator = Allocator::default();
    let parse = Parser::new(&allocator, source, source_type).parse();
    let Some(error) = parse.diagnostics.errors().next() else {
        return SyntaxStatus::Valid;
    };
    let index = LineIndex::new(source);
    // У oxc-ошибки обычно есть label с офсетом; если нет — строка неизвестна,
    // честнее поставить 1 и оставить текст ошибки, чем выкинуть факт целиком.
    let line = error
        .labels
        .first()
        .map(|label| index.line_of(label.offset()))
        .unwrap_or(1);
    SyntaxStatus::Invalid {
        line,
        message: error.message.to_string(),
    }
}

/// Python: `rustpython-parser` — тот же вызов, что и в [`super::py_lang::tags`].
fn python_status(rel_path: &str, source: &str) -> SyntaxStatus {
    match rustpython_parser::ast::Suite::parse(source, rel_path) {
        Ok(_) => SyntaxStatus::Valid,
        Err(error) => {
            let index = LineIndex::new(source);
            SyntaxStatus::Invalid {
                line: index.line_of(error.offset.to_u32()),
                message: error.error.to_string(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Rust -------------------------------------------------------------------

    #[test]
    fn rust_valid_source_is_valid() {
        let status = inspect("a.rs", "fn main() { let x = 1; }\n", Lang::Rust);
        assert_eq!(status, SyntaxStatus::Valid);
    }

    #[test]
    fn rust_first_error_carries_its_line() {
        // syn отдаёт ПЕРВУЮ ошибку: обе строки сломаны, координата — строка 2.
        let source = "fn broken() {\n    let x = ;\n}\n";
        let status = inspect("a.rs", source, Lang::Rust);
        match status {
            SyntaxStatus::Invalid { line, message } => {
                assert_eq!(line, 2, "ошибка — на второй строке");
                assert!(!message.is_empty(), "текст ошибки обязателен: {message}");
            }
            other => panic!("ожидался Invalid, получен {other:?}"),
        }
    }

    #[test]
    fn rust_parse_on_line_one_reports_one() {
        let status = inspect("a.rs", "fn broken( {}\n", Lang::Rust);
        match status {
            SyntaxStatus::Invalid { line, .. } => assert_eq!(line, 1),
            other => panic!("ожидался Invalid, получен {other:?}"),
        }
    }

    // --- TS/JS ------------------------------------------------------------------

    #[test]
    fn ts_valid_source_is_valid() {
        let status = inspect("a.ts", "const value: string = \"text\";\n", Lang::TsJs);
        assert_eq!(status, SyntaxStatus::Valid);
    }

    #[test]
    fn ts_syntax_error_is_not_hidden_behind_recovery() {
        // `let = ;` — синтаксическая ошибка; oxc восстанавливает разбор,
        // и карта строится дальше, но check обязан назвать это Invalid.
        let source = "const ok = 1;\nlet = ;\nconst after = 2;\n";
        let status = inspect("a.ts", source, Lang::TsJs);
        match status {
            SyntaxStatus::Invalid { line, message } => {
                assert_eq!(line, 2, "ошибка — на второй строке");
                assert!(!message.is_empty(), "текст ошибки обязателен: {message}");
            }
            other => panic!("восстановленный AST не обязан выглядеть валидным: {other:?}"),
        }
    }

    // --- Python -----------------------------------------------------------------

    #[test]
    fn python_valid_source_is_valid() {
        let status = inspect("a.py", "value = \"text\"\n", Lang::Python);
        assert_eq!(status, SyntaxStatus::Valid);
    }

    #[test]
    fn python_error_carries_its_line() {
        let source = "value = 1\nbroken = (\n";
        let status = inspect("a.py", source, Lang::Python);
        match status {
            SyntaxStatus::Invalid { line, message } => {
                assert_eq!(line, 2, "незакрытая скобка — вторая строка");
                assert!(!message.is_empty(), "текст ошибки обязателен: {message}");
            }
            other => panic!("ожидался Invalid, получен {other:?}"),
        }
    }

    // --- Flat -------------------------------------------------------------------

    #[test]
    fn flat_languages_are_explicitly_unsupported() {
        // Не «ошибка» и не «валидно»: про `.md` мы честно ничего не судим.
        assert_eq!(inspect("note.md", "# Заголовок\n", Lang::Flat), SyntaxStatus::Unsupported);
        assert_eq!(inspect("data.json", "{}", Lang::Flat), SyntaxStatus::Unsupported);
    }

    // --- inspect_file -----------------------------------------------------------

    fn write_temp(root: &Path, rel: &str, content: &[u8]) -> std::path::PathBuf {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, content).unwrap();
        path
    }

    /// Есть ли файл к моменту вызова — существование проверяют сами тесты,
    ///Каталог создаётся один раз на тест.
    fn tmpdir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "agent_tools_syntax_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn unreadable_lines_are_not_confirmed_by_file_path() {
        let dir = tmpdir();
        write_temp(&dir, "a.rs", b"fn main() {}\n");
        assert_eq!(inspect_file(&dir, "a.rs"), Ok(SyntaxStatus::Valid));
        write_temp(&dir, "b.rs", b"fn broken( {}\n");
        assert!(
            matches!(inspect_file(&dir, "b.rs"), Ok(SyntaxStatus::Invalid { .. })),
            "сломанный rs-файл обязан показать Invalid"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_distinguishes_from_other_input_errors() {
        let dir = tmpdir();
        let error = inspect_file(&dir, "definitely-missing.rs").unwrap_err();
        assert!(
            matches!(error, LookupError::Missing(_)),
            "отсутствие файла — отдельный вариант: {error}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn binary_file_is_an_input_error_and_flat_is_verified_as_unsupported() {
        let dir = tmpdir();
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(&[0x00, 0x01, 0x02, 0x03]);
        write_temp(&dir, "pic.png", &png);
        let error = inspect_file(&dir, "pic.png").unwrap_err();
        assert!(
            matches!(error, LookupError::Input(_)),
            "бинарник не новая операция разведки, а неверный запрос: {error}"
        );
        write_temp(&dir, "note.md", b"# title\n");
        assert_eq!(inspect_file(&dir, "note.md"), Ok(SyntaxStatus::Unsupported));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
