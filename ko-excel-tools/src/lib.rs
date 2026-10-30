//! # ko-excel-tools — SSOT-ядро Excel-инструментов
//!
//! Общее ядро для работы с .xlsx: операции над книгой ([`ops`]), статические
//! QA-гейты формул ([`qa`]), чтение структуры/диапазонов ([`inspect`]),
//! пересчёт и PDF через LibreOffice ([`soffice`]), конвертация CSV/TSV
//! ([`convert`]).
//!
//! ## Границы
//! - **Без Python и без скриптовых движков**: обработка .xlsx — чистый Rust
//!   (`umya-spreadsheet`, MIT), линкуется в бинарь хоста.
//! - **LibreOffice — внешняя программа, не зависимость**: детект
//!   ([`soffice::detect_soffice`]) + честный отказ. Нет soffice — статические
//!   гейты работают, пересчёт/PDF честно отказывают (решение пользователя,
//!   31.10.26).
//! - **Preserve-first**: исходник только читается; все результаты — новые
//!   файлы в каталоге, который передаёт хост. in-place исключён.
//!
//! ## Хост-контракт
//! Хост (King Orch / CLI agent-tools) передаёт: пути файлов, каталог для
//! результатов, опциональный путь к soffice (config-ключ). Ядро не знает про
//! exe-пути хоста и структуру его конфига — только аргументы.

pub mod convert;
pub mod inspect;
pub mod ops;
pub mod pipeline;
pub mod qa;
pub mod soffice;

/// Версия ядра (для логов и отчётов host-адаптеров).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Форматы, которые ядро редактирует. Всё остальное — честный отказ:
/// `.xlsm` (макросы теряются при перезаписи OOXML umya), `.xls` (старый
/// бинарный формат, не OOXML), `.ods` (другой контейнер).
pub const EDITABLE_EXTENSIONS: [&str; 1] = ["xlsx"];

/// Ошибки ядра. Все сообщения — человекочитаемые, готовые к показу агенту.
#[derive(Debug, thiserror::Error)]
pub enum ExcelError {
    #[error("Файл не найден: {0}")]
    FileNotFound(String),
    #[error("Формат не поддерживается: {0}. {1}")]
    UnsupportedFormat(String, String),
    #[error("Не удалось прочитать .xlsx: {0}")]
    Read(String),
    #[error("Не удалось записать .xlsx: {0}")]
    Write(String),
    #[error("Некорректные аргументы: {0}")]
    Usage(String),
    #[error("Операция '{op}': {msg}")]
    OpFailed { op: String, msg: String },
    #[error(
        "LibreOffice (soffice) не найден. Пересчёт формул и PDF-рендер недоступны: \
         установите LibreOffice или укажите путь в настройках (ключ libreoffice_path). \
         Статическая проверка формул работает без него."
    )]
    SofficeNotDetected,
    #[error("LibreOffice: {0}")]
    Soffice(String),
    #[error("Конвертация: {0}")]
    Convert(String),
}

/// Проверка расширения на редактируемость. Всё не-.xlsx — честный отказ:
/// `.xlsm` (макросы теряются при перезаписи OOXML umya), `.xls` (старый
/// бинарный формат), `.ods` (другой контейнер).
pub fn check_editable(path: &std::path::Path) -> Result<(), ExcelError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if EDITABLE_EXTENSIONS.contains(&ext.as_str()) {
        return Ok(());
    }
    let reason: String = match ext.as_str() {
        "xlsm" => "книга с макросами: перезапись OOXML потеряет макросы".to_string(),
        "xls" => "старый бинарный формат Excel 97-2003: пересохраните как .xlsx".to_string(),
        "ods" => "формат OpenDocument: пересохраните как .xlsx".to_string(),
        "" => "у файла нет расширения".to_string(),
        other => format!("расширение .{other} не поддерживается (работают .xlsx)"),
    };
    Err(ExcelError::UnsupportedFormat(
        path.display().to_string(),
        reason.to_string(),
    ))
}

/// Адрес ячейки ("B3") из её координат.
pub fn cell_address(cell: &umya_spreadsheet::structs::Cell) -> String {
    let coordinate = cell.coordinate();
    format!(
        "{}{}",
        column_letter(coordinate.col_num()),
        coordinate.row_num()
    )
}

/// Открыть книгу для редактирования с проверкой формата (preserve-first:
/// только чтение, никаких изменений исходника).
pub fn open_workbook(path: &std::path::Path) -> Result<umya_spreadsheet::Workbook, ExcelError> {
    if !path.is_file() {
        return Err(ExcelError::FileNotFound(path.display().to_string()));
    }
    check_editable(path)?;
    umya_spreadsheet::reader::xlsx::read(path)
        .map_err(|e| ExcelError::Read(format!("{} — {}", path.display(), e)))
}

/// Записать книгу на диск.
pub fn save_workbook(
    book: &umya_spreadsheet::Workbook,
    path: &std::path::Path,
) -> Result<(), ExcelError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ExcelError::Write(format!("{} — {}", parent.display(), e)))?;
    }
    umya_spreadsheet::writer::xlsx::write(book, path)
        .map_err(|e| ExcelError::Write(format!("{} — {}", path.display(), e)))
}

/// Число (1-based) → буквенный номер столбца Excel ("A", "Z", "AA").
pub fn column_letter(mut col: u32) -> String {
    debug_assert!(col >= 1, "столбцы 1-based");
    let mut out = Vec::new();
    while col > 0 {
        let rem = ((col - 1) % 26) as u8;
        out.push((b'A' + rem) as char);
        col = (col - 1) / 26;
    }
    out.reverse();
    out.into_iter().collect()
}

/// Буквенный номер столбца ("A", "AA") → число (1-based). Ошибка на мусоре.
pub fn column_number(letters: &str) -> Result<u32, ExcelError> {
    let upper = letters.to_ascii_uppercase();
    if upper.is_empty() || !upper.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(ExcelError::Usage(format!(
            "Некорректный столбец: '{letters}'"
        )));
    }
    let mut num: u32 = 0;
    for b in upper.bytes() {
        num = num * 26 + (b - b'A') as u32 + 1;
    }
    Ok(num)
}

/// Парсинг адреса ячейки ("B3") → (col, row), оба 1-based.
pub fn parse_cell_addr(addr: &str) -> Result<(u32, u32), ExcelError> {
    let addr = addr.trim().replace('$', "");
    let split = addr
        .find(|c: char| c.is_ascii_digit())
        .ok_or_else(|| ExcelError::Usage(format!("Некорректный адрес ячейки: '{addr}'")))?;
    let (letters, digits) = addr.split_at(split);
    let col = column_number(letters)?;
    let row: u32 = digits
        .parse()
        .map_err(|_| ExcelError::Usage(format!("Некорректный адрес ячейки: '{addr}'")))?;
    if row == 0 {
        return Err(ExcelError::Usage(format!(
            "Некорректный адрес ячейки: '{addr}'"
        )));
    }
    Ok((col, row))
}

/// Парсинг диапазона "A1:C10" (без имён листов) → ((c1,r1),(c2,r2)).
/// Одна ячейка ("B3") трактуется как диапазон из одной клетки.
pub fn parse_range(range: &str) -> Result<((u32, u32), (u32, u32)), ExcelError> {
    let range = range.trim().replace('$', "");
    match range.split_once(':') {
        Some((a, b)) => Ok((parse_cell_addr(a)?, parse_cell_addr(b)?)),
        None => {
            let p = parse_cell_addr(&range)?;
            Ok((p, p))
        }
    }
}

/// Отделить имя листа от диапазона: "Лист1!A1:B2" → ("Лист1", "A1:B2").
/// Без "!" — (None, как есть). Кавычки имени листа и $-якоря диапазона
/// снимаются.
pub fn split_sheet_range(spec: &str) -> (Option<String>, String) {
    let trimmed = spec.trim();
    if let Some(pos) = trimmed.rfind('!') {
        let mut sheet = trimmed[..pos].trim().to_string();
        let range = trimmed[pos + 1..].trim().replace('$', "");
        if sheet.starts_with('\'') && sheet.ends_with('\'') && sheet.len() >= 2 {
            sheet = sheet[1..sheet.len() - 1].to_string();
        }
        (Some(sheet), range)
    } else {
        (None, trimmed.replace('$', ""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_letter_roundtrip() {
        assert_eq!(column_letter(1), "A");
        assert_eq!(column_letter(26), "Z");
        assert_eq!(column_letter(27), "AA");
        assert_eq!(column_letter(52), "AZ");
        assert_eq!(column_letter(703), "AAA");
        for col in [1u32, 5, 26, 27, 100, 702, 703, 5000] {
            assert_eq!(column_number(&column_letter(col)).unwrap(), col);
        }
    }

    #[test]
    fn parse_cell_addr_cases() {
        assert_eq!(parse_cell_addr("B3").unwrap(), (2, 3));
        assert_eq!(parse_cell_addr("$AA$10").unwrap(), (27, 10));
        assert_eq!(parse_cell_addr("a1").unwrap(), (1, 1));
        assert!(parse_cell_addr("3B").is_err());
        assert!(parse_cell_addr("B").is_err());
        assert!(parse_cell_addr("B0").is_err());
    }

    #[test]
    fn parse_range_cases() {
        assert_eq!(parse_range("A1:C3").unwrap(), ((1, 1), (3, 3)));
        assert_eq!(parse_range("B2").unwrap(), ((2, 2), (2, 2)));
        assert_eq!(parse_range("$B$2:$D$6").unwrap(), ((2, 2), (4, 6)));
    }

    #[test]
    fn split_sheet_range_cases() {
        let (sheet, range) = split_sheet_range("Лист1!A1:B2");
        assert_eq!(sheet.as_deref(), Some("Лист1"));
        assert_eq!(range, "A1:B2");
        let (sheet, range) = split_sheet_range("'My Sheet'!$A$1");
        assert_eq!(sheet.as_deref(), Some("My Sheet"));
        assert_eq!(range, "A1");
        let (sheet, range) = split_sheet_range("A1:B2");
        assert_eq!(sheet, None);
        assert_eq!(range, "A1:B2");
    }

    #[test]
    fn check_editable_rejects_non_xlsx() {
        assert!(check_editable(std::path::Path::new("a.xlsx")).is_ok());
        assert!(check_editable(std::path::Path::new("A.XLSX")).is_ok());
        let err = check_editable(std::path::Path::new("a.xls")).unwrap_err();
        assert!(err.to_string().contains("97-2003"), "{err}");
        let err = check_editable(std::path::Path::new("a.xlsm")).unwrap_err();
        assert!(err.to_string().contains("макросы"), "{err}");
    }
}
