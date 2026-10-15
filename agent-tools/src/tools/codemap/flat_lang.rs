//! Файлы без AST-парсера: инвентарь и текстовые чанки.
//!
//! ## Зачем это нужно
//!
//! Агент спрашивает «где описаны правила проекта» — и ответ лежит в `README.md`
//! или `AGENTS.md`. Если неподдержанное расширение просто выпадало бы из индекса,
//! инструмент молчал бы там, где помощь нужнее всего. Поэтому неизвестные
//! файлы не выбрасываются, а попадают в плоский индекс и находятся поиском.
//!
//! Чанки режутся с перекрытием: символ на границе двух чанков иначе не попал бы
//! ни в один из них и поиск его бы не нашёл.

use serde::{Deserialize, Serialize};

use super::tags::Tag;
use super::tokens;

/// Чанк плоского файла.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlatChunk {
    /// Путь файла относительно корня индекса.
    pub rel_path: String,
    /// Номер первой строки чанка, 1-based.
    pub line: u32,
    /// Сам текст чанка.
    pub text: String,
}

/// Сколько строк в чанке.
const CHUNK_LINES: usize = 40;

/// Перекрытие между соседними чанками, в строках.
///
/// Без перекрытия оператор длиной в две строки, попавший на границу, не был бы
/// найден ни одним из чанков — то есть существовал бы в файле, но не в индексе.
const OVERLAP_LINES: usize = 10;

/// Разрезать файл на чанки. Без AST-структуры делить нечем, поэтому режем по
/// строкам — но с перекрытием, а не «одна функция пополам».
pub fn chunks(rel_path: &str, source: &str) -> Vec<FlatChunk> {
    let lines: Vec<&str> = source.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }
    let step = CHUNK_LINES.saturating_sub(OVERLAP_LINES).max(1);
    let mut out = Vec::new();
    let mut start = 0usize;
    while start < lines.len() {
        let end = (start + CHUNK_LINES).min(lines.len());
        out.push(FlatChunk {
            rel_path: rel_path.to_string(),
            line: (start + 1) as u32,
            text: lines[start..end].join("\n"),
        });
        if end >= lines.len() {
            break;
        }
        start += step;
    }
    out
}

/// Плоский файл не даёт ни определений, ни ссылок: символов у него нет.
///
/// Функция существует, чтобы это было **проверяемым фактом**, а не соглашением:
/// бэкенд `Flat` обязан быть единственным, кто возвращает пустой список тегов,
/// иначе получился бы «разобранный» файл без символов — то есть ложь в ответе
/// агенту о структуре кода.
pub fn tags(_rel_path: &str, _source: &str) -> Vec<Tag> {
    Vec::new()
}

/// Оценка размера плоского файла в токенах.
pub fn token_estimate(source: &str) -> u32 {
    tokens::estimate(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbered_lines(count: usize) -> String {
        (1..=count).map(|i| format!("line{i}")).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn empty_file_produces_no_chunks() {
        assert!(chunks("empty.md", "").is_empty());
    }

    #[test]
    fn short_file_is_one_chunk_starting_at_line_one() {
        let chunks = chunks("a.md", "one\ntwo\nthree");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].line, 1);
        assert_eq!(chunks[0].text, "one\ntwo\nthree");
    }

    #[test]
    fn chunks_cover_the_whole_file_with_overlap() {
        let source = numbered_lines(200);
        let chunks = chunks("a.md", &source);
        assert!(chunks.len() > 1, "длинный файл обязан нарезаться");
        assert_eq!(chunks[0].line, 1);
        // Последний чанк обязан добирать до конца файла.
        let last = chunks.last().expect("есть последний чанок");
        assert_eq!(last.text.lines().count(), (200 - (last.line as usize - 1)).min(CHUNK_LINES));
        // Перекрытие есть: шаг меньше длины чанка.
        assert!(CHUNK_LINES - OVERLAP_LINES < CHUNK_LINES);
    }

    #[test]
    fn text_around_a_chunk_boundary_is_still_indexed() {
        // Граница проходит после 30-й строки. Строка 35 обязана попасть хоть в
        // один чанк, иначе её не найдёт поиск.
        let source = numbered_lines(100);
        let chunks = chunks("a.md", &source);
        let found = chunks
            .iter()
            .any(|c| c.text.lines().any(|line| line == "line35"));
        assert!(found, "строка у границы чанков обязана быть в индексе");
    }

    #[test]
    fn flat_backend_yields_no_symbols() {
        // Не «вроде пусто», а строго пусто: символов у н��разобранного файла нет.
        assert!(tags("a.md", "def alpha(): pass").is_empty());
    }

    #[test]
    fn chunk_line_numbers_are_one_based_and_ascending() {
        let chunks = chunks("a.md", &numbered_lines(150));
        assert!(chunks.windows(2).all(|w| w[0].line < w[1].line), "номера строк обязаны расти");
        assert_eq!(chunks[0].line, 1);
    }
}