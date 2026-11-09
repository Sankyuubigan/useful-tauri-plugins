//! Скелет одного файла: импорты и сигнатуры с номерами строк.
//!
//! Отвечает на вопрос «что вообще есть в этом файле», не отдавая его тело.
//! Это дешевле карты проекта (разбирается один файл, а не весь репозиторий)
//! и точнее карты (карта отбирает символы по рангу, а скелет показывает все).

use super::lookup::FileView;

/// Собрать скелет файла.
///
/// Порядок разделов и формат строк — те же, что у карты проекта
/// (`{номер:>5} | {текст}`), чтобы агент, читавший карту, читал и скелет
/// без переучивания.
pub fn skeleton(view: &FileView) -> String {
    let mut out = format!(
        "Файл: {} ({}, {} строк)\n",
        view.rel_path,
        view.lang.name(),
        view.lines.len()
    );

    let imports = view.imports();
    out.push_str(&format!("\nИмпорты ({}):\n", imports.len()));
    if imports.is_empty() {
        out.push_str("  (импортов не найдено)\n");
    }
    for (line, text) in &imports {
        out.push_str(&format!("{line:>5} | {text}\n"));
    }

    let defs = view.definitions();
    out.push_str(&format!("\nОпределения ({}):\n", defs.len()));
    if defs.is_empty() {
        out.push_str("  (определений не найдено)\n");
    }
    for tag in &defs {
        // Текст строки — как в исходнике, без обрезки отступов: отступ показывает
        // вложенность (метод внутри `impl` виден по отступу, а не по имени).
        let text = view.line_text(tag.line).unwrap_or("");
        out.push_str(&format!("{:>5} | {}\n", tag.line, text));
    }

    out.push_str(
        "\nДальше: тело конкретного символа — вызови body для этого файла и имени \
         символа из списка определений.\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::codemap::lookup::FileView;

    const SOURCE: &str = "\
use std::fmt;

pub struct Service {
    field: u32,
}

impl Service {
    pub fn run(&self, input: u32) -> u32 {
        input
    }
}

pub fn entry() -> u32 {
    Service { field: 0 }.run(1)
}
";

    fn view(source: &str) -> FileView {
        // Каталог уникален на вызов: тесты идут параллельно, и общий каталог
        // означал бы, что один тест перезаписывает фикстуру другого.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let dir = std::env::temp_dir().join(format!(
            "codemap_skeleton_{}_{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("каталог");
        let path = dir.join("sample.rs");
        std::fs::write(&path, source).expect("фикстура");
        let view = FileView::open(&dir, "sample.rs").expect("разбор");
        let _ = std::fs::remove_dir_all(&dir);
        view
    }

    #[test]
    fn skeleton_lists_imports_and_signatures_with_lines() {
        let text = skeleton(&view(SOURCE));
        assert!(text.contains("use std::fmt;"), "импорт виден: {text}");
        assert!(text.contains("pub struct Service {"), "структура видна: {text}");
        assert!(text.contains("pub fn entry() -> u32 {"), "функция видна: {text}");
        assert!(text.contains("pub fn run(&self, input: u32) -> u32 {"), "метод виден: {text}");
    }

    #[test]
    fn skeleton_numbers_lines_like_the_map_does() {
        let text = skeleton(&view(SOURCE));
        // `use std::fmt;` — первая строка файла, `pub struct Service` — третья.
        assert!(text.contains("    1 | use std::fmt;"), "номер строки импорта: {text}");
        assert!(text.contains("    3 | pub struct Service {"), "номер строки структуры: {text}");
    }

    #[test]
    fn skeleton_says_how_to_continue() {
        let text = skeleton(&view(SOURCE));
        assert!(
            text.contains("body"),
            "скилет подсказывает следующий шаг: {text}"
        );
    }

    #[test]
    fn file_without_imports_says_so_instead_of_leaving_an_empty_section() {
        let text = skeleton(&view("pub fn only() {}\n"));
        assert!(text.contains("(импортов не найдено)"), "пустой раздел честен: {text}");
        assert!(text.contains("pub fn only() {}"), "определение на месте: {text}");
    }
}
