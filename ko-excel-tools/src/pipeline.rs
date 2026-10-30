//! Атомарные пайплайны: apply (ops → сохранить → гейты → пересчёт) и audit
//! (гейты над исходником БЕЗ его модификации).
//!
//! Инвариант (§00.2 «отказ структурно невозможен»): файл с проваленными
//! гейтами НЕ доходит до пользователя — запись удаляется, агент получает
//! список проблем и чинит ops. Доставка возможна только в статусах
//! `ok_recalced` / `ok_without_recalc` / `ok_with_issues` (последний — по
//! явному `allow_issues: true` агента, после чтения списка проблем).

use crate::qa::{self, Issue};
use crate::{open_workbook, ops::apply_ops, save_workbook, ExcelError};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Статусы apply. Первые три — файл сохранён и приложен к ответу; остальные —
/// файла нет (удалён гейтом) либо исходник не тронут.
pub const APPLY_STATUS_DELIVERED: [&str; 3] =
    ["ok_recalced", "ok_without_recalc", "ok_with_issues"];

/// Запрос на применение операций.
pub struct ApplyRequest<'a> {
    /// Исходник для редактирования; None — создание новой книги.
    pub source: Option<&'a Path>,
    /// Каталог результатов (хост отдаёт `<exe>/excel_outputs/<session_id>/`).
    pub output_dir: &'a Path,
    /// Имя файла результата без расширения (None — автоген).
    pub output_stem: Option<&'a str>,
    /// Список операций (JSON-массив).
    pub ops: &'a Value,
    /// Сохранить файл даже при найденных проблемах (по умолчанию false:
    /// проблемы → удаление файла + отчёт агенту).
    pub allow_issues: bool,
}

/// Единая точка генерации уникального имени результата:
/// `<stem>_<HHMMSS>.xlsx`. Хронометр берётся из системы; коллизии в пределах
/// секунды снимает суффикс-счётчик вызывающего хоста (передаётся стемом).
fn unique_output_path(output_dir: &Path, stem: &str) -> PathBuf {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    output_dir.join(format!("{stem}_{timestamp}.xlsx"))
}

/// Санитизация имени файла: разрешаем буквы/цифры/пробел/дефис/подчерк/точку,
/// остальное — в подчерк (имя приходит от модели).
pub fn sanitize_stem(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.');
    let reserved = ["con", "prn", "aux", "nul"];
    let base = if trimmed.is_empty() {
        "workbook".to_string()
    } else {
        trimmed.to_string()
    };
    if reserved.contains(&base.to_ascii_lowercase().as_str()) {
        return format!("{base}_file");
    }
    base
}

/// Применить операции атомарно:
/// 1. загрузка исходника (или новая книга) → 2. ops → 3. запись в output_dir →
/// 4. статические гейты по сохранённому файлу → 5. пересчёт soffice (если
/// найден) + скан ошибок значений. Любой провал гейта (без allow_issues)
/// удаляет файл и возвращает статус с проблемами.
pub fn apply(
    request: &ApplyRequest,
    soffice: Option<&Path>,
    soffice_timeout: Duration,
) -> Result<Value, ExcelError> {
    let Value::Array(ops) = request.ops else {
        return Err(ExcelError::Usage(
            "ops обязаны быть JSON-массивом операций".to_string(),
        ));
    };

    let mut book = match request.source {
        Some(source) => open_workbook(source)?,
        None => umya_spreadsheet::new_file(),
    };
    apply_ops(&mut book, ops)?;

    let stem_raw: String = match (request.output_stem, request.source) {
        (Some(name), _) => name.to_string(),
        (None, Some(source)) => match source.file_stem() {
            Some(stem) => format!("{}_изменённый", stem.to_string_lossy()),
            None => "изменённый_файл".to_string(),
        },
        (None, None) => "новая_таблица".to_string(),
    };
    let stem = sanitize_stem(&stem_raw);
    let output_path = unique_output_path(request.output_dir, &stem);
    save_workbook(&book, &output_path)?;

    // Гейт 1: статический аудит ТОГО, что на диске (перечитываем файл).
    let written = open_workbook(&output_path)?;
    let issues = qa::static_issues(&written);
    if !issues.is_empty() && !request.allow_issues {
        let report = issues_to_json(&issues);
        let _ = std::fs::remove_file(&output_path);
        return Ok(json!({
            "status": "validation_failed",
            "output_path": Value::Null,
            "issues": report,
            "note": "Файл НЕ сохранён: исправьте операции (запрещённые функции / внешние ссылки) и примените снова. Список проблем выше."
        }));
    }

    // Гейт 2: пересчёт + скан ошибок значений. Только soffice даёт кэш
    // значений; без него — честный статус без обещаний.
    let Some(soffice) = soffice else {
        let status = if issues.is_empty() {
            "ok_without_recalc"
        } else {
            "ok_with_issues"
        };
        return Ok(json!({
            "status": status,
            "output_path": output_path.display().to_string(),
            "issues": issues_to_json(&issues),
            "recalculated": false,
            "note": "LibreOffice не найден: формулы записаны, но НЕ пересчитаны — ошибки значений (если есть) проявятся при открытии в Excel. Статическая проверка формул чистая."
        }));
    };

    crate::soffice::recalc_and_save(soffice, &output_path, soffice_timeout)?;
    let recalculated = open_workbook(&output_path)?;
    let errors = qa::scan_error_values(&recalculated);
    if !errors.is_empty() && !request.allow_issues {
        let report = issues_to_json(&errors);
        let _ = std::fs::remove_file(&output_path);
        return Ok(json!({
            "status": "recalc_errors",
            "output_path": Value::Null,
            "issues": report,
            "recalculated": true,
            "note": "Файл НЕ сохранён: после пересчёта вычислились ошибки (адреса в списке). Исправьте формулы/данные и примените снова."
        }));
    }
    let status = if issues.is_empty() {
        "ok_recalced"
    } else {
        "ok_with_issues"
    };
    Ok(json!({
        "status": status,
        "output_path": output_path.display().to_string(),
        "issues": issues_to_json(&issues),
        "recalculated": true,
    }))
}

/// Аудит исходника «найди ошибку»: статические гейты по оригиналу + пересчёт
/// на КОПИИ (исходник гарантированно не модифицируется) + скан ошибок.
pub fn audit(
    source: &Path,
    temp_dir: &Path,
    soffice: Option<&Path>,
    soffice_timeout: Duration,
) -> Result<Value, ExcelError> {
    let book = open_workbook(source)?;
    let issues = qa::static_issues(&book);
    let Some(soffice) = soffice else {
        return Ok(json!({
            "file": source.display().to_string(),
            "recalculated": false,
            "issues": issues_to_json(&issues),
            "errors_after_recalc": [],
            "note": "LibreOffice не найден: пересчёт невозможен, ошибки ЗНАЧЕНИЙ (#DIV/0! и т.п.) не видны — только статический анализ формул. Скачанный в Excel файл покажет такие ошибки адресно."
        }));
    };
    std::fs::create_dir_all(temp_dir)
        .map_err(|e| ExcelError::Soffice(format!("{}: {e}", temp_dir.display())))?;
    let stem = sanitize_stem(
        &source
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "audit".to_string()),
    );
    let copy_path = temp_dir.join(format!(
        "audit_{stem}_{}.xlsx",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    std::fs::copy(source, &copy_path)
        .map_err(|e| ExcelError::Soffice(format!("копия для аудита: {e}")))?;
    let recalc_result = crate::soffice::recalc_and_save(soffice, &copy_path, soffice_timeout);
    let outcome = match recalc_result {
        Ok(()) => {
            let recalculated = open_workbook(&copy_path)?;
            let errors = qa::scan_error_values(&recalculated);
            Ok(json!({
                "file": source.display().to_string(),
                "recalculated": true,
                "issues": issues_to_json(&issues),
                "errors_after_recalc": issues_to_json(&errors),
            }))
        }
        Err(e) => Ok(json!({
            "file": source.display().to_string(),
            "recalculated": false,
            "issues": issues_to_json(&issues),
            "errors_after_recalc": [],
            "note": format!("Пересчёт не удался ({e}) — доступны только статические находки выше."),
        })),
    };
    let _ = std::fs::remove_file(&copy_path);
    outcome
}

fn issues_to_json(issues: &[Issue]) -> Value {
    match serde_json::to_value(issues) {
        Ok(v) => v,
        Err(_) => json!([]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join(tag);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn create_source(dir: &Path) -> PathBuf {
        let source = dir.join("source.xlsx");
        let mut book = umya_spreadsheet::new_file();
        let sheet = book.sheet_by_name_mut("Sheet1").unwrap();
        sheet.cell_mut("A1").set_value_string("Цена");
        sheet.cell_mut("A2").set_value_number(10.0);
        save_workbook(&book, &source).unwrap();
        source
    }

    #[test]
    fn sanitize_stem_rules() {
        assert_eq!(sanitize_stem("отчёт 2026"), "отчёт 2026");
        assert_eq!(sanitize_stem("bad/name*"), "bad_name_");
        assert_eq!(sanitize_stem(".."), "workbook");
        assert_eq!(sanitize_stem("CON"), "CON_file");
        assert_eq!(sanitize_stem("  x.  "), "x");
    }

    #[test]
    fn apply_create_rejects_forbidden_function_and_keeps_dir_clean() {
        let dir = tmp_dir("apply_forbidden");
        let ops = json!([
            {"op": "set_cell", "sheet": "Sheet1", "addr": "A1", "value": "итог"},
            {"op": "set_cell", "sheet": "Sheet1", "addr": "B1", "formula": "XLOOKUP(A1,A:A,B:B)"}
        ]);
        let request = ApplyRequest {
            source: None,
            output_dir: &dir,
            output_stem: Some("тест"),
            ops: &ops,
            allow_issues: false,
        };
        let result = apply(&request, None, crate::soffice::DEFAULT_TIMEOUT).unwrap();
        assert_eq!(result["status"], "validation_failed");
        assert!(result["output_path"].is_null());
        let issue_list = result["issues"].as_array().unwrap();
        assert_eq!(issue_list.len(), 1);
        assert_eq!(issue_list[0]["kind"], "forbidden_function");
        // Файл удалён гейтом: в каталоге не осталось xlsx.
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|x| x == "xlsx").unwrap_or(false))
            .collect();
        assert!(leftovers.is_empty(), "битый файл не должен существовать");
    }

    #[test]
    fn apply_create_without_soffice_is_honest() {
        let dir = tmp_dir("apply_clean");
        let ops = json!([
            {"op": "set_cell", "sheet": "Sheet1", "addr": "A1", "value": "Цена"},
            {"op": "set_cell", "sheet": "Sheet1", "addr": "A2", "value": 100},
            {"op": "set_cell", "sheet": "Sheet1", "addr": "A3", "formula": "A2*2"},
            {"op": "autofit_columns", "sheet": "Sheet1"}
        ]);
        let request = ApplyRequest {
            source: None,
            output_dir: &dir,
            output_stem: None,
            ops: &ops,
            allow_issues: false,
        };
        let result = apply(&request, None, crate::soffice::DEFAULT_TIMEOUT).unwrap();
        assert_eq!(result["status"], "ok_without_recalc", "{result}");
        let path = PathBuf::from(result["output_path"].as_str().unwrap());
        assert!(path.is_file(), "файл сохранён: {}", path.display());
        let book = umya_spreadsheet::reader::xlsx::read(&path).unwrap();
        let sheet = book.sheet_by_name("Sheet1").unwrap();
        assert_eq!(sheet.cell("A3").unwrap().formula(), "A2*2");
    }

    #[test]
    fn apply_edit_output_stem_from_source() {
        let dir = tmp_dir("apply_edit");
        let source = create_source(&dir);
        let ops = json!([
            {"op": "set_cell", "sheet": "Sheet1", "addr": "B1", "value": "новое"}
        ]);
        let request = ApplyRequest {
            source: Some(&source),
            output_dir: &dir,
            output_stem: None,
            ops: &ops,
            allow_issues: false,
        };
        let result = apply(&request, None, crate::soffice::DEFAULT_TIMEOUT).unwrap();
        assert_eq!(result["status"], "ok_without_recalc", "{result}");
        let output = result["output_path"].as_str().unwrap();
        assert!(output.contains("source_изменённый"), "{output}");
        // Исходник не модифицирован (preserve-first).
        let original = umya_spreadsheet::reader::xlsx::read(&source).unwrap();
        assert!(
            original
                .sheet_by_name("Sheet1")
                .unwrap()
                .cell("B1")
                .is_none(),
            "в исходнике не должно быть B1"
        );
    }

    #[test]
    fn apply_allow_issues_keeps_file_with_warning() {
        let dir = tmp_dir("apply_allow");
        let ops = json!([
            {"op": "set_cell", "sheet": "Sheet1", "addr": "A1", "formula": "SUM(A:A)"}
        ]);
        let request = ApplyRequest {
            source: None,
            output_dir: &dir,
            output_stem: Some("спорный"),
            ops: &ops,
            allow_issues: true,
        };
        let result = apply(&request, None, crate::soffice::DEFAULT_TIMEOUT).unwrap();
        assert_eq!(result["status"], "ok_without_recalc", "{result}");
        assert!(result["issues"].as_array().unwrap().is_empty());
    }

    #[test]
    fn apply_rejects_non_array_ops() {
        let dir = tmp_dir("apply_bad_ops");
        let request = ApplyRequest {
            source: None,
            output_dir: &dir,
            output_stem: None,
            ops: &json!({"op": "set_cell"}),
            allow_issues: false,
        };
        let err = apply(&request, None, crate::soffice::DEFAULT_TIMEOUT).unwrap_err();
        assert!(err.to_string().contains("массивом"), "{err}");
    }

    #[test]
    fn audit_without_soffice_reports_static_only() {
        let dir = tmp_dir("audit_nosoffice");
        let source = dir.join("with_error.xlsx");
        let mut book = umya_spreadsheet::new_file();
        {
            let sheet = book.sheet_by_name_mut("Sheet1").unwrap();
            sheet.cell_mut("A1").set_value_number(5.0);
            sheet.cell_mut("A2").set_value_number(0.0);
            sheet.cell_mut("A3").set_formula("A1/A2");
            sheet.cell_mut("B1").set_formula("XLOOKUP(1,A:A,A:A)");
        }
        save_workbook(&book, &source).unwrap();
        let result = audit(&source, &dir.join("tmp"), None, crate::soffice::DEFAULT_TIMEOUT).unwrap();
        assert_eq!(result["recalculated"], false);
        let issues = result["issues"].as_array().unwrap();
        assert_eq!(issues.len(), 1, "XLOOKUP пойман статикой: {issues:?}");
        assert_eq!(issues[0]["kind"], "forbidden_function");
    }
}
