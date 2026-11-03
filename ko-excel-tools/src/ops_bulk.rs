//! Массовые операции блочной записи: `set_rows` (блок строк за один op),
//! `format_range` (формат/стиль на диапазон), `add_table` (умная таблица Excel).
//!
//! Зачем отдельный модуль: единственная запись данных через `set_cell` по одной
//! ячейке делает конверт `excel_apply` для реальных таблиц (десятки строк) тысячами
//! токенов — он перестаёт помещаться в бюджет генерации LLM (кейс 03.11.2026:
//! «не смог завершить ответ после 3 докачек»). Блок-операции сжимают JSON на
//! порядок; цикл по ячейкам исполняется в Rust и для модели бесплатен.

use crate::ops::{apply_style, sheet_mut, CellStyle};
use crate::{column_letter, parse_cell_addr, parse_range, ExcelError};
use umya_spreadsheet::structs::{
    ShowColumn, ShowStripes, Table, TableColumn, TableStyleInfo,
};

/// Жёсткий потолок на объём одной массовой операции (в ячейках). Механическая
/// защита от «A1:XFD1048576» — многовекового зависания на пустом месте.
const MAX_BULK_CELLS: usize = 100_000;

/// Записать содержимое одной ячейки: значение (JSON-тип сохраняется), формулу,
/// формат числа, стиль. Общая рука для `set_cell` и ячеек блока `set_rows`.
pub(crate) fn write_cell_content(
    cell: &mut umya_spreadsheet::structs::Cell,
    addr_label: &str,
    value: Option<serde_json::Value>,
    formula: Option<String>,
    number_format: Option<String>,
    style: Option<CellStyle>,
) -> Result<(), ExcelError> {
    if let Some(v) = value {
        match v {
            serde_json::Value::Number(n) => {
                let num = n
                    .as_f64()
                    .ok_or_else(|| ExcelError::Usage(format!("Число вне диапазона f64: {n}")))?;
                cell.set_value_number(num);
            }
            serde_json::Value::Bool(b) => {
                cell.set_value_bool(b);
            }
            serde_json::Value::String(s) => {
                // Строка — ровно текст: никаких автопревращений "00123"
                // в число (интыент агента обязателен).
                cell.set_value_string(s);
            }
            other => {
                return Err(ExcelError::Usage(format!(
                    "set_cell {addr_label}: значение должно быть строкой, числом или bool, получено: {other}"
                )));
            }
        }
    }
    if let Some(f) = formula {
        if f.starts_with('=') {
            return Err(ExcelError::Usage(format!(
                "set_cell {addr_label}: формула передаётся БЕЗ ведущего '=' (получено '{f}')"
            )));
        }
        cell.set_formula(f);
    }
    if let Some(code) = number_format {
        cell.style_mut().numbering_format_mut().set_format_code(code);
    }
    if let Some(style_spec) = style {
        apply_style(cell.style_mut(), &style_spec)?;
    }
    Ok(())
}

/// Ячейка блока `set_rows`: голое значение (строка/число/bool/null) или объект
/// {value?, formula?, number_format?, style?}. `null` — ячейка не пишется
/// (явный пропуск в сетке).
fn write_block_cell(
    sheet_ref: &mut umya_spreadsheet::Worksheet,
    col: u32,
    row: u32,
    payload: &serde_json::Value,
) -> Result<(), ExcelError> {
    let addr_label = format!("{}{}", column_letter(col), row);
    let (value, formula, number_format, style) = match payload {
        serde_json::Value::Null => return Ok(()),
        plain @ (serde_json::Value::String(_) | serde_json::Value::Number(_) | serde_json::Value::Bool(_)) => {
            (Some(plain.clone()), None, None, None)
        }
        serde_json::Value::Object(map) => {
            if map.is_empty() {
                return Err(ExcelError::Usage(format!(
                    "set_rows {addr_label}: объект-ячейка пуст — нужны value и/или formula"
                )));
            }
            let known = ["value", "formula", "number_format", "style"];
            if let Some(unknown) = map.keys().find(|k| !known.contains(&k.as_str())) {
                return Err(ExcelError::Usage(format!(
                    "set_rows {addr_label}: неизвестное поле '{unknown}' (известные: value, formula, number_format, style)"
                )));
            }
            let style = match map.get("style") {
                Some(s) => {
                    Some(serde_json::from_value::<CellStyle>(s.clone()).map_err(|e| {
                        ExcelError::Usage(format!(
                            "set_rows {addr_label}: style не соответствует схеме: {e}"
                        ))
                    })?)
                }
                None => None,
            };
            (
                map.get("value").cloned().filter(|v| !v.is_null()),
                map.get("formula")
                    .map(|v| v.as_str().unwrap_or_default().to_string()),
                map.get("number_format")
                    .map(|v| v.as_str().unwrap_or_default().to_string()),
                style,
            )
        }
        other => {
            return Err(ExcelError::Usage(format!(
                "set_rows {addr_label}: ячейка должна быть значением или объектом, получено: {other}"
            )));
        }
    };
    if let Some(f) = &formula {
        if f.is_empty() {
            return Err(ExcelError::Usage(format!(
                "set_rows {addr_label}: поле formula пустое"
            )));
        }
    }
    let cell = sheet_ref.cell_mut((col, row));
    write_cell_content(cell, &addr_label, value, formula, number_format, style)
}

/// `set_rows`: блок строк одной операцией. `addr` — левый верхний угол;
/// строки могут быть рваными (короче — хвост не пишется). Повторяет семантику
/// set_cell для каждой ячейки (типы сохраняются, формула без ведущего '=').
pub(crate) fn apply_set_rows(
    book: &mut umya_spreadsheet::Workbook,
    sheet: &str,
    addr: &str,
    rows: &[Vec<serde_json::Value>],
) -> Result<(), ExcelError> {
    if rows.is_empty() {
        return Err(ExcelError::Usage(
            "set_rows: rows пуст — нечего записывать".to_string(),
        ));
    }
    let total: usize = rows.iter().map(Vec::len).sum();
    if total > MAX_BULK_CELLS {
        return Err(ExcelError::Usage(format!(
            "set_rows: {total} ячеек превышает потолок {MAX_BULK_CELLS} — разбей на несколько apply"
        )));
    }
    let (col0, row0) = parse_cell_addr(addr)?;
    let sheet_ref = sheet_mut(book, sheet)?;
    for (i, row) in rows.iter().enumerate() {
        for (j, payload) in row.iter().enumerate() {
            let col = col0 + j as u32;
            let row_num = row0 + i as u32;
            write_block_cell(sheet_ref, col, row_num, payload)?;
        }
    }
    Ok(())
}

/// `format_range`: формат числа и/или стиль на весь диапазон (объём работы
/// цикла по ячейкам в Rust — для модели один короткий op вместо десятков).
pub(crate) fn apply_format_range(
    book: &mut umya_spreadsheet::Workbook,
    sheet: &str,
    range: &str,
    number_format: Option<&str>,
    style: Option<CellStyle>,
) -> Result<(), ExcelError> {
    if number_format.is_none() && style.is_none() {
        return Err(ExcelError::Usage(
            "format_range: нечего применять — нужен number_format и/или style".to_string(),
        ));
    }
    let ((c1, r1), (c2, r2)) = parse_range(range)?;
    let cells = (c2 - c1 + 1) as usize * (r2 - r1 + 1) as usize;
    if cells > MAX_BULK_CELLS {
        return Err(ExcelError::Usage(format!(
            "format_range: диапазон '{range}' ({cells} ячеек) превышает потолок {MAX_BULK_CELLS} — сузь диапазон"
        )));
    }
    let sheet_ref = sheet_mut(book, sheet)?;
    for row in r1..=r2 {
        for col in c1..=c2 {
            let cell = sheet_ref.cell_mut((col, row));
            if let Some(code) = number_format {
                cell.style_mut().numbering_format_mut().set_format_code(code);
            }
            if let Some(style_spec) = &style {
                apply_style(cell.style_mut(), style_spec)?;
            }
        }
    }
    Ok(())
}

/// Отображаемые имена таблиц книги (уникальность displayName в Excel —
/// требование формата, а не пожелание).
fn existing_table_names(book: &umya_spreadsheet::Workbook) -> Vec<String> {
    book.sheet_collection()
        .iter()
        .flat_map(|s| s.tables().iter().map(|t| t.display_name().to_string()))
        .collect()
}

/// Проверка отображаемого имени таблицы: без пробелов, не адрес ячейки,
/// не длиннее лимита Excel.
fn validate_table_name(name: &str) -> Result<(), ExcelError> {
    if name.is_empty() {
        return Err(ExcelError::Usage(
            "add_table: имя таблицы пустое".to_string(),
        ));
    }
    if name.len() > 255 {
        return Err(ExcelError::Usage(
            "add_table: имя таблицы длиннее 255 символов (лимит Excel)".to_string(),
        ));
    }
    if name.chars().any(char::is_whitespace) {
        return Err(ExcelError::Usage(format!(
            "add_table: имя таблицы '{name}' содержит пробелы — Excel их не допускает"
        )));
    }
    if parse_cell_addr(name).is_ok() {
        return Err(ExcelError::Usage(format!(
            "add_table: имя таблицы '{name}' совпадает с адресом ячейки — Excel не допускает"
        )));
    }
    Ok(())
}

/// `add_table`: умная таблица Excel (Table). Первая строка диапазона —
/// заголовки (пустые ячейки получают имена Column1..N); style по умолчанию —
/// встроенный TableStyleMedium2 с полосами строк. Имя не указано — первое
/// свободное Table1, Table2, ...
pub(crate) fn apply_add_table(
    book: &mut umya_spreadsheet::Workbook,
    sheet: &str,
    range: &str,
    name: Option<&str>,
) -> Result<(), ExcelError> {
    let range = range.trim().replace('$', "");
    let ((c1, r1), (c2, r2)) = parse_range(&range)?;
    if c2 < c1 || r2 < r1 {
        return Err(ExcelError::Usage(format!(
            "add_table: диапазон '{range}' вывернут наизнанку"
        )));
    }

    let taken = existing_table_names(book);
    let display_name = match name {
        Some(user_name) => {
            validate_table_name(user_name)?;
            if taken.iter().any(|t| t == user_name) {
                return Err(ExcelError::Usage(format!(
                    "add_table: имя таблицы '{user_name}' уже используется в книге"
                )));
            }
            user_name.to_string()
        }
        None => {
            let mut n = 1;
            while taken.iter().any(|t| t == &format!("Table{n}")) {
                n += 1;
            }
            format!("Table{n}")
        }
    };

    // Заголовки из первой строки диапазона; пустые — ColumnN.
    let headers: Vec<String> = {
        let sheet_ref = sheet_mut(book, sheet)?;
        (c1..=c2)
            .map(|col| {
                let text = sheet_ref
                    .cell((col, r1))
                    .map(|c| c.value().to_string())
                    .unwrap_or_default();
                let trimmed = text.trim().to_string();
                if trimmed.is_empty() {
                    format!("Column{}", col - c1 + 1)
                } else {
                    trimmed
                }
            })
            .collect()
    };

    let mut table = Table::default();
    table.set_area(((c1, r1), (c2, r2)));
    table.set_name(&display_name);
    table.set_display_name(&display_name);
    table.set_totals_row_shown(false);
    table.set_auto_filter(range.clone());
    for header in headers {
        let mut col = TableColumn::default();
        col.set_name(header);
        table.add_column(col);
    }
    table.set_style_info(Some(TableStyleInfo::new(
        "TableStyleMedium2",
        ShowColumn::Hide,
        ShowColumn::Hide,
        ShowStripes::Show,
        ShowStripes::Hide,
    )));

    let sheet_ref = sheet_mut(book, sheet)?;
    sheet_ref.add_table(table);
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::ops::apply_ops;

    fn book() -> umya_spreadsheet::Workbook {
        umya_spreadsheet::new_file()
    }

    #[test]
    fn set_rows_writes_block_with_types() {
        let mut book = book();
        let ops = serde_json::json!([
            {"op": "add_sheet", "name": "Продажи"},
            {"op": "set_rows", "sheet": "Продажи", "addr": "A1", "rows": [
                ["Дата", "Товар", "Цена", "Итог"],
                ["2026-10-01", "Смартфон", 59990, {"formula": "C2*2"}],
                [null, "Наушники", 4990.5, {"formula": "C3*2", "number_format": "#,##0.00"}]
            ]}
        ]);
        apply_ops(&mut book, ops.as_array().unwrap()).unwrap();

        let sheet = book.sheet_by_name("Продажи").unwrap();
        assert_eq!(sheet.cell("A1").unwrap().value(), "Дата");
        // Число — числом, строка с датой — текстом.
        assert_eq!(sheet.cell("C2").unwrap().data_type(), "n");
        assert_eq!(sheet.cell("A2").unwrap().data_type(), "s");
        assert_eq!(sheet.cell("D2").unwrap().formula(), "C2*2");
        // null-ячейка не создана.
        assert!(sheet.cell("A3").is_none());
        // Формат числа из объекта-ячейки дошёл до D3 (колонка «Итог»).
        assert_eq!(
            sheet
                .cell("D3")
                .unwrap()
                .style()
                .numbering_format()
                .expect("формат задан")
                .format_code(),
            "#,##0.00"
        );
    }

    #[test]
    fn set_rows_ragged_and_style_payload() {
        let mut book = book();
        let ops = serde_json::json!([
            {"op": "set_rows", "sheet": "Sheet1", "addr": "B2", "rows": [
                ["только один", {"value": 7, "style": {"bold": true}}]
            ]}
        ]);
        apply_ops(&mut book, ops.as_array().unwrap()).unwrap();
        let sheet = book.sheet_by_name("Sheet1").unwrap();
        assert_eq!(sheet.cell("B2").unwrap().value(), "только один");
        assert_eq!(sheet.cell("C2").unwrap().data_type(), "n");
        assert!(sheet
            .cell("C2")
            .unwrap()
            .style()
            .font()
            .expect("жирный задан стилем")
            .bold());
    }

    #[test]
    fn set_rows_rejects_empty_and_leading_eq() {
        let mut book = book();
        let ops = serde_json::json!([
            {"op": "set_rows", "sheet": "Sheet1", "addr": "A1", "rows": [[{"formula": "=A1+1"}]]}
        ]);
        let err = apply_ops(&mut book, ops.as_array().unwrap()).unwrap_err();
        assert!(err.to_string().contains("БЕЗ ведущего '='"), "{err}");

        let ops = serde_json::json!([{"op": "set_rows", "sheet": "Sheet1", "addr": "A1", "rows": []}]);
        let err = apply_ops(&mut book, ops.as_array().unwrap()).unwrap_err();
        assert!(err.to_string().contains("rows пуст"), "{err}");

        let ops = serde_json::json!([
            {"op": "set_rows", "sheet": "Sheet1", "addr": "A1", "rows": [[{"value": 1, "чужое": 2}]]}
        ]);
        let err = apply_ops(&mut book, ops.as_array().unwrap()).unwrap_err();
        assert!(err.to_string().contains("чужое"), "{err}");
    }

    #[test]
    fn format_range_covers_all_cells() {
        let mut book = book();
        let ops = serde_json::json!([
            {"op": "set_rows", "sheet": "Sheet1", "addr": "A1", "rows": [["a", "b"], [1, 2]]},
            {"op": "format_range", "sheet": "Sheet1", "range": "A2:B2", "number_format": "#,##0.00"},
            {"op": "format_range", "sheet": "Sheet1", "range": "A1:B1", "style": {"bold": true, "fill": "DDDDDD"}}
        ]);
        apply_ops(&mut book, ops.as_array().unwrap()).unwrap();
        let sheet = book.sheet_by_name("Sheet1").unwrap();
        for addr in ["A2", "B2"] {
            assert_eq!(
                sheet
                    .cell(addr)
                    .unwrap()
                    .style()
                    .numbering_format()
                    .expect("формат задан")
                    .format_code(),
                "#,##0.00",
                "{addr}"
            );
        }
        for addr in ["A1", "B1"] {
            assert!(
                sheet
                    .cell(addr)
                    .unwrap()
                    .style()
                    .font()
                    .expect("жирный задан стилем")
                    .bold(),
                "{addr}"
            );
        }
        // Ничего не применять — честная ошибка.
        let ops = serde_json::json!([
            {"op": "format_range", "sheet": "Sheet1", "range": "A1:B2"}
        ]);
        let err = apply_ops(&mut book, ops.as_array().unwrap()).unwrap_err();
        assert!(err.to_string().contains("нечего применять"), "{err}");
    }

    #[test]
    fn add_table_builds_headers_and_persists_roundtrip() {
        let dir = std::env::temp_dir().join(format!("ko_excel_add_table_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("table_roundtrip.xlsx");
        let _ = std::fs::remove_file(&path);

        let mut book = book();
        let ops = serde_json::json!([
            {"op": "set_rows", "sheet": "Sheet1", "addr": "A1", "rows": [
                ["Дата", "Товар", "Цена"],
                ["2026-10-01", "Хлеб", 45],
                ["2026-10-02", "Молоко", 89]
            ]},
            {"op": "add_table", "sheet": "Sheet1", "range": "A1:C3"}
        ]);
        apply_ops(&mut book, ops.as_array().unwrap()).unwrap();

        // В памяти: таблица с тремя колонками по заголовкам.
        let sheet = book.sheet_by_name("Sheet1").unwrap();
        let tables = sheet.tables();
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].display_name(), "Table1");
        let names: Vec<&str> = tables[0].columns().iter().map(|c| c.name()).collect();
        assert_eq!(names, ["Дата", "Товар", "Цена"]);

        // Roundtrip через диск: writer обязан сохранить tableParts.
        crate::save_workbook(&book, &path).unwrap();
        let reread = crate::open_workbook(&path).unwrap();
        let sheet = reread.sheet_by_name("Sheet1").unwrap();
        assert!(sheet.has_table(), "таблица пережила запись/чтение");
        assert_eq!(sheet.tables()[0].display_name(), "Table1");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn add_table_names_and_validation() {
        let mut book = book();
        let ops = serde_json::json!([
            {"op": "set_rows", "sheet": "Sheet1", "addr": "A1", "rows": [["h1", "h2"], [1, 2], [3, 4]]},
            {"op": "add_table", "sheet": "Sheet1", "range": "A1:B3", "name": "Продажи 2026"}
        ]);
        // Пробел в имени — честная ошибка.
        let err = apply_ops(&mut book, ops.as_array().unwrap()).unwrap_err();
        assert!(err.to_string().contains("пробелы"), "{err}");

        // Вторая таблица без имени получает Table2 (Table1 уже занята).
        let ops = serde_json::json!([
            {"op": "add_table", "sheet": "Sheet1", "range": "A1:B3", "name": "MyTable"},
            {"op": "add_table", "sheet": "Sheet1", "range": "A1:B3"}
        ]);
        apply_ops(&mut book, ops.as_array().unwrap()).unwrap();
        let sheet = book.sheet_by_name("Sheet1").unwrap();
        assert_eq!(sheet.tables()[0].display_name(), "MyTable");
        // Имен Table1 в книге ещё нет — свободное имя начинается с Table1.
        assert_eq!(sheet.tables()[1].display_name(), "Table1");

        // Дубликат имени и имя-адрес — честные ошибки.
        let ops = serde_json::json!([{"op": "add_table", "sheet": "Sheet1", "range": "A1:B3", "name": "MyTable"}]);
        let err = apply_ops(&mut book, ops.as_array().unwrap()).unwrap_err();
        assert!(err.to_string().contains("уже используется"), "{err}");
        let ops = serde_json::json!([{"op": "add_table", "sheet": "Sheet1", "range": "A1:B3", "name": "A1"}]);
        let err = apply_ops(&mut book, ops.as_array().unwrap()).unwrap_err();
        assert!(err.to_string().contains("адресом ячейки"), "{err}");
    }
}
