//! Чтение: структура книги и значения/формулы диапазонов. Исходник
//! не модифицируется (preserve-first).

use crate::{cell_address, check_editable, parse_range, split_sheet_range, ExcelError};
use serde_json::{json, Value};
use umya_spreadsheet::structs::Cell;
use umya_spreadsheet::Workbook;

/// Структура книги: листы, размеры, объединения, диаграммы, автофильтры.
pub fn structure(path: &std::path::Path) -> Result<Value, ExcelError> {
    check_editable(path)?;
    let book = umya_spreadsheet::reader::xlsx::read(path)
        .map_err(|e| ExcelError::Read(format!("{} — {}", path.display(), e)))?;
    let sheets: Vec<Value> = book
        .sheet_collection()
        .iter()
        .map(sheet_summary)
        .collect();
    let total_cells: u64 = sheets
        .iter()
        .filter_map(|s| s.get("used_cells").and_then(|v| v.as_u64()))
        .sum();
    Ok(json!({
        "file": path.display().to_string(),
        "sheet_count": sheets.len(),
        "active_sheet": book.sheet_collection().first().map(|s| s.name()),
        "total_used_cells": total_cells,
        "sheets": sheets,
    }))
}

fn sheet_summary(sheet: &umya_spreadsheet::structs::Worksheet) -> Value {
    let (max_col, max_row) = sheet.highest_column_and_row();
    let merged: Vec<String> = sheet
        .merge_cells()
        .iter()
        .map(|r| r.range())
        .collect();
    let charts = sheet.chart_collection().len();
    let auto_filter = sheet
        .auto_filter()
        .map(|f| f.range().range())
        .unwrap_or_default();
    let dimensions = if max_col == 0 || max_row == 0 {
        "пусто".to_string()
    } else {
        format!("A1:{}{}", crate::column_letter(max_col), max_row)
    };
    json!({
        "name": sheet.name(),
        "dimensions": dimensions,
        "used_cells": (max_col as u64) * (max_row as u64),
        "merged_ranges": merged,
        "chart_count": charts,
        "auto_filter": auto_filter,
    })
}

/// Значения и формулы диапазона. Диапазон ограничен 2000 ячеек: больше —
/// честный отказ с просьбой уточнить (защита контекста модели от заливки).
pub const MAX_READ_CELLS: u64 = 2000;

/// Прочитать диапазон (или весь использованный прямоугольник, если range не
/// задан). `sheet_spec` — имя листа; None — первый лист. Ячейки с формулами
/// возвращают и `f` (формулу), и `v` (кэш значения, может отсутствовать у
/// свежезаписанных формул до пересчёта).
pub fn read_range(
    path: &std::path::Path,
    sheet_spec: Option<&str>,
    range_spec: Option<&str>,
) -> Result<Value, ExcelError> {
    check_editable(path)?;
    let book = umya_spreadsheet::reader::xlsx::read(path)
        .map_err(|e| ExcelError::Read(format!("{} — {}", path.display(), e)))?;
    read_range_of_book(&book, sheet_spec, range_spec)
}

/// Вариант для уже открытой книги (используется пайплайном apply/audit).
pub fn read_range_of_book(
    book: &Workbook,
    sheet_spec: Option<&str>,
    range_spec: Option<&str>,
) -> Result<Value, ExcelError> {
    let sheet = match sheet_spec {
        Some(name) => book
            .sheet_by_name(name)
            .map_err(|_| ExcelError::Usage(format!("Лист '{name}' не найден")))?,
        None => book
            .sheet_collection()
            .first()
            .ok_or_else(|| ExcelError::Usage("Книга без листов".to_string()))?,
    };
    let sheet_name = sheet.name();
    let ((c1, r1), (c2, r2)) = match range_spec {
        Some(spec) => {
            // Формат "Лист!A1:B2" допустим — имя листа сверяется с выбранным.
            let (embedded_sheet, range) = split_sheet_range(spec);
            if let Some(embedded) = embedded_sheet {
                if embedded != sheet_name {
                    return Err(ExcelError::Usage(format!(
                        "Диапазон с листом '{embedded}' не совпадает с выбранным листом '{sheet_name}'"
                    )));
                }
            }
            parse_range(&range)?
        }
        None => {
            let (max_col, max_row) = sheet.highest_column_and_row();
            ((1, 1), (max_col, max_row))
        }
    };
    if (c2 < c1) || (r2 < r1) {
        return Err(ExcelError::Usage(
            "Диапазон задан от большего к меньшему: используйте формат A1:C3 (левый верхний : правый нижний)".to_string(),
        ));
    }
    let cells_count = (c2 - c1 + 1) as u64 * (r2 - r1 + 1) as u64;
    if cells_count > MAX_READ_CELLS {
        let width = c2 - c1 + 1;
        let height = r2 - r1 + 1;
        return Err(ExcelError::Usage(format!(
            "Диапазон {width}×{height} = {cells_count} ячеек — больше лимита {MAX_READ_CELLS}. \
             Укажите уже диапазон или читайте по частям."
        )));
    }
    let mut cells = Vec::new();
    for row in r1..=r2 {
        for col in c1..=c2 {
            let Some(cell) = sheet.cell((col, row)) else {
                continue;
            };
            let record = cell_record(cell);
            if !record.is_null() {
                cells.push(record);
            }
        }
    }
    Ok(json!({
        "sheet": sheet_name,
        "range": format!("{}{}:{}{}", crate::column_letter(c1), r1, crate::column_letter(c2), r2),
        "cell_count": cells.len(),
        "cells": cells,
    }))
}

/// Запись ячейки для JSON-ответа: адрес, значение, формула, тип.
/// Пустые (нет значения и формулы) отсекаются вызывающим.
fn cell_record(cell: &Cell) -> Value {
    let value = cell.value().to_string();
    let formula = cell.formula().to_string();
    let data_type = cell.data_type().to_string();
    if value.is_empty() && formula.is_empty() {
        return Value::Null;
    }
    let mut record = json!({
        "addr": cell_address(cell),
    });
    let obj = record.as_object_mut().expect("объект создан выше");
    if !value.is_empty() {
        obj.insert("v".to_string(), Value::String(value));
    }
    if !formula.is_empty() {
        obj.insert("f".to_string(), Value::String(formula));
        obj.insert("t".to_string(), Value::String(data_type));
    } else {
        obj.insert("t".to_string(), Value::String(data_type));
    }
    record
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structure_reports_sheets_and_charts() {
        let mut book = umya_spreadsheet::new_file();
        let sheet = book.sheet_by_name_mut("Sheet1").unwrap();
        sheet.cell_mut("B2").set_value_string("x");
        sheet.cell_mut("C3").set_formula("B2&\"!\"");
        sheet.add_merge_cells("A1:A5");
        book.new_sheet("Второй").unwrap();

        assert_eq!(structure_of(&book)["sheet_count"], 2);
        let first = structure_of(&book)["sheets"].as_array().unwrap()[0].clone();
        assert_eq!(first["dimensions"], "A1:C3");
        assert_eq!(first["used_cells"], 9);
        assert_eq!(first["merged_ranges"].as_array().unwrap().len(), 1);
    }

    fn structure_of(book: &Workbook) -> Value {
        // Внутренний проход той же логики (без файла).
        let sheets: Vec<Value> = book
            .sheet_collection()
            .iter()
            .map(sheet_summary)
            .collect();
        json!({
            "sheet_count": sheets.len(),
            "sheets": sheets,
        })
    }

    #[test]
    fn read_range_skips_empty_cells_and_counts() {
        let mut book = umya_spreadsheet::new_file();
        let sheet = book.sheet_by_name_mut("Sheet1").unwrap();
        sheet.cell_mut("A1").set_value_string("заголовок");
        sheet.cell_mut("B1").set_value_string("число");
        sheet.cell_mut("A3").set_value_number(42.0);
        sheet.cell_mut("B3").set_formula("A3*2");

        let result = read_range_of_book(&book, None, Some("A1:B3")).unwrap();
        assert_eq!(result["range"], "A1:B3");
        let cells = result["cells"].as_array().unwrap();
        assert_eq!(cells.len(), 4, "пустая строка 2 пропущена: {cells:?}");
        let formula_cell = cells
            .iter()
            .find(|c| c["addr"] == "B3")
            .expect("формульная ячейка в ответе");
        assert_eq!(formula_cell["f"], "A3*2");
    }

    #[test]
    fn read_range_limit_is_enforced() {
        let book = umya_spreadsheet::new_file();
        let err = read_range_of_book(&book, None, Some("A1:Z100")).unwrap_err();
        assert!(err.to_string().contains("лимит"), "{err}");
    }

    #[test]
    fn read_range_rejects_reverse_order() {
        let book = umya_spreadsheet::new_file();
        let err = read_range_of_book(&book, None, Some("C3:A1")).unwrap_err();
        assert!(err.to_string().contains("от большего"), "{err}");
    }
}
