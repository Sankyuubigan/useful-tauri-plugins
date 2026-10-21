//! Разбор ОДНОГО файла — общий слой для операций разведки.
//!
//! Зачем отдельный модуль: и «скелет файла», и «тело символа» начинаются
//! одинаково — открыть файл, определить язык, разобрать его тегами. Если это
//! повторить в двух местах, то со временем появятся две разные трактовки
//! «что считать файлом» и «где взять строки», и операции начнут расходиться
//! между собой при одинаковом входе.
//!
//! Разбор здесь — **тот же**, что и в индексе ([`super::parse::tags_of`]):
//! второй парсер запрещён, иначе карта и разведка будут говорить о коде разное.

use std::fmt;
use std::path::{Path, PathBuf};

use super::lang::Lang;
use super::parse;
use super::tags::{Tag, TagKind};
use super::text;

/// Класс отказа операции разведки.
///
/// Разделение не формальное: «файла нет» и «символа нет» — это ошибка
/// запроса (агент указал не то, код 2), а «файл есть, но не читается или не
/// разобран» — отказ исполнения (код 4). Смешивать их в один код значит
/// отличить опечатку агента от сломанного репозитория только по тексту.
///
/// `Missing` держит тот же код исхода (ошибка запроса), но выделен отдельным
/// вариантом: для разведки отсутствие файла — отказ, а для lint-gate (снимок
/// baseline ДО записи) «файла ещё нет» — норма нового файла. Гейт различает
/// это по типу, а не разбором текста сообщения.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LookupError {
    /// Запрос неверен: файла или символа нет.
    Input(String),
    /// Файл найден, но прочитать или разобрать его не удалось.
    Failed(String),
    /// Файла нет по указанному пути.
    Missing(String),
}

impl LookupError {
    /// Короткий текст причины — без префикса.
    pub fn reason(&self) -> &str {
        match self {
            LookupError::Input(reason)
            | LookupError::Failed(reason)
            | LookupError::Missing(reason) => reason,
        }
    }
}

impl fmt::Display for LookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.reason())
    }
}

/// Разобранный файл: исходник по строкам плюс теги символов.
#[derive(Debug)]
pub struct FileView {
    /// Путь файла относительно корня (со `/` вместо `\`).
    pub rel_path: String,
    /// Язык, выбранный по расширению.
    pub lang: Lang,
    /// Строки исходника без завершающих переводов строк.
    pub lines: Vec<String>,
    /// Теги символов файла (определения и ссылки).
    pub tags: Vec<Tag>,
}

impl FileView {
    /// Открыть файл и разобрать его.
    ///
    /// Путь принимается относительно корня проекта (обычный случай) либо
    /// абсолютный (когда агент уже знает полный путь). Файл за пределами
    /// корня — отказ: операция обещает смотреть «проект», а не произвольное
    /// место на диске.
    pub fn open(root: &Path, file: &str) -> Result<FileView, LookupError> {
        let (path, rel_path) = resolve(root, file)?;
        let source = match text::read_content(&path) {
            text::Content::Text(source) => source,
            text::Content::Binary => {
                return Err(LookupError::Input(format!(
                    "{rel_path}: это не текстовый файл — скелет и тело строятся только по исходному коду"
                )));
            }
            text::Content::Unreadable(reason) => {
                return Err(LookupError::Failed(format!("{rel_path}: не удалось прочитать файл: {reason}")));
            }
        };
        let lang = Lang::of_extension(&extension_of(&path));
        let tags = parse::tags_of(&rel_path, &source, lang)
            .map_err(|reason| LookupError::Failed(format!("{rel_path}: {reason}")))?;
        Ok(FileView {
            rel_path,
            lang,
            lines: source.lines().map(str::to_string).collect(),
            tags,
        })
    }

    /// Определения файла — по возрастанию строки.
    pub fn definitions(&self) -> Vec<&Tag> {
        let mut defs: Vec<&Tag> = self.tags.iter().filter(|t| t.kind == TagKind::Def).collect();
        defs.sort_by_key(|tag| tag.line);
        defs
    }

    /// Импорты файла: (номер строки, текст строки).
    ///
    /// Импорт в тегах **не помечен отдельным видом**, и полагаться на теги
    /// здесь нельзя: у TS-бэкенда ссылка на импортированное имя указывает на
    /// место ИСПОЛЬЗОВАНИЯ (строка `helper(...)`), а не на строку импорта, так
    /// что отбор «тег-ссылка + строка-импорт» для TS не даёт ничего.
    ///
    /// Поэтому импорты отбираются по строкам исходника — по ключевому слову
    /// языка (`use`, `import`, `from`, `export … from`). Это текстовый признак,
    /// а не второй AST-разбор: назначение строки узнаётся по её собственному
    /// тексту, разбор кода остаётся в бэкендах.
    ///
    /// Плата за такой признак известна и названа: строка, начинающаяся с
    /// `use`/`import`/`from`, попадёт в список импортов даже если это обычный
    /// код. Ошибка здесь косметическая — лишняя строка в списке импортов.
    pub fn imports(&self) -> Vec<(u32, String)> {
        self.lines
            .iter()
            .enumerate()
            .filter(|(_, text)| is_import_line(text))
            .map(|(index, text)| (index as u32 + 1, text.clone()))
            .collect()
    }

    /// Текст строки по её номеру (1-based).
    pub fn line_text(&self, line: u32) -> Option<&str> {
        if line == 0 {
            return None;
        }
        self.lines.get(line as usize - 1).map(String::as_str)
    }
}

/// Найти файл внутри корня и привести путь к относительному.
///
/// Абсолютный путь принимается, только если он внутри корня: иначе операция
/// «посмотреть скелет файла проекта» превратилась бы в чтение чего угодно.
///
/// Отсутствие файла отличено от прочих отказов запроса: гейт различает это
/// по [`LookupError::Missing`], а не по разбору текста сообщения.
pub(crate) fn resolve(root: &Path, file: &str) -> Result<(PathBuf, String), LookupError> {
    let trimmed = file.trim();
    if trimmed.is_empty() {
        return Err(LookupError::Input("не указан файл".to_string()));
    }
    let candidate = Path::new(trimmed);
    let absolute = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        root.join(candidate)
    };
    let normalized = normalize(&absolute);
    let normalized_root = normalize(root);
    if !normalized.starts_with(&normalized_root) {
        return Err(LookupError::Input(format!(
            "{trimmed}: путь вне корня проекта (корень: {})",
            root.display()
        )));
    }
    let rel_path = normalized
        .strip_prefix(&normalized_root)
        .unwrap_or(&normalized)
        .to_string_lossy()
        .replace('\\', "/");
    if !normalized.is_file() {
        return Err(LookupError::Missing(format!(
            "{rel_path}: файла нет (корень: {})",
            root.display()
        )));
    }
    Ok((normalized, rel_path))
}

/// Нормализовать путь без обращения к файловой системе.
///
/// `canonicalize` требует существования пути и на Windows добавляет `\\?\`,
/// который потом не совпадает с путями агента. Поэтому нормализация
/// выполняется по правилам сегментов: `.` убирается, `..` поднимает вверх.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for segment in path.components() {
        match segment {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Расширение файла без точки, в нижнем регистре.
pub(crate) fn extension_of(path: &Path) -> String {
    path.extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// Является ли строка строкой импорта.
///
/// Правила по языкам различаются, но все они начинаются с ключевого слова:
/// Rust — `use`, TypeScript/JavaScript — `import` и реэкспорт `export … from`,
/// Python — `import` и `from … import`.
fn is_import_line(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with("use ")
        || trimmed.starts_with("use{")
        || trimmed.starts_with("import ")
        || trimmed.starts_with("import{")
        || trimmed.starts_with("from ")
        || (trimmed.starts_with("export ") && trimmed.contains(" from "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_lines_are_recognized_per_language() {
        assert!(is_import_line("use std::fmt;"));
        assert!(is_import_line("    use crate::a::b;"));
        assert!(is_import_line("import { a } from './a';"));
        assert!(is_import_line("from a import b"));
        assert!(is_import_line("export { a } from './a';"));
        assert!(!is_import_line("fn alpha() {}"));
        assert!(!is_import_line("let value = 1;"));
    }

    #[test]
    fn normalize_removes_dot_and_parent_segments() {
        assert_eq!(
            normalize(Path::new("a/./b/../c")),
            PathBuf::from("a/c"),
            "точка и родительский сегмент сворачиваются"
        );
    }

    #[test]
    fn error_classes_are_distinct() {
        let input = LookupError::Input("нет".to_string());
        let failed = LookupError::Failed("сломан".to_string());
        let missing = LookupError::Missing("не появился".to_string());
        assert_eq!(input.reason(), "нет");
        assert_eq!(failed.reason(), "сломан");
        assert_eq!(missing.reason(), "не появился");
        assert_ne!(input, failed);
        assert_ne!(input, missing);
    }

    /// Отсутствующий файл — ошибка запроса (код 2), но при этом отдельный
    /// вариант `Missing`: lint-gate снимает baseline ДО записи, когда файла
    /// ещё нет, и обязан отличать это по типу, а не разбирать текст.
    #[test]
    fn missing_file_is_a_request_error() {
        let root = std::env::temp_dir();
        let error = FileView::open(&root, "definitely-missing-42.rs").unwrap_err();
        assert!(
            matches!(error, LookupError::Missing(_)),
            "отсутствующий файл — отдельный вариант ошибки запроса: {error}"
        );
    }

    #[test]
    fn path_outside_root_is_refused() {
        let root = std::env::temp_dir().join("codemap_lookup_root");
        std::fs::create_dir_all(&root).expect("каталог");
        let error = FileView::open(&root, "../вне-корня.rs").unwrap_err();
        assert!(
            matches!(error, LookupError::Input(_)),
            "путь вне корня — отказ: {error}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
