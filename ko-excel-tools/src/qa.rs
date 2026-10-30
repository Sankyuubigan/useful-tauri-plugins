//! QA-гейты: статический аудит формул и скан ошибочных значений.
//!
//! Портировано из методологии плагина spreadsheets. Два уровня:
//! 1. **Статический** ([`static_issues`]) — по тексту формул: запрещённые
//!    динамические функции, внешние ссылки, `#REF!` в тексте. Работает БЕЗ
//!    LibreOffice.
//! 2. **По значениям** ([`scan_error_values`]) — 7 ошибок Excel в кэшированных
//!    значениях ячеек. Осмыслен только ПОСЛЕ пересчёта (soffice): у только что
//!    записанных формул кэша нет.

use crate::cell_address;
use serde::Serialize;
use umya_spreadsheet::Workbook;

/// Семь ошибок Excel, которые ищем в значениях (плагинный набор).
pub const EXCEL_ERROR_VALUES: [&str; 7] = [
    "#VALUE!", "#DIV/0!", "#REF!", "#NAME?", "#NULL!", "#NUM!", "#N/A",
];

/// Запрещённые динамические функции Excel 365: не работают в старых Excel,
/// LibreOffice и дают #NAME? при открытии файла на другой машине.
/// Правило методологии: только совместимый набор (VLOOKUP, INDEX+MATCH,
/// SUMIF(S), SUMPRODUCT...).
pub const FORBIDDEN_FUNCTIONS: &[&str] = &[
    "XLOOKUP", "FILTER", "UNIQUE", "SORT", "SORTBY", "LET", "LAMBDA",
    "RANDARRAY", "SEQUENCE", "BYROW", "BYCOL", "MAP", "REDUCE", "SCAN",
    "MAKEARRAY", "TEXTSPLIT", "VSTACK", "HSTACK", "TOCOL", "TOROW",
    "REGEXEXTRACT", "REGEXMATCH", "REGEXREPLACE",
];

/// Одна находка аудита.
#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    pub kind: String,
    pub sheet: String,
    pub addr: String,
    pub detail: String,
}

fn issue(kind: &str, sheet: &str, addr: &str, detail: String) -> Issue {
    Issue {
        kind: kind.to_string(),
        sheet: sheet.to_string(),
        addr: addr.to_string(),
        detail,
    }
}

/// Статический аудит по всем формулам книги: запрещённые функции, внешние
/// ссылки, #REF! в тексте формулы. Файл не модифицируется.
pub fn static_issues(book: &Workbook) -> Vec<Issue> {
    let mut out = Vec::new();
    for sheet in book.sheet_collection() {
        let sheet_name = sheet.name();
        let (max_col, max_row) = sheet.highest_column_and_row();
        for row in 1..=max_row {
            for col in 1..=max_col {
                let Some(cell) = sheet.cell((col, row)) else {
                    continue;
                };
                let formula = cell.formula();
                if formula.is_empty() {
                    continue;
                }
                let addr = cell_address(cell);
                let upper = formula.to_ascii_uppercase();
                for func in forbidden_function_hits(&upper) {
                    out.push(issue(
                        "forbidden_function",
                        &sheet_name,
                        &addr,
                        format!("динамическая функция {func} не работает в старых Excel и LibreOffice — замените на совместимый аналог"),
                    ));
                }
                if external_ref_hits(&upper) {
                    out.push(issue(
                        "external_reference",
                        &sheet_name,
                        &addr,
                        "ссылка на другую книгу/внешний файл — на другой машине формула сломается"
                            .to_string(),
                    ));
                }
                if upper.contains("#REF!") {
                    out.push(issue(
                        "ref_in_formula",
                        &sheet_name,
                        &addr,
                        "в тексте формулы стоит #REF! — ссылка указывает на удалённую область"
                            .to_string(),
                    ));
                }
            }
        }
    }
    out
}

/// Имена запрещённых функций, реально использованных в формуле.
/// Поиск "FN(" с границей слова: перед именем — не буква/цифра/точка/подчерк
/// (иначе SUMIFS отсечёт SUMIF-подобные ложные матчи в обратную сторону).
pub fn forbidden_function_hits(formula_upper: &str) -> Vec<String> {
    let mut out = Vec::new();
    for func in FORBIDDEN_FUNCTIONS {
        let needle = format!("{func}(");
        let mut start = 0;
        while let Some(pos) = formula_upper[start..].find(&needle) {
            let abs = start + pos;
            let boundary_ok = abs == 0
                || !formula_upper
                    .as_bytes()
                    .get(abs - 1)
                    .map(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'.')
                    .unwrap_or(false);
            if boundary_ok && !out.iter().any(|f: &String| f == func) {
                out.push(func.to_string());
            }
            start += pos + needle.len();
        }
    }
    out
}

/// Внешние ссылки: `'C:\path\[book.xlsx]Лист1'!A1`, `[1]Лист1!A1`,
/// `://` (веб-функции вроде WEBSERVICE). Регистронезависимо.
pub fn external_ref_hits(formula_upper: &str) -> bool {
    let formula = formula_upper.to_ascii_uppercase();
    formula.contains(".XLSX]")
        || formula.contains(".XLSM]")
        || formula.contains(".XLS]")
        || formula.contains("]!")
        || formula.contains("://")
}

/// Скан ошибочных значений по кэшированным значениям ячеек.
/// ВАЖНО: осмыслен только после пересчёта формул (soffice) — у свежезаписанных
/// формул кэша нет, и пустой результат не означает «ошибок нет».
pub fn scan_error_values(book: &Workbook) -> Vec<Issue> {
    let mut out = Vec::new();
    for sheet in book.sheet_collection() {
        let sheet_name = sheet.name();
        let (max_col, max_row) = sheet.highest_column_and_row();
        for row in 1..=max_row {
            for col in 1..=max_col {
                let Some(cell) = sheet.cell((col, row)) else {
                    continue;
                };
                let value = cell.value().to_string();
                let Some(err) = EXCEL_ERROR_VALUES.iter().find(|e| value == **e) else {
                    continue;
                };
                out.push(issue(
                    "error_value",
                    &sheet_name,
                    &cell_address(cell),
                    format!("в ячейке вычислилась ошибка {err}"),
                ));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forbidden_functions_detected_with_word_boundary() {
        assert_eq!(
            forbidden_function_hits("XLOOKUP(A1,B:B,2)"),
            vec!["XLOOKUP"]
        );
        // SORTBY не триггерит SORT (после SORT идёт B, не скобка).
        assert_eq!(forbidden_function_hits("SORTBY(A1:B2,1)"), vec!["SORTBY"]);
        // Но SORT в составной формуле ловится.
        assert_eq!(
            forbidden_function_hits("IFERROR(SORT(A1:A3),\"\")"),
            vec!["SORT"]
        );
        // Совместимые функции не ловятся.
        assert!(forbidden_function_hits("VLOOKUP(A1,B:C,2,0)").is_empty());
        assert!(forbidden_function_hits("SUMIFS(B:B,A:A,1)").is_empty());
        // Псевдо-имя с точкой не должно матчиться как SORT.
        assert!(forbidden_function_hits("MY.SORT(A1)").is_empty());
    }

    #[test]
    fn external_refs_detected() {
        assert!(external_ref_hits(
            "'[Книга1.xlsx]Лист1'!A1".to_uppercase().as_str()
        ));
        assert!(external_ref_hits("'C:\\DATA\\[BASE.XLS]SHEET1'!A1"));
        assert!(external_ref_hits("WEBSERVICE(\"HTTP://X\")"));
        assert!(!external_ref_hits("VLOOKUP(A1,B:C,2,0)"));
    }

    #[test]
    fn scan_error_values_finds_only_errors() {
        let mut book = umya_spreadsheet::new_file();
        let sheet = book.sheet_by_name_mut("Sheet1").unwrap();
        sheet.cell_mut("A1").set_value_string("ok");
        sheet.cell_mut("A2").set_error("#DIV/0!");
        sheet.cell_mut("A3").set_error("#N/A");
        let hits = scan_error_values(&book);
        assert_eq!(hits.len(), 2, "{hits:?}");
        assert_eq!(hits[0].addr, "A2");
        assert_eq!(hits[1].kind, "error_value");
    }

    #[test]
    fn static_issues_walks_formulas() {
        let mut book = umya_spreadsheet::new_file();
        let sheet = book.sheet_by_name_mut("Sheet1").unwrap();
        sheet.cell_mut("A1").set_formula("VLOOKUP(B1,C:D,2,0)");
        sheet.cell_mut("A2").set_formula("XLOOKUP(B1,C:C,2)");
        sheet.cell_mut("A3").set_formula("SUM(B1:B2)");
        let issues = static_issues(&book);
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert_eq!(issues[0].kind, "forbidden_function");
        assert_eq!(issues[0].addr, "A2");
    }
}
