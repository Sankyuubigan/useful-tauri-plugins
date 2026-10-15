//! Единственный источник правды о том, какой язык понимает индекс.
//!
//! Расширение → бэкенд. Всё остальное (обход, индекс, поиск) спрашивает
//! здесь и больше нигде не содержит списков расширений: добавить язык —
//! одна строка в [`Lang::of_extension`] плюс новый модуль бэкенда.

use serde::{Deserialize, Serialize};

/// Какой бэкенд разбирает файл.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lang {
    /// `syn`: полный AST с узлами-элементами.
    Rust,
    /// `oxc`: полный AST плюс семантический анализ (символьная таблица).
    TsJs,
    /// `rustpython-parser`: полный AST.
    Python,
    /// Парсера нет. Файл не выбрасывается, а попадает в плоский индекс:
    /// инвентарь и текстовые чанки, доступные поиску.
    Flat,
}

impl Lang {
    /// Бэкенд по расширению файла (без точки, в любом регистре).
    ///
    /// Неизвестное расширение — не ошибка и не повод пропустить файл:
    /// оно даёт [`Lang::Flat`], иначе агент потерял бы из индекса половину
    /// репозитория (`.md`, `.toml`, `.json`, `.yml`).
    pub fn of_extension(ext: &str) -> Lang {
        match ext.to_ascii_lowercase().as_str() {
            "rs" => Lang::Rust,
            "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs" => Lang::TsJs,
            "py" | "pyi" => Lang::Python,
            _ => Lang::Flat,
        }
    }

    /// Есть ли структурный парсер.
    ///
    /// У `Flat` его нет, поэтому для него не строится граф вызовов: файл
    /// попадает в поиск как текст, а не как символы. Это различие намеренное:
    /// делать вид, что `.md` разобран, значило бы врать агенту о структуре.
    pub fn has_parser(self) -> bool {
        !matches!(self, Lang::Flat)
    }

    /// Короткое имя для логов и `--help`.
    pub fn name(self) -> &'static str {
        match self {
            Lang::Rust => "rust",
            Lang::TsJs => "ts/js",
            Lang::Python => "python",
            Lang::Flat => "flat",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_languages_map_to_their_backends() {
        assert_eq!(Lang::of_extension("rs"), Lang::Rust);
        assert_eq!(Lang::of_extension("ts"), Lang::TsJs);
        assert_eq!(Lang::of_extension("tsx"), Lang::TsJs);
        assert_eq!(Lang::of_extension("jsx"), Lang::TsJs);
        assert_eq!(Lang::of_extension("py"), Lang::Python);
    }

    #[test]
    fn extension_match_ignores_case() {
        assert_eq!(Lang::of_extension("RS"), Lang::Rust);
        assert_eq!(Lang::of_extension("Py"), Lang::Python);
    }

    #[test]
    fn unknown_extension_falls_back_to_flat_instead_of_disappearing() {
        // `.md` не выбрасывается из индекса — он просто не имеет AST.
        assert_eq!(Lang::of_extension("md"), Lang::Flat);
        assert!(!Lang::of_extension("md").has_parser());
        assert!(Lang::of_extension("rs").has_parser());
    }

    #[test]
    fn names_are_distinct() {
        let all = [Lang::Rust, Lang::TsJs, Lang::Python, Lang::Flat];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a.name(), b.name(), "имена бэкендов обязаны различаться");
            }
        }
    }
}