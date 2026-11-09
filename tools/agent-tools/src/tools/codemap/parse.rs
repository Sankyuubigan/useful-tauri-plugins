//! Единая точка выбора бэкенда по языку.
//!
//! Существует, чтобы маршрутизация «файл → парсер» жила в одном месте. Если
//! каждый вызывающий код будет решать это сам, через месяц одно и то же
//! расширение начнут читать по-разному, а расхождение будет выглядеть как
//! «индекс построен, но пуст».

use super::lang::Lang;
use super::md_lang;
use super::py_lang;
use super::rust_lang;
use super::tags::Tag;
use super::ts_lang;

/// Собрать теги файла выбранным бэкендом.
///
/// Для [`Lang::Flat`] возвращается пустой список **без** ошибки: у файла без
/// AST-парсера нет символов, и это факт, а не сбой. Вызывающий код по пустому
/// списку отправляет файл в плоский индекс.
///
/// Ошибка означает, что файл заявлен как исходник, но не разобран: такая
/// ситуация обязана быть видна, а не растворяться в пустом результате.
pub fn tags_of(rel_path: &str, source: &str, lang: Lang) -> Result<Vec<Tag>, String> {
    match lang {
        Lang::Rust => rust_lang::tags(rel_path, source),
        Lang::TsJs => ts_lang::tags(rel_path, source, lang),
        Lang::Python => py_lang::tags(rel_path, source),
        Lang::Markdown => Ok(md_lang::tags(rel_path, source)),
        Lang::Flat => Ok(super::flat_lang::tags(rel_path, source)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_language_uses_its_own_backend() {
        let rust = tags_of("a.rs", "fn alpha() {}", Lang::Rust).expect("rust");
        assert!(rust.iter().any(|t| t.name == "alpha"));

        let ts = tags_of("a.ts", "export function alpha() {}", Lang::TsJs).expect("ts");
        assert!(ts.iter().any(|t| t.name == "alpha"));

        let py = tags_of("a.py", "def alpha():\n    pass\n", Lang::Python).expect("python");
        assert!(py.iter().any(|t| t.name == "alpha"));
    }

    #[test]
    fn markdown_yields_heading_definitions() {
        let tags = tags_of("doc.md", "# Правила\n\n## Установка\n", Lang::Markdown)
            .expect("markdown разбирается без ошибок");
        let names: Vec<&str> = tags.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["Правила", "Установка"]);
    }

    #[test]
    fn flat_language_yields_nothing_without_error() {
        // Не «ошибка разбора», а «символов нет»: иначе произвольный текст
        // сыпал бы предупреждениями на каждом файле без парсера.
        let tags = tags_of("a.cfg", "ключ = значение", Lang::Flat).expect("flat не падает");
        assert!(tags.is_empty());
    }

    #[test]
    fn broken_source_reports_an_error_instead_of_emptiness() {
        // Пустой результат при ошибке был бы ложью: агент решил бы, что в файле
        // нет символов, и потерял бы об этом файл.
        assert!(tags_of("a.py", "def oops(:\n", Lang::Python).is_err());
        assert!(tags_of("a.rs", "fn ((( nope", Lang::Rust).is_err());
    }
}