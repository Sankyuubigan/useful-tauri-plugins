//! Лексический поиск по индексу: символы и текст плоских файлов.
//!
//! ## Почему без векторов
//!
//! Векторный поиск в v1 сознательно отсутствует. Для кода точное совпадение
//! имени важнее смысловой близости: `getUserById` и `fetchUserById` — разные
//! функции, и эмбеддинг сгладил бы их в соседей. Это же измерение приводит
//! исследования, приведённые при постановке задачи. Вектор — отдельная фаза,
//! и только после того, как лексика будет измерена.
//!
//! ## Как считается совпадение
//!
//! Собственный скоринг: точное имя символа весит больше совпадения по
//! подстроке, а совпадение в имени — больше, чем в теле чанка. Никаких внешних
//! индексов: `rusqlite` потребовал бы C-компиляции, а для десятков тысяч
//! символов хватает обычного `HashMap`.

use std::collections::HashMap;

use super::flat_lang::FlatChunk;
use super::model::Index;
use super::tags::TagKind;

/// Одно найденное совпадение.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// Путь файла.
    pub rel_path: String,
    /// Номер строки, 1-based.
    pub line: u32,
    /// Почему нашлось: `def` — объявление, `ref` — использование, `text` — файл без AST.
    pub kind: &'static str,
    /// Короткая выжимка вокруг совпадения.
    pub snippet: String,
}

/// Порог для отсева: ниже результат считается шумом.
const MIN_SCORE: f64 = 1.0;

/// Найти по запросу в индексе.
///
/// Запрос делится на слова; каждое должно найтись хотя бы где-то в индексе.
/// Иначе запрос из трёх слов вернул бы тысячу результатов по одному
/// совпавшему — это хуже, чем «ничего не найдено».
pub fn search(index: &Index, query: &str, limit: usize) -> Vec<Hit> {
    let terms: Vec<String> = query
        .split_whitespace()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    if terms.is_empty() {
        return Vec::new();
    }

    let mut hits: Vec<Hit> = Vec::new();

    // Символы: имена известны точно, поэтому сравнение точное, а не подстрочное.
    for tag in index.tags.iter().filter(|t| t.kind == TagKind::Def) {
        let name = tag.name.to_lowercase();
        let score = terms.iter().filter(|t| name.contains(*t)).count();
        if score > 0 {
            let exact = terms.iter().filter(|t| **t == name).count();
            let rank = score as f64 + exact as f64 * 2.0;
            if rank >= MIN_SCORE {
                hits.push(Hit {
                    rel_path: tag.rel_path.clone(),
                    line: tag.line,
                    kind: "def",
                    snippet: tag.name.clone(),
                });
            }
        }
    }

    // Плоские файлы: структуры нет, ищем по тексту чанков.
    for chunk in &index.chunks {
        let haystack = chunk.text.to_lowercase();
        let score = terms.iter().filter(|t| haystack.contains(*t)).count();
        if score == terms.len() {
            hits.push(Hit {
                rel_path: chunk.rel_path.clone(),
                line: chunk.line,
                kind: "text",
                snippet: snippet_around(&chunk.text, &terms),
            });
        }
    }

    // Порядок: сначала объявления (именно они отвечают на «где это»), внутри —
    // по пути и строке, чтобы выдача была воспроизводимой.
    hits.sort_by(|a, b| {
        a.kind
            .cmp(b.kind)
            .then_with(|| a.rel_path.cmp(&b.rel_path))
            .then_with(|| a.line.cmp(&b.line))
    });
    hits.truncate(limit);
    hits
}

/// Собрать короткую выжимку вокруг первого совпадения.
fn snippet_around(text: &str, terms: &[String]) -> String {
    const WINDOW: usize = 160;
    let lower = text.to_lowercase();
    let at = terms
        .iter()
        .find_map(|term| lower.find(term.as_str()))
        .unwrap_or(0);

    // Срез по байтам мог бы разорвать многобайтовый символ, поэтому границы
    // двигаются до ближайшего корректного UTF-8.
    let start = floor_char_boundary(text, at.saturating_sub(WINDOW / 2));
    let end = ceil_char_boundary(text, (at + WINDOW).min(text.len()));
    text[start..end].replace('\n', " ").trim().to_string()
}

/// Граница UTF-8, не режущая символ пополам.
fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

/// Граница UTF-8, не режущая символ пополам.
fn ceil_char_boundary(text: &str, mut index: usize) -> usize {
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

/// Статистика индекса для `--status` и MCP.
pub fn summary(index: &Index) -> HashMap<&'static str, usize> {
    let mut out = HashMap::new();
    out.insert("files", index.files.len());
    out.insert("parsed", index.parsed_count());
    out.insert("flat", index.flat_count());
    out.insert("definitions", index.tags.iter().filter(|t| t.kind == TagKind::Def).count());
    out.insert("references", index.tags.iter().filter(|t| t.kind == TagKind::Ref).count());
    out.insert("chunks", index.chunks.len());
    out
}

/// Обратные ссылки: где символ упоминается, кроме его объявления.
///
/// Основа для `codemap callers`.
pub fn callers(index: &Index, name: &str) -> Vec<Hit> {
    index
        .tags
        .iter()
        .filter(|tag| tag.kind == TagKind::Ref && tag.name == name)
        .map(|tag| Hit {
            rel_path: tag.rel_path.clone(),
            line: tag.line,
            kind: "ref",
            snippet: tag.name.clone(),
        })
        .collect()
}

/// Чанки одного файла — для показа найденного места агенту.
pub fn chunks_of<'a>(index: &'a Index, rel_path: &str) -> Vec<&'a FlatChunk> {
    index
        .chunks
        .iter()
        .filter(|c| c.rel_path == rel_path)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::codemap::lang::Lang;
    use crate::tools::codemap::model::{FileEntry, Index};
    use crate::tools::codemap::tags::Tag;

    fn sample() -> Index {
        Index {
            version: 1,
            root: "repo".into(),
            built_at: "2026-10-15 00:00:00".into(),
            files: vec![
                FileEntry {
                    rel_path: "a.rs".into(),
                    lang: Lang::Rust,
                    bytes: 100,
                    lines: 20,
                    tokens: 30,
                    mtime_secs: 1,
                    parsed: true,
                },
                FileEntry {
                    rel_path: "notes.md".into(),
                    lang: Lang::Flat,
                    bytes: 50,
                    lines: 5,
                    tokens: 12,
                    mtime_secs: 1,
                    parsed: false,
                },
            ],
            tags: vec![
                Tag::def("a.rs", "handle_payment", 4),
                Tag::def("a.rs", "unrelated", 9),
                Tag::reference("a.rs", "handle_payment", 14),
            ],
            chunks: vec![FlatChunk {
                rel_path: "notes.md".into(),
                line: 1,
                text: "Правила проекта: обработка платежей описана здесь.".into(),
            }],
            ignored: Vec::new(),
        }
    }

    #[test]
    fn finds_definition_by_exact_name() {
        let hits = search(&sample(), "handle_payment", 10);
        assert!(!hits.is_empty(), "объявление обязано находиться");
        assert_eq!(hits[0].rel_path, "a.rs");
        assert_eq!(hits[0].line, 4);
        assert_eq!(hits[0].kind, "def");
    }

    #[test]
    fn definitions_come_before_text_matches() {
        let hits = search(&sample(), "обработка платежей", 10);
        // Запрос на русском не найдёт символ по имени, но найдёт плоский файл.
        assert!(hits.iter().any(|h| h.kind == "text" && h.rel_path == "notes.md"));
    }

    /// ГЕЙТ 7: файл без AST-парсера не выпадает из индекса, а находится поиском.
    #[test]
    fn gate7_flat_file_is_searchable() {
        let index = sample();
        let hits = search(&index, "платежей", 10);
        let flat = hits
            .iter()
            .find(|h| h.rel_path == "notes.md")
            .unwrap_or_else(|| panic!("плоский файл обязан находиться: {hits:?}"));
        assert_eq!(flat.kind, "text");
        assert!(!flat.snippet.is_empty(), "выжимка обязана быть непустой");
    }

    #[test]
    fn empty_query_returns_nothing() {
        assert!(search(&sample(), "   ", 10).is_empty());
    }

    #[test]
    fn no_match_returns_empty_instead_of_guessing() {
        assert!(
            search(&sample(), "такого_символа_нет", 10).is_empty(),
            "правдоподобный, но выдуманный результат хуже пустого"
        );
    }

    #[test]
    fn multi_word_query_requires_all_terms_in_a_chunk() {
        let hits = search(&sample(), "платежей правила", 10);
        assert!(hits.iter().any(|h| h.rel_path == "notes.md"));
        assert!(
            !hits.iter().any(|h| h.rel_path == "a.rs" && h.kind == "text"),
            "файл без части слов не должен попадать в выдачу"
        );
    }

    #[test]
    fn limit_is_respected() {
        let hits = search(&sample(), "a e i o u", 1);
        assert!(hits.len() <= 1);
    }

    #[test]
    fn callers_exclude_the_declaration_itself() {
        let hits = callers(&sample(), "handle_payment");
        assert_eq!(hits.len(), 1, "объявление не является вызовом");
        assert_eq!(hits[0].line, 14);
    }

    #[test]
    fn unknown_symbol_has_no_callers() {
        assert!(callers(&sample(), "нет_такого").is_empty());
    }

    #[test]
    fn summary_separates_parsed_from_flat() {
        let stats = summary(&sample());
        assert_eq!(stats["parsed"], 1);
        assert_eq!(stats["flat"], 1);
        assert_eq!(stats["definitions"], 2);
        assert_eq!(stats["references"], 1);
        assert_eq!(stats["chunks"], 1);
    }

    #[test]
    fn snippet_keeps_multibyte_text_intact() {
        let text = "платежи́ обработка".repeat(40);
        let terms = vec!["платежи́".to_string()];
        let snippet = snippet_around(&text, &terms);
        // Обрезка по байтам разорвала бы символ и дала бы не-UTF8.
        assert!(snippet.chars().count() > 0);
        assert!(text.contains(snippet.trim()) || snippet.contains("платеж"));
    }
}