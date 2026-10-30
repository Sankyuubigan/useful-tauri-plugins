//! Бэкенд Markdown: заголовки секций как определения.
//!
//! ## Что здесь разбирается
//!
//! Строки `#`–`###` становятся `Def`-тегами: имя — текст заголовка, строка —
//! позиция в файле. Так документация начинает участвовать в тех же операциях,
//! что и код: `search` находит раздел по названию, карта показывает секцию
//! документации, `skeleton` перечисляет оглавление.
//!
//! Чего здесь НЕТ, и это осознанно:
//!
//! - **Ссылок между документами**. Markdown-переходы `[текст](файл.md)` не
//!   разрешаются в рёбра графа: имя цели — путь, а не символ, и делать вид,
//!   что это связь вызова, значило бы смешивать граф кода с оглавлением.
//! - **Заголовков глубже `###`**. Глубокая вложенность — деталь структуры
//!   документа, а не навигации; в карте она дала бы шум.
//!
//! ## Почему блоки кода пропускаются
//!
//! Строка `# пример` внутри ```-блока — это комментарий кода, а не заголовок.
//! Без отслеживания ограждений заголовки из примеров попадали бы в оглавление
//! и путали бы и карту, и поиск. Отсчёт ограждений ведётся по строкам,
//! начинающимся с ``` (с любым числом leading-пробелов, как разрешает CommonMark).

use super::tags::Tag;

/// Максимальный уровень заголовка, попадающего в индекс (`#` = 1).
const MAX_HEADING_LEVEL: usize = 3;

/// Извлечь определения-заголовки из документа.
///
/// Имя определения — текст заголовка без решёток и завершающих `#`. Ровно так
/// заголовок ищется поиском и печатается картой.
pub fn tags(rel_path: &str, source: &str) -> Vec<Tag> {
    let mut out = Vec::new();
    let mut in_fence = false;

    for (index, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            // Внутри ограждения сменить состояние может только закрывающее
            // ограждение; в незакрытом блоке до конца файла заголовков нет.
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }

        let Some(heading) = heading_of(trimmed) else {
            continue;
        };
        out.push(Tag::def(rel_path, &heading, (index + 1) as u32));
    }
    out
}

/// Уровень и текст заголовка строки, либо `None`.
fn heading_of(line: &str) -> Option<String> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > MAX_HEADING_LEVEL {
        return None;
    }
    // После решёток обязан быть пробел: `#тег` в Markdown — не заголовок,
    // и превращать хештеги в секции значило бы засорять индекс.
    let rest = &line[hashes..];
    if !rest.starts_with(' ') {
        return None;
    }
    let name = rest.trim().trim_end_matches('#').trim();
    if name.is_empty() {
        return None;
    }
    Some(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_become_definitions_with_source_lines() {
        let source = "# Верх\n\nтекст\n\n## Середина\n\n### Низ\n";
        let tags = tags("doc.md", source);
        let names: Vec<(&str, u32)> = tags
            .iter()
            .map(|t| (t.name.as_str(), t.line))
            .collect();
        assert_eq!(
            names,
            vec![("Верх", 1), ("Середина", 5), ("Низ", 7)],
            "заголовки обязаны стать определениями на своих строках: {names:?}"
        );
    }

    #[test]
    fn headings_deeper_than_level_three_are_ignored() {
        let tags = tags("doc.md", "#### деталь\n");
        assert!(tags.is_empty(), "четвёртый уровень — шум: {tags:?}");
    }

    #[test]
    fn headings_inside_code_fences_are_not_sections() {
        let source = "# Настоящий\n\n```rust\n# это комментарий\n## не заголовок\n```\n\n## После блока\n";
        let tags = tags("doc.md", source);
        let names: Vec<&str> = tags.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Настоящий", "После блока"],
            "заголовки из блоков кода обязаны пропускаться: {names:?}"
        );
    }

    #[test]
    fn closing_fence_reopens_headings() {
        let source = "```\n# не заголовок\n```\n# заголовок\n";
        let tags = tags("doc.md", source);
        assert_eq!(tags.len(), 1, "после закрытия блока разбор продолжается: {tags:?}");
        assert_eq!(tags[0].line, 4);
    }

    #[test]
    fn hash_without_space_is_not_a_heading() {
        // `#тег` — хештег, `#5` — номер: превращать их в секции значило бы
        // засорять индекс.
        assert!(tags("doc.md", "#тег\n").is_empty());
        assert!(tags("doc.md", "#5 не заголовок\n").is_empty());
    }

    #[test]
    fn trailing_hashes_are_trimmed_like_closing_atx() {
        let tags = tags("doc.md", "## Заголовок ##\n");
        assert_eq!(tags[0].name, "Заголовок");
    }

    #[test]
    fn indented_heading_is_recognized() {
        // До трёх пробелов отступа заголовком быть не перестаёт (CommonMark).
        let tags = tags("doc.md", "   ## С отступом\n");
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].name, "С отступом");
    }

    #[test]
    fn empty_document_yields_no_tags() {
        assert!(tags("doc.md", "").is_empty());
    }
}
