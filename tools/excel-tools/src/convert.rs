//! Конвертация CSV/TSV ↔ XLSX. Кодировки: UTF-8 (с BOM и без) — основной
//! путь, Windows-1251 — fallback для старых русских выгрузок (encoding_rs).
//! Числа распознаются при записи в xlsx (JSON-число → числовая ячейка);
//! строки с ведущим нулём ("00123", телефоны) сохраняются текстом.

use crate::{check_editable, ExcelError};
use serde_json::{json, Value};
use std::io::Read;
use std::path::Path;

/// Сводка конвертации.
#[derive(Debug)]
pub struct ConversionSummary {
    pub rows: usize,
    pub cols: usize,
    pub output_path: String,
}

impl ConversionSummary {
    pub fn to_json(&self) -> Value {
        json!({
            "output_path": self.output_path,
            "rows": self.rows,
            "cols": self.cols,
        })
    }
}

/// Прочитать текст CSV: UTF-8 → UTF-8 BOM → Windows-1251 (fallback).
fn read_text(path: &Path) -> Result<String, ExcelError> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|mut f| f.read_to_end(&mut bytes))
        .map_err(|e| ExcelError::Convert(format!("{}: {e}", path.display())))?;
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8(bytes[3..].to_vec())
            .map_err(|_| ExcelError::Convert("файл с BOM, но не валидный UTF-8".to_string()));
    }
    match String::from_utf8(bytes.clone()) {
        Ok(text) => Ok(text),
        Err(_) => {
            let (decoded, _, had_errors) = encoding_rs::WINDOWS_1251.decode(&bytes);
            if had_errors {
                return Err(ExcelError::Convert(
                    "кодировка файла не UTF-8 и не Windows-1251 (текст повреждён)".to_string(),
                ));
            }
            log::info!("CSV прочитан как Windows-1251: {}", path.display());
            Ok(decoded.into_owned())
        }
    }
}

/// Определить разделитель по первой строке: максимум вхождений из , ; \t |
fn detect_delimiter(first_line: &str) -> char {
    let mut best = ',';
    let mut best_count = 0usize;
    for candidate in [',', ';', '\t', '|'] {
        let count = first_line.matches(candidate).count();
        if count > best_count {
            best = candidate;
            best_count = count;
        }
    }
    best
}

fn delimiter_byte(d: char) -> u8 {
    d as u8
}

/// CSV/TSV → XLSX. Числовые поля становятся числами (разделитель дробной
/// части: "." всегда; "," — если это НЕ разделитель полей). Ведущие нули
/// сохраняют текстовый тип.
pub fn csv_to_xlsx(
    csv_path: &Path,
    out_path: &Path,
    sheet_name: Option<&str>,
    delimiter: Option<char>,
) -> Result<ConversionSummary, ExcelError> {
    if !csv_path.is_file() {
        return Err(ExcelError::FileNotFound(csv_path.display().to_string()));
    }
    let text = read_text(csv_path)?;
    let first_line = text.lines().next().unwrap_or_default().to_string();
    let delimiter = delimiter.unwrap_or_else(|| detect_delimiter(&first_line));

    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter_byte(delimiter))
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes());
    let mut rows: Vec<Vec<String>> = Vec::new();
    for record in reader.records() {
        let record = record
            .map_err(|e| ExcelError::Convert(format!("разбор CSV: {e}")))?;
        rows.push(record.into_iter().map(str::to_string).collect());
    }
    if rows.is_empty() {
        return Err(ExcelError::Convert(
            "файл пуст или не разобран как CSV/TSV".to_string(),
        ));
    }
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);

    let mut book = umya_spreadsheet::new_file();
    let sheet = book
        .sheet_collection_mut()
        .first_mut()
        .expect("new_file содержит лист");
    sheet.set_name(
        sheet_name
            .map(str::to_string)
            .unwrap_or_else(|| "Данные".to_string()),
    );
    let delimiter_is_comma = delimiter == ',';
    for (row_index, row) in rows.iter().enumerate() {
        for (col_index, raw) in row.iter().enumerate() {
            let cell = sheet.cell_mut((col_index as u32 + 1, row_index as u32 + 1));
            match numeric_value(raw, delimiter_is_comma) {
                Some(number) => cell.set_value_number(number),
                None => cell.set_value_string(raw.clone()),
            };
        }
    }
    crate::save_workbook(&book, out_path)?;
    Ok(ConversionSummary {
        rows: rows.len(),
        cols,
        output_path: out_path.display().to_string(),
    })
}

/// Распознавание числа: Some(f64) — числовая ячейка, None — текст.
/// "00123" и "+7 900..." — текст; "1,5" — число только когда запятая не
/// служит разделителем полей.
fn numeric_value(raw: &str, comma_is_text: bool) -> Option<f64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    // "+"-префикс — телефоны и подобное: намерение текста (Excel-числа
    // в CSV так не пишут), ведущий ноль — артикулы: тоже текст.
    if trimmed.starts_with('+') {
        return None;
    }
    let digits = trimmed.trim_start_matches('-');
    if digits.len() > 1 && digits.starts_with('0') && !digits.contains(['.', ',']) {
        return None;
    }
    let normalized: &str = if comma_is_text {
        trimmed
    } else {
        &trimmed.replace(',', ".")
    };
    if normalized.parse::<f64>().is_ok() {
        return normalized.parse::<f64>().ok();
    }
    None
}

/// XLSX → CSV/TSV. Значения — кэш; формульная ячейка без кэша (файл записан
/// программно и не пересчитывался) отдаётся текстом "=ФОРМУЛА" — Excel
/// вычислит её при открытии CSV.
pub fn xlsx_to_csv(
    xlsx_path: &Path,
    out_path: &Path,
    sheet_name: Option<&str>,
    delimiter: char,
) -> Result<ConversionSummary, ExcelError> {
    check_editable(xlsx_path)?;
    let book = umya_spreadsheet::reader::xlsx::read(xlsx_path)
        .map_err(|e| ExcelError::Read(format!("{} — {}", xlsx_path.display(), e)))?;
    let sheet = match sheet_name {
        Some(name) => book
            .sheet_by_name(name)
            .map_err(|_| ExcelError::Usage(format!("Лист '{name}' не найден")))?,
        None => book
            .sheet_collection()
            .first()
            .ok_or_else(|| ExcelError::Usage("Книга без листов".to_string()))?,
    };
    let (max_col, max_row) = sheet.highest_column_and_row();
    let mut writer = csv::WriterBuilder::new()
        .delimiter(delimiter_byte(delimiter))
        .from_path(out_path)
        .map_err(|e| ExcelError::Convert(format!("{}: {e}", out_path.display())))?;
    for row in 1..=max_row {
        let mut record = Vec::with_capacity(max_col as usize);
        for col in 1..=max_col {
            match sheet.cell((col, row)) {
                Some(cell) => {
                    let value = cell.value().to_string();
                    if value.is_empty() {
                        let formula = cell.formula();
                        if formula.is_empty() {
                            record.push(String::new());
                        } else {
                            record.push(format!("={formula}"));
                        }
                    } else {
                        record.push(value);
                    }
                }
                None => record.push(String::new()),
            }
        }
        writer
            .write_record(&record)
            .map_err(|e| ExcelError::Convert(format!("запись CSV: {e}")))?;
    }
    writer
        .flush()
        .map_err(|e| ExcelError::Convert(format!("запись CSV: {e}")))?;
    Ok(ConversionSummary {
        rows: max_row as usize,
        cols: max_col as usize,
        output_path: out_path.display().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        // Временинки — внутри target/ крейта (не %TEMP%).
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join(tag);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn csv_to_xlsx_types_and_encoding() {
        let dir = tmp_dir("csv_types");
        let csv_path = dir.join("data.csv");
        std::fs::write(
            &csv_path,
            "Товар;Цена;Артикул\nХлеб;45,5;00123\nМолоко;89,9;+79001234567\n",
        )
        .unwrap();
        let out = dir.join("data.xlsx");
        let summary = csv_to_xlsx(&csv_path, &out, None, None).unwrap();
        assert_eq!(summary.rows, 3);
        assert_eq!(summary.cols, 3);

        let book = umya_spreadsheet::reader::xlsx::read(&out).unwrap();
        let sheet = book.sheet_by_name("Данные").unwrap();
        // "45,5" при разделителе ';' — число с запятой как десятичный.
        assert_eq!(sheet.cell("B2").unwrap().data_type(), "n");
        assert_eq!(sheet.cell("B2").unwrap().value(), "45.5");
        // Артикулы с ведущим нулём и телефоном — текст.
        assert_eq!(sheet.cell("C2").unwrap().data_type(), "s");
        assert_eq!(sheet.cell("C2").unwrap().value(), "00123");
        assert_eq!(sheet.cell("C3").unwrap().value(), "+79001234567");
    }

    #[test]
    fn csv_bom_and_tsv_detected() {
        let dir = tmp_dir("csv_bom");
        let tsv_path = dir.join("data.tsv");
        // UTF-8 BOM + табы: BOM снимается, разделитель определяется по \t.
        std::fs::write(
            &tsv_path,
            [[0xEF, 0xBB, 0xBF].as_slice(), b"a\tb\n1\t2\n"].concat(),
        )
        .unwrap();
        let out = dir.join("data.xlsx");
        let summary = csv_to_xlsx(&tsv_path, &out, Some("Лист"), None).unwrap();
        assert_eq!(summary.cols, 2);
        let book = umya_spreadsheet::reader::xlsx::read(&out).unwrap();
        assert!(book.sheet_by_name("Лист").is_ok());
    }

    #[test]
    fn xlsx_to_csv_roundtrip_with_formula() {
        let dir = tmp_dir("roundtrip");
        let xlsx = dir.join("book.xlsx");
        let mut book = umya_spreadsheet::new_file();
        {
            let sheet = book.sheet_by_name_mut("Sheet1").unwrap();
            sheet.cell_mut("A1").set_value_string("имя");
            sheet.cell_mut("B1").set_value_string("сумма");
            sheet.cell_mut("A2").set_value_string("запись");
            sheet.cell_mut("B2").set_value_number(12.5);
            sheet.cell_mut("B3").set_formula("B2*2");
        }
        umya_spreadsheet::writer::xlsx::write(&book, &xlsx).unwrap();

        let csv_out = dir.join("book.csv");
        let summary = xlsx_to_csv(&xlsx, &csv_out, None, ',').unwrap();
        assert_eq!(summary.rows, 3);
        let text = std::fs::read_to_string(&csv_out).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "имя,сумма");
        assert_eq!(lines[1], "запись,12.5");
        // A3 пустая (пустое поле), формула без кэша отдаётся как "=формула" —
        // Excel вычислит её при открытии CSV.
        assert_eq!(lines[2], ",=B2*2");
    }

    #[test]
    fn windows1251_fallback() {
        let dir = tmp_dir("cp1251");
        let (encoded, _, _) = encoding_rs::WINDOWS_1251.encode("Имя;Значение\nТест;5\n");
        let csv_path = dir.join("old.csv");
        std::fs::write(&csv_path, encoded.as_ref()).unwrap();
        let out = dir.join("old.xlsx");
        csv_to_xlsx(&csv_path, &out, None, None).unwrap();
        let book = umya_spreadsheet::reader::xlsx::read(&out).unwrap();
        let sheet = book.sheet_by_name("Данные").unwrap();
        assert_eq!(sheet.cell("A2").unwrap().value(), "Тест");
    }

    #[test]
    fn numeric_recognition_edges() {
        assert_eq!(numeric_value("45,5", false), Some(45.5));
        assert_eq!(numeric_value("45,5", true), None);
        assert_eq!(numeric_value("00123", false), None);
        assert_eq!(numeric_value("0", false), Some(0.0));
        assert_eq!(numeric_value("-7", false), Some(-7.0));
        assert_eq!(numeric_value("+79001234567", false), None);
        assert_eq!(numeric_value("abc", false), None);
        assert_eq!(numeric_value("", false), None);
    }
}
