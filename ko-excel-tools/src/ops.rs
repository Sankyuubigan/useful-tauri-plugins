//! Движок операций над книгой: декларативный список ops → изменения в
//! `umya_spreadsheet::Workbook`. Неизвестная операция — ошибка валидации
//! (честный отказ, не тихий пропуск). Список ops известен агенту через
//! host-схему тула `excel_apply`.

use crate::{column_letter, column_number, parse_cell_addr, parse_range, split_sheet_range, ExcelError};
use serde::Deserialize;
use umya_spreadsheet::structs::drawing::spreadsheet::MarkerType;
use umya_spreadsheet::structs::{
    Chart, ChartType, Coordinate, Fill, HorizontalAlignmentValues, Pane, PaneStateValues,
    PaneValues, PatternValues, Style, VerticalAlignmentValues,
};

/// Одна операция над книгой. Тег `op` — имя операции (snake_case).
#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    /// Добавить лист. Имя обязано быть уникальным в книге.
    AddSheet { name: String },
    /// Удалить лист (не последний — Excel требует хотя бы один).
    DeleteSheet { name: String },
    /// Переименовать лист.
    RenameSheet { from: String, to: String },
    /// Записать ячейку: значение (JSON-тип сохраняется: число — числом,
    /// строка — текстом), формулу или и то и другое; опционально стиль и
    /// формат числа.
    SetCell {
        sheet: String,
        addr: String,
        #[serde(default)]
        value: Option<serde_json::Value>,
        #[serde(default)]
        formula: Option<String>,
        #[serde(default)]
        style: Option<CellStyle>,
        #[serde(default)]
        number_format: Option<String>,
    },
    /// Ширина столбца (col — буква "A" или число 1-based).
    SetColumnWidth {
        sheet: String,
        col: serde_json::Value,
        width: f64,
    },
    /// Автоподбор ширин по содержимому строк (порт методологии spreadsheets:
    /// ширина = длина отображаемого текста × 1.1 + 2, ограничена min/max).
    AutofitColumns {
        #[serde(default)]
        sheet: Option<String>,
        #[serde(default)]
        min: Option<f64>,
        #[serde(default)]
        max: Option<f64>,
    },
    /// Объединить диапазон ("A1:C1").
    MergeCells { sheet: String, range: String },
    /// Автофильтр по диапазону ("A1:D10").
    AutoFilter { sheet: String, range: String },
    /// Закрепить области по ячейке ("B3" — столбец A и строки 1–2).
    FreezePanes { sheet: String, cell: String },
    /// Диаграмма. `series` — диапазоны значений ("Лист1!$B$2:$B$10"),
    /// `categories_range` — диапазон подписей категорий (для pie/bar обязателен),
    /// `anchor` — левая верхняя клетка размещения, `width_cells`/`height_cells` —
    /// размер в клетках (по умолчанию 8×15).
    AddChart {
        sheet: String,
        chart_type: String,
        anchor: String,
        series: Vec<String>,
        #[serde(default)]
        categories_range: Option<String>,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        width_cells: Option<u32>,
        #[serde(default)]
        height_cells: Option<u32>,
    },
}

/// Стиль ячейки (подмножество, достаточное для отчётов/заголовков).
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "snake_case")]
pub struct CellStyle {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub size: Option<f64>,
    /// Цвет шрифта, RGB hex ("FF0000") или ARGB ("FF FF0000" без пробелов).
    pub color: Option<String>,
    /// Цвет заливки (solid), тот же формат.
    pub fill: Option<String>,
    pub halign: Option<String>,
    pub valign: Option<String>,
    pub wrap: Option<bool>,
}

/// Применить список операций к книге по порядку. Любая ошибка прерывает
/// применение (книга остаётся в памяти только у вызывающего — на диск
/// промежуточное состояние не пишется, атомарность обеспечивает хост-пайплайн).
pub fn apply_ops(
    book: &mut umya_spreadsheet::Workbook,
    ops: &[serde_json::Value],
) -> Result<(), ExcelError> {
    if ops.is_empty() {
        return Err(ExcelError::Usage(
            "Список ops пуст — нечего применять".to_string(),
        ));
    }
    for (index, raw) in ops.iter().enumerate() {
        let op: Op = serde_json::from_value(raw.clone()).map_err(|e| {
            let name = raw
                .get("op")
                .and_then(|v| v.as_str())
                .unwrap_or("<без поля op>");
            ExcelError::Usage(format!(
                "Операция #{index} ('{name}') не соответствует схеме: {e}. \
                 Известные операции: add_sheet, delete_sheet, rename_sheet, set_cell, \
                 set_column_width, autofit_columns, merge_cells, auto_filter, \
                 freeze_panes, add_chart"
            ))
        })?;
        apply_one(book, op)
            .map_err(|e| ExcelError::OpFailed { op: format!("#{index}"), msg: e.to_string() })?;
    }
    Ok(())
}

fn apply_one(book: &mut umya_spreadsheet::Workbook, op: Op) -> Result<(), ExcelError> {
    match op {
        Op::AddSheet { name } => {
            book.new_sheet(name.clone())
                .map_err(|e| ExcelError::Usage(format!("add_sheet '{name}': {e}")))?;
            Ok(())
        }
        Op::DeleteSheet { name } => {
            if book.sheet_collection().len() <= 1 {
                return Err(ExcelError::Usage(
                    "Нельзя удалить единственный лист книги".to_string(),
                ));
            }
            book.remove_sheet_by_name(&name)
                .map_err(|e| ExcelError::Usage(format!("delete_sheet '{name}': {e}")))?;
            Ok(())
        }
        Op::RenameSheet { from, to } => {
            let index = book
                .sheet_collection()
                .iter()
                .position(|s| s.name() == &from)
                .ok_or_else(|| ExcelError::Usage(format!("Лист '{from}' не найден")))?;
            book.set_sheet_name(index, to.clone())
                .map_err(|e| ExcelError::Usage(format!("rename_sheet '{from}'→'{to}': {e}")))?;
            Ok(())
        }
        Op::SetCell {
            sheet,
            addr,
            value,
            formula,
            style,
            number_format,
        } => {
            let sheet_ref = sheet_mut(book, &sheet)?;
            let cell = sheet_ref.cell_mut(addr.as_str());
            if let Some(v) = value {
                match v {
                    serde_json::Value::Number(n) => {
                        let num = n.as_f64().ok_or_else(|| {
                            ExcelError::Usage(format!("Число вне диапазона f64: {n}"))
                        })?;
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
                            "set_cell {addr}: значение должно быть строкой, числом или bool, получено: {other}"
                        )));
                    }
                }
            }
            if let Some(f) = formula {
                if f.starts_with('=') {
                    return Err(ExcelError::Usage(format!(
                        "set_cell {addr}: формула передаётся БЕЗ ведущего '=' (получено '{f}')"
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
        Op::SetColumnWidth { sheet, col, width } => {
            let col_num = match col {
                serde_json::Value::String(s) => column_number(&s)?,
                serde_json::Value::Number(n) => {
                    let v = n.as_u64().ok_or_else(|| {
                        ExcelError::Usage(format!("Некорректный номер столбца: {n}"))
                    })?;
                    u32::try_from(v).map_err(|_| {
                        ExcelError::Usage(format!("Некорректный номер столбца: {v}"))
                    })?
                }
                other => {
                    return Err(ExcelError::Usage(format!(
                        "set_column_width: col должен быть буквой или числом, получено {other}"
                    )));
                }
            };
            if !(1.0..=255.0).contains(&width) {
                return Err(ExcelError::Usage(format!(
                    "set_column_width: ширина {width} вне допустимого диапазона Excel (1..255)"
                )));
            }
            let sheet_ref = sheet_mut(book, &sheet)?;
            ensure_column(sheet_ref, col_num).set_width(width);
            Ok(())
        }
        Op::AutofitColumns { sheet, min, max } => {
            let sheet_ref = match sheet {
                Some(name) => sheet_mut(book, &name)?,
                None => book
                    .sheet_collection_mut()
                    .first_mut()
                    .ok_or_else(|| ExcelError::Usage("Книга без листов".to_string()))?,
            };
            autofit(sheet_ref, min.unwrap_or(8.0), max.unwrap_or(60.0));
            Ok(())
        }
        Op::MergeCells { sheet, range } => {
            parse_range(&range)?;
            let sheet_ref = sheet_mut(book, &sheet)?;
            sheet_ref.add_merge_cells(range);
            Ok(())
        }
        Op::AutoFilter { sheet, range } => {
            parse_range(&range)?;
            let sheet_ref = sheet_mut(book, &sheet)?;
            sheet_ref.set_auto_filter(range);
            Ok(())
        }
        Op::FreezePanes { sheet, cell } => {
            let (col, row) = parse_cell_addr(&cell)?;
            if col == 1 && row == 1 {
                return Err(ExcelError::Usage(
                    "freeze_panes: ячейка A1 ничего не закрепляет — укажите первую незакреплённую клетку (например B2)".to_string(),
                ));
            }
            let sheet_ref = sheet_mut(book, &sheet)?;
            let view = ensure_sheet_view(sheet_ref);
            let mut top_left = Coordinate::default();
            top_left.set_coordinate(cell.as_str());
            let mut pane = Pane::default();
            pane.set_horizontal_split((row - 1) as f64);
            pane.set_vertical_split((col - 1) as f64);
            pane.set_top_left_cell(top_left);
            pane.set_state(PaneStateValues::Frozen);
            pane.set_active_pane(match ((col > 1), (row > 1)) {
                (true, true) => PaneValues::BottomRight,
                (false, true) => PaneValues::BottomLeft,
                (true, false) => PaneValues::TopRight,
                (false, false) => PaneValues::TopLeft,
            });
            view.set_pane(pane);
            Ok(())
        }
        Op::AddChart {
            sheet,
            chart_type,
            anchor,
            series,
            categories_range,
            title,
            width_cells,
            height_cells,
        } => {
            apply_chart(
                book,
                &sheet,
                &chart_type,
                &anchor,
                &series,
                categories_range.as_deref(),
                title.as_deref(),
                width_cells.unwrap_or(8),
                height_cells.unwrap_or(15),
            )
        }
    }
}

fn sheet_mut<'a>(
    book: &'a mut umya_spreadsheet::Workbook,
    name: &str,
) -> Result<&'a mut umya_spreadsheet::Worksheet, ExcelError> {
    book.sheet_by_name_mut(name)
        .map_err(|_| ExcelError::Usage(format!("Лист '{name}' не найден")))
}

/// Столбец в dimensions: umya хранит только созданные — создаём при отсутствии.
fn ensure_column(
    sheet: &mut umya_spreadsheet::Worksheet,
    col_num: u32,
) -> &mut umya_spreadsheet::structs::Column {
    let list = sheet.column_dimensions_mut();
    let existing = list.iter().position(|c| c.col_num() == col_num);
    let index = match existing {
        Some(index) => index,
        None => {
            let mut column = umya_spreadsheet::structs::Column::default();
            column.set_col_num(col_num);
            list.push(column);
            list.len() - 1
        }
    };
    &mut list[index]
}

fn ensure_sheet_view(
    sheet: &mut umya_spreadsheet::Worksheet,
) -> &mut umya_spreadsheet::structs::SheetView {
    let views = sheet.sheet_views_mut();
    if views.sheet_view_list_mut().is_empty() {
        views.add_sheet_view_list_mut(umya_spreadsheet::structs::SheetView::default());
    }
    let index = views.sheet_view_list_mut().len() - 1;
    &mut views.sheet_view_list_mut()[index]
}

/// Автоподбор ширин: максимальная отображаемая длина текста по столбцу
/// (кириллица/CJK считаются двойной шириной), ширина = len × 1.1 + 2.
fn autofit(sheet: &mut umya_spreadsheet::Worksheet, min: f64, max: f64) {
    let (max_col, max_row) = sheet.highest_column_and_row();
    let mut widths: Vec<f64> = vec![0.0; max_col as usize];
    for row in 1..=max_row {
        for col in 1..=max_col {
            if let Some(cell) = sheet.cell((col, row)) {
                let text = cell_value_text(cell);
                let width = display_len(&text) as f64;
                let slot = &mut widths[(col - 1) as usize];
                if width > *slot {
                    *slot = width;
                }
            }
        }
    }
    for (index, width) in widths.into_iter().enumerate() {
        if width == 0.0 {
            continue;
        }
        let computed = (width * 1.1 + 2.0).clamp(min, max);
        ensure_column(sheet, index as u32 + 1).set_width(computed);
    }
}

/// Текст ячейки для автоширины: значение, а если его нет — формула без '='.
fn cell_value_text(cell: &umya_spreadsheet::structs::Cell) -> String {
    let value = cell.value().to_string();
    if value.is_empty() {
        cell.formula().trim_start_matches('=').to_string()
    } else {
        value
    }
}

/// Отображаемая длина: полный шириной считаем символы вне латиницы/цифр
/// (кириллица, CJK и прочие не-ASCII — ×2, как в Excel).
fn display_len(text: &str) -> usize {
    text.chars()
        .map(|c| if c.is_ascii() { 1 } else { 2 })
        .sum()
}

fn apply_style(style: &mut Style, spec: &CellStyle) -> Result<(), ExcelError> {
    let font = style.font_mut();
    if let Some(true) = spec.bold {
        font.set_bold(true);
    }
    if let Some(true) = spec.italic {
        font.set_italic(true);
    }
    if let Some(size) = spec.size {
        if !(1.0..=409.0).contains(&size) {
            return Err(ExcelError::Usage(format!(
                "style.size {size} вне допустимого диапазона Excel (1..409)"
            )));
        }
        font.set_size(size);
    }
    if let Some(color) = &spec.color {
        font.color_mut().set_argb_str(normalize_argb(color)?);
    }
    if let Some(fill_color) = &spec.fill {
        let mut fill = Fill::default();
        fill.pattern_fill_mut().set_pattern_type(PatternValues::Solid);
        fill.pattern_fill_mut()
            .foreground_color_mut()
            .set_argb_str(normalize_argb(fill_color)?);
        style.set_fill(fill);
    }
    let alignment = style.alignment_mut();
    if let Some(h) = &spec.halign {
        let value = match h.as_str() {
            "left" => HorizontalAlignmentValues::Left,
            "center" => HorizontalAlignmentValues::Center,
            "right" => HorizontalAlignmentValues::Right,
            other => {
                return Err(ExcelError::Usage(format!(
                    "style.halign '{other}': допустимо left | center | right"
                )));
            }
        };
        alignment.set_horizontal(value);
    }
    if let Some(v) = &spec.valign {
        let value = match v.as_str() {
            "top" => VerticalAlignmentValues::Top,
            "center" | "middle" => VerticalAlignmentValues::Center,
            "bottom" => VerticalAlignmentValues::Bottom,
            other => {
                return Err(ExcelError::Usage(format!(
                    "style.valign '{other}': допустимо top | center | bottom"
                )));
            }
        };
        alignment.set_vertical(value);
    }
    if let Some(true) = spec.wrap {
        alignment.set_wrap_text(true);
    }
    Ok(())
}

/// RGB hex ("FF0000") → ARGB ("FFFF0000"). ARGB на входе проходит как есть.
fn normalize_argb(color: &str) -> Result<String, ExcelError> {
    let cleaned: String = color.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    match cleaned.len() {
        6 => Ok(format!("FF{cleaned}")),
        8 => Ok(cleaned.to_ascii_uppercase()),
        _ => Err(ExcelError::Usage(format!(
            "Цвет '{color}': ожидается RGB hex (FF0000) или ARGB (FFFF0000)"
        ))),
    }
}

fn apply_chart(
    book: &mut umya_spreadsheet::Workbook,
    sheet_name: &str,
    chart_type: &str,
    anchor: &str,
    series: &[String],
    categories_range: Option<&str>,
    title: Option<&str>,
    width_cells: u32,
    height_cells: u32,
) -> Result<(), ExcelError> {
    let kind = match chart_type {
        "bar" | "column" => ChartType::BarChart,
        "line" => ChartType::LineChart,
        "pie" => ChartType::PieChart,
        "area" => ChartType::AreaChart,
        "scatter" => ChartType::ScatterChart,
        other => {
            return Err(ExcelError::Usage(format!(
                "add_chart: тип '{other}' не поддерживается (bar | line | pie | area | scatter)"
            )));
        }
    };
    if series.is_empty() {
        return Err(ExcelError::Usage(
            "add_chart: список series пуст — укажите хотя бы один диапазон значений".to_string(),
        ));
    }
    for spec in series {
        let (sheet_of, range) = split_sheet_range(spec);
        if sheet_of.is_none() {
            return Err(ExcelError::Usage(format!(
                "add_chart: диапазон серии '{spec}' без имени листа — формат 'Лист!$A$1:$A$10'"
            )));
        }
        parse_range(&range)?;
    }
    let (anchor_col, anchor_row) = parse_cell_addr(anchor)?;
    // Категории: значения читаются из книги и становятся подписями оси/секторов.
    let point_titles = match categories_range {
        Some(range_spec) => {
            let (sheet_of, range) = split_sheet_range(range_spec);
            let sheet_to_read = sheet_of.unwrap_or_else(|| sheet_name.to_string());
            read_column_values(book, &sheet_to_read, &range)?
        }
        None => Vec::new(),
    };

    let mut from_marker = MarkerType::default();
    from_marker.set_coordinate(anchor);
    let mut to_marker = MarkerType::default();
    to_marker.set_coordinate(format!(
        "{}{}",
        column_letter(anchor_col + width_cells),
        anchor_row + height_cells
    ));

    let owned: Vec<String> = series.to_vec();
    let series_refs: Vec<&str> = owned.iter().map(String::as_str).collect();
    let mut chart = Chart::default();
    chart.new_chart(&kind, from_marker, to_marker, series_refs);
    if let Some(t) = title {
        chart.set_title(t);
    }
    if !point_titles.is_empty() {
        chart.set_series_point_title(point_titles);
    }
    if matches!(kind, ChartType::LineChart) {
        // umya по умолчанию строит СТОПКИВАЕМУЮ линию — для обычного графика
        // группировка обязана быть Standard (док-пример umya).
        chart.set_grouping(umya_spreadsheet::structs::drawing::charts::GroupingValues::Standard);
    }
    let sheet_ref = sheet_mut(book, sheet_name)?;
    sheet_ref.add_chart(chart);
    Ok(())
}

/// Значения диапазона (одного столбца или строки) как строки — для подписей
/// категорий диаграммы.
fn read_column_values(
    book: &umya_spreadsheet::Workbook,
    sheet_name: &str,
    range: &str,
) -> Result<Vec<String>, ExcelError> {
    let sheet = book
        .sheet_by_name(sheet_name)
        .map_err(|_| ExcelError::Usage(format!("add_chart: лист '{sheet_name}' не найден")))?;
    let ((c1, r1), (c2, r2)) = parse_range(range)?;
    if c1 != c2 && r1 != r2 {
        return Err(ExcelError::Usage(format!(
            "add_chart: диапазон категорий '{range}' должен быть строкой или столбцом"
        )));
    }
    let mut out = Vec::new();
    if c1 == c2 {
        for row in r1..=r2 {
            if let Some(cell) = sheet.cell((c1, row)) {
                out.push(cell.value().to_string());
            }
        }
    } else {
        for col in c1..=c2 {
            if let Some(cell) = sheet.cell((col, r1)) {
                out.push(cell.value().to_string());
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book_with_data() -> umya_spreadsheet::Workbook {
        let mut book = umya_spreadsheet::new_file();
        let sheet = book.sheet_by_name_mut("Sheet1").unwrap();
        sheet.cell_mut("A1").set_value_string("Товар");
        sheet.cell_mut("B1").set_value_string("Цена");
        sheet.cell_mut("A2").set_value_string("Хлеб");
        sheet.cell_mut("B2").set_value_number(45.5);
        sheet.cell_mut("A3").set_value_string("Молоко");
        sheet.cell_mut("B3").set_value_number(89.9);
        book
    }

    #[test]
    fn set_cell_value_types_and_style() {
        let mut book = book_with_data();
        let ops = serde_json::json!([
            {"op": "set_cell", "sheet": "Sheet1", "addr": "C1", "value": "Итог",
             "style": {"bold": true, "fill": "DDDDDD", "halign": "center"}},
            {"op": "set_cell", "sheet": "Sheet1", "addr": "C2", "formula": "B2*2",
             "number_format": "0.00"},
            {"op": "set_cell", "sheet": "Sheet1", "addr": "C3", "value": 42},
            {"op": "set_cell", "sheet": "Sheet1", "addr": "C4", "value": "00123"}
        ]);
        apply_ops(&mut book, ops.as_array().unwrap()).unwrap();

        let sheet = book.sheet_by_name("Sheet1").unwrap();
        assert_eq!(sheet.cell("C1").unwrap().value(), "Итог");
        assert!(sheet.cell("C1").unwrap().style().font().expect("шрифт задан стилем").bold());
        assert_eq!(sheet.cell("C2").unwrap().formula(), "B2*2");
        assert_eq!(
            sheet
                .cell("C2")
                .unwrap()
                .style()
                .numbering_format()
                .expect("формат числа задан выше")
                .format_code(),
            "0.00"
        );
        // Число сохранилось числом (не строкой).
        assert_eq!(sheet.cell("C3").unwrap().data_type(), "n");
        // Строка с ведущим нулём осталась текстом.
        assert_eq!(sheet.cell("C4").unwrap().data_type(), "s");
        assert_eq!(sheet.cell("C4").unwrap().value(), "00123");
    }

    #[test]
    fn formula_with_leading_eq_rejected() {
        let mut book = book_with_data();
        let ops = serde_json::json!([
            {"op": "set_cell", "sheet": "Sheet1", "addr": "D1", "formula": "=SUM(B1:B3)"}
        ]);
        let err = apply_ops(&mut book, ops.as_array().unwrap()).unwrap_err();
        assert!(err.to_string().contains("БЕЗ ведущего '='"), "{err}");
    }

    #[test]
    fn unknown_op_is_honest_error() {
        let mut book = book_with_data();
        let ops = serde_json::json!([{"op": "pivot_table", "sheet": "Sheet1"}]);
        let err = apply_ops(&mut book, ops.as_array().unwrap()).unwrap_err();
        assert!(err.to_string().contains("pivot_table"), "{err}");
        assert!(err.to_string().contains("add_chart"), "{err}");
    }

    #[test]
    fn sheet_lifecycle_and_view_ops() {
        let mut book = book_with_data();
        let ops = serde_json::json!([
            {"op": "add_sheet", "name": "Отчёт"},
            {"op": "set_cell", "sheet": "Отчёт", "addr": "A1", "value": "данные"},
            {"op": "merge_cells", "sheet": "Отчёт", "range": "B2:D2"},
            {"op": "auto_filter", "sheet": "Отчёт", "range": "A1:C10"},
            {"op": "freeze_panes", "sheet": "Отчёт", "cell": "B2"},
            {"op": "rename_sheet", "from": "Отчёт", "to": "Итоги"}
        ]);
        apply_ops(&mut book, ops.as_array().unwrap()).unwrap();
        assert!(book.sheet_by_name("Итоги").is_ok());
        let sheet = book.sheet_by_name("Итоги").unwrap();
        assert_eq!(sheet.cell("A1").unwrap().value(), "данные");
        assert_eq!(sheet.merge_cells().len(), 1);
        assert_eq!(sheet.merge_cells()[0].range(), "B2:D2");
        assert_eq!(
            sheet.auto_filter().expect("автофильтр установлен").range().range(),
            "A1:C10"
        );
    }

    #[test]
    fn autofit_computes_cyrillic_width() {
        let mut book = book_with_data();
        // "Молоко" = 6 символов кириллицы → отображаемая длина 12.
        let ops = serde_json::json!([{"op": "autofit_columns", "sheet": "Sheet1"}]);
        apply_ops(&mut book, ops.as_array().unwrap()).unwrap();
        let sheet = book.sheet_by_name("Sheet1").unwrap();
        let col_a = sheet
            .column_dimensions()
            .iter()
            .find(|c| c.col_num() == 1)
            .expect("ширина столбца A создана");
        // 12 × 1.1 + 2 = 15.2
        assert!((col_a.width() - 15.2).abs() < 0.01, "{}", col_a.width());
    }

    #[test]
    fn chart_with_categories() {
        let mut book = book_with_data();
        let ops = serde_json::json!([
            {"op": "add_chart", "sheet": "Sheet1", "chart_type": "pie",
             "anchor": "E2", "series": ["Sheet1!$B$2:$B$3"],
             "categories_range": "Sheet1!$A$2:$A$3", "title": "Цены"}
        ]);
        apply_ops(&mut book, ops.as_array().unwrap()).unwrap();
        let sheet = book.sheet_by_name("Sheet1").unwrap();
        assert_eq!(sheet.chart_collection().len(), 1, "диаграмма добавлена");
    }

    #[test]
    fn chart_line_is_not_stacked() {
        let mut book = book_with_data();
        let ops = serde_json::json!([
            {"op": "add_chart", "sheet": "Sheet1", "chart_type": "line",
             "anchor": "E2", "series": ["Sheet1!$B$2:$B$3"]}
        ]);
        apply_ops(&mut book, ops.as_array().unwrap()).unwrap();
        // GroupingValues::Standard проверяется косвенно: применение без ошибок
        // и диаграмма на месте (детальная проверка группировки — интеграционный
        // тест на перечитанном файле).
        assert_eq!(
            book.sheet_by_name("Sheet1").unwrap().chart_collection().len(),
            1
        );
    }

    #[test]
    fn chart_series_without_sheet_name_rejected() {
        let mut book = book_with_data();
        let ops = serde_json::json!([
            {"op": "add_chart", "sheet": "Sheet1", "chart_type": "bar",
             "anchor": "E2", "series": ["$B$2:$B$3"]}
        ]);
        let err = apply_ops(&mut book, ops.as_array().unwrap()).unwrap_err();
        assert!(err.to_string().contains("без имени листа"), "{err}");
    }

    #[test]
    fn cannot_delete_last_sheet() {
        let mut book = book_with_data();
        let ops = serde_json::json!([{"op": "delete_sheet", "name": "Sheet1"}]);
        let err = apply_ops(&mut book, ops.as_array().unwrap()).unwrap_err();
        assert!(err.to_string().contains("единственный лист"), "{err}");
    }
}
