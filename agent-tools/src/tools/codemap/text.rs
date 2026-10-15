//! Перевод байтовых смещений в номера строк.
//!
//! Нужен бэкендам, которые отдают позиции байтами: `oxc` возвращает `Span` с
//! байтовыми смещениями, `rustpython-parser` — `TextRange`. `syn` отдаёт сразу
//! номера строк, и ему этот индекс не требуется.
//!
//! Строится один раз на файл и переиспользуется для всех его символов:
//! построчный подсчёт на каждый символ дал бы O(n²) на крупных файлах.

use std::path::Path;

/// Сколько байт в начале файла проверяется на признак бинарности.
///
/// 8 КиБ — достаточно, чтобы поймать любой обычный формат (PNG, ICO, WOFF2,
/// ZIP-подобные), и достаточно мало, чтобы не читать целиком многогигабайтные
/// файлы вроде моделей или видео.
const SNIFF_BYTES: usize = 8 * 1024;

/// Что лежит в файле.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    /// Текст, пригодный для парсинга и поиска.
    Text(String),
    /// Двоичный файл: в индекс не попадает и ошибкой не считается.
    Binary,
    /// Файл не удалось прочитать с диска — вот это уже настоящая ошибка.
    Unreadable(String),
}

/// Прочитать файл и решить, текст это или нет.
///
/// Решение принимается **до** выбора парсера, а не внутри него. Иначе отказ
/// утекал наружу: `read_to_string` на `.png` возвращал ошибку, и весь обход
/// репозитория падал на первом же файле иконки — универсальный инструмент
/// обязан принимать любую папку, а подмножество её содержимого не имеет права
/// отменять весь запуск.
///
/// Признак бинарности — нулевой байт в начале файла либо невалидный UTF-8
/// в просмотренном префиксе. Ни то, ни другое в исходниках не встречается:
/// нулевого байта в тексте не бывает по определению.
pub fn read_content(path: &Path) -> Content {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => return Content::Unreadable(error.to_string()),
    };
    if bytes.is_empty() {
        return Content::Text(String::new());
    }
    let head = &bytes[..bytes.len().min(SNIFF_BYTES)];
    if head.contains(&0) {
        return Content::Binary;
    }
    match String::from_utf8(bytes) {
        Ok(text) => Content::Text(text),
        Err(_) => Content::Binary,
    }
}

/// Отсортированные начала строк исходника.
///
/// Последний элемент — длина исходника: он позволяет отвечать и для смещения
/// ровно в конце файла, не заводя отдельной ветки «а что если за границей».
#[derive(Debug, Clone)]
pub struct LineIndex {
    starts: Vec<u32>,
}

impl LineIndex {
    /// Построить индекс по исходному тексту.
    pub fn new(source: &str) -> LineIndex {
        let length = source.len() as u32;
        let mut starts = Vec::with_capacity(source.len() / 24 + 2);
        starts.push(0u32);
        for (offset, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                starts.push((offset + 1) as u32);
            }
        }
        // Хвостовой элемент держит смещение в конце файла, чтобы `line_of` не
        // требовал отдельной ветки «а что если за границей».
        //
        // Условие строгое: если файл кончается переводом строки, последний
        // перенос уже дал смещение `length + 1`, и добавление `length` снова
        // нарушило бы упорядоченность, на которой держится бинарный поиск.
        if starts.last().is_none_or(|&start| start < length) {
            starts.push(length);
        }
        LineIndex { starts }
    }

    /// Номер строки (1-based) для байтового смещения.
    ///
    /// Смещение за пределами исходника не считается ошибкой и не «съезжает» на
    /// первую строку: возвращается последняя строка файла, потому что это
    /// правдоподобнее всего, и отсечение делает вызывающий код.
    pub fn line_of(&self, offset: u32) -> u32 {
        // Сколько начал строк не правее смещения — столько строк пройдено.
        // Сравнение нестрогое: смещение, равное началу строки, принадлежит
        // именно ей, а не предыдущей.
        let passed = self.starts.partition_point(|&start| start <= offset) as u32;
        passed.clamp(1, self.line_count())
    }

    /// Всего строк в файле (1-based максимум).
    pub fn line_count(&self) -> u32 {
        self.starts.len().saturating_sub(1).max(1) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_offsets_to_one_based_lines() {
        let index = LineIndex::new("alpha\nbeta\ngamma\n");
        assert_eq!(index.line_of(0), 1, "начало файла — первая строка");
        assert_eq!(index.line_of(4), 1, "перед переводом строки всё ещё первая");
        assert_eq!(index.line_of(6), 2, "beta начинается со второй строки");
        assert_eq!(index.line_of(12), 3, "gamma начинается с третьей строки");
    }

    #[test]
    fn handles_file_without_trailing_newline() {
        let index = LineIndex::new("alpha\nbeta");
        assert_eq!(index.line_of(6), 2);
        assert_eq!(index.line_of(9), 2, "конец файла без перевода строки");
    }

    #[test]
    fn handles_empty_and_single_line() {
        assert_eq!(LineIndex::new("").line_of(0), 1);
        assert_eq!(LineIndex::new("only").line_of(4), 1);
    }

    #[test]
    fn offset_past_the_end_does_not_wrap_to_first_line() {
        // Возврат 1-й строки для заведомо неверного смещения означал бы, что
        // символ «находится» в начале файла. Это ложь, поэтому ждём последнюю.
        let index = LineIndex::new("a\nb\nc\n");
        assert_eq!(index.line_of(9999), 3);
    }

    #[test]
    fn multiline_syntax_grows_index_per_line() {
        let source = (0..100).map(|i| format!("line{i}\n")).collect::<String>();
        let index = LineIndex::new(&source);
        assert_eq!(index.line_count(), 100);
        let offset = source.find("line42").expect("маркер есть") as u32;
        assert_eq!(index.line_of(offset), 43);
    }

    // --- Определение бинарности -------------------------------------------------

    fn write_temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, bytes).expect("запись");
        path
    }

    #[test]
    fn reads_plain_text() {
        let path = write_temp("codemap_text_ok.rs", b"fn main() {}\n");
        assert!(matches!(read_content(&path), Content::Text(_)));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn empty_file_is_text_not_binary() {
        // Пустой файл — не «нечитаемый» и не «двоичный»: иначе он молча
        // исчезал бы из индекса вместе с остальными пустыми файлами.
        let path = write_temp("codemap_text_empty.rs", b"");
        assert_eq!(read_content(&path), Content::Text(String::new()));
        let _ = std::fs::remove_file(path);
    }

    /// Регрессия против реального прогона на king_orch_3: иконка в формате PNG
    /// роняла весь обход репозитория.
    #[test]
    fn png_signature_is_binary() {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(&[0x00, 0x01, 0x02, 0x03]);
        let path = write_temp("codemap_binary.png", &png);
        assert_eq!(read_content(&path), Content::Binary);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn nul_byte_anywhere_in_prefix_is_binary() {
        let path = write_temp("codemap_nul.woff2", b"wOF2\x00\x00\x00\x00binary");
        assert_eq!(read_content(&path), Content::Binary);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn invalid_utf8_without_nul_is_binary() {
        // Неполная многобайтовая последовательность: нулевого байта нет, но
        // текстом это быть не может.
        let path = write_temp("codemap_bad_utf8.txt", &[0xC3, 0x28, b'a']);
        assert_eq!(read_content(&path), Content::Binary);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn valid_multibyte_text_is_not_binary() {
        let path = write_temp("codemap_cyrillic.md", "Привет, мир\n".as_bytes());
        assert!(matches!(read_content(&path), Content::Text(_)));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn missing_file_is_unreadable_not_binary() {
        // Подмена «нет файла» на «файл двоичный» стёрла бы различие между
        // «нечего индексировать» и «диск не отдал содержимое».
        assert!(matches!(
            read_content(Path::new("definitely/missing/file.bin")),
            Content::Unreadable(_)
        ));
    }
}