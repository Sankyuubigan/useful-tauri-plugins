//! Разбор аргументов правки и нормализация блоков: EditSpec, массив `edits[]`,
//! снятие префиксов номеров строк из вывода read_file.

use serde_json::Value;

use crate::contract::ToolError;

/// Одна замена в мульти-правке `edits[]`.
#[derive(Clone)]
pub(super) struct EditSpec {
    pub(super) old: String,
    pub(super) new: String,
}

/// Разобрать строку блоков `edits` из аргументов тула.
pub(super) fn parse_edits_array(args: &Value) -> Result<Vec<EditSpec>, ToolError> {
    let arr = args
        .get("edits")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ToolError::Usage("параметр 'edits' (массив {oldText,newText}) обязателен".to_string()))?;
    if arr.is_empty() {
        return Err(ToolError::Usage("edits не может быть пустым".to_string()));
    }
    let mut specs = Vec::with_capacity(arr.len());
    for (i, item) in arr.iter().enumerate() {
        let old = item
            .get("oldText")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::Usage(format!("edits[{}].oldText (строка) обязателен", i)))?
            .to_string();
        let new = item
            .get("newText")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if old.is_empty() {
            return Err(ToolError::Usage(format!(
                "edits[{}].oldText не может быть пустым",
                i
            )));
        }
        specs.push(EditSpec { old, new });
    }
    Ok(specs)
}

/// Снять с тела строки префикс номера: `<пробелы><цифры><пробелы>(→|\|)<пробелы>`.
/// Возвращает `None`, если префикс не подходит (строка НЕ выглядит как выдача read_file).
fn match_line_number_prefix(body: &str) -> Option<&str> {
    let mut rest = body;
    // Ведущие пробелы.
    if rest.chars().next().is_some_and(|c| c.is_whitespace()) {
        let idx = rest
            .char_indices()
            .find(|(_, c)| !c.is_whitespace())
            .map(|(i, _)| i)
            .unwrap_or(rest.len());
        rest = &rest[idx..];
    }
    // Цифры номера.
    let digits_end = rest
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit())
        .map(|(i, _)| i)
        .unwrap_or(rest.len());
    if digits_end == 0 {
        return None;
    }
    rest = &rest[digits_end..];
    // Пробелы после номера.
    if rest.chars().next().is_some_and(|c| c.is_whitespace()) {
        let idx = rest
            .char_indices()
            .find(|(_, c)| !c.is_whitespace())
            .map(|(i, _)| i)
            .unwrap_or(rest.len());
        rest = &rest[idx..];
    }
    // Маркер: '→' (minimax) или '|' (наш прежний формат).
    let marker = rest.chars().next()?;
    if marker != '→' && marker != '|' {
        return None;
    }
    rest = &rest[marker.len_utf8()..];
    // Хвостовые пробелы.
    if rest.chars().next().is_some_and(|c| c.is_whitespace()) {
        let idx = rest
            .char_indices()
            .find(|(_, c)| !c.is_whitespace())
            .map(|(i, _)| i)
            .unwrap_or(rest.len());
        rest = &rest[idx..];
    }
    Some(rest)
}

/// Снять префиксы номеров со ВСЕГО блока. Возвращает `None`, если хотя бы одна
/// non-empty строка не-префиксная — тогда блок НЕ трогаем (контент может
/// легитимно выглядеть как пронумерованный).
pub(super) fn strip_line_number_prefixes_from_block(text: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let has_nl = line.ends_with('\n');
        let body = line.strip_suffix('\n').unwrap_or(line);
        if body.trim().is_empty() {
            out.push_str(line);
            continue;
        }
        let stripped = match_line_number_prefix(body)?;
        out.push_str(stripped);
        if has_nl {
            out.push('\n');
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::super::EditFile;
    use crate::contract::CodeTool;
    use crate::contract::test_util::{auth_for, tmpdir};
    use crate::contract::{ToolCtx, ToolError};
    use std::fs;

    #[test]
    fn edit_file_strips_line_number_prefixes_on_retry() {
        let d = tmpdir("fs", "edit_retry");
        fs::write(d.join("r.rs"), "fn foo() {\n    return 1;\n}\n").unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        // oldText скопирован из вывода read_file (формат '     N→').
        let r = EditFile
            .execute(
                &serde_json::json!({"path": "r.rs", "edits": [
                    {"oldText": "    2→     return 1;", "newText": "    2→     return 2;"}
                ]}),
                &ctx,
            )
            .unwrap();
        assert!(r.contains("сняты"), "должен сработать ретрай со strip: {}", r);
        let content = fs::read_to_string(d.join("r.rs")).unwrap();
        assert!(content.contains("return 2;"), "замена должна примениться: {}", content);
        assert!(!content.contains("2→"), "новый текст с префиксом — ненормализован");
    }

    #[test]
    fn edit_file_legacy_prefix_style_also_stripped() {
        let d = tmpdir("fs", "edit_retry_legacy");
        fs::write(d.join("s.rs"), "alpha\nbeta\n").unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let r = EditFile
            .execute(
                &serde_json::json!({"path": "s.rs", "edits": [
                    {"oldText": "   2 | beta", "newText": "   2 | gamma"}
                ]}),
                &ctx,
            )
            .unwrap();
        assert!(r.contains("сняты"));
        assert!(fs::read_to_string(d.join("s.rs")).unwrap().contains("gamma"));
    }

    #[test]
    fn edit_file_does_not_strip_mixed_content() {
        let d = tmpdir("fs", "edit_nostrip");
        fs::write(d.join("t.txt"), "hello world\n").unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        // Одна строка префиксная, другая нет → весь блок НЕ нормализуется.
        let err = EditFile
            .execute(
                &serde_json::json!({"path": "t.txt", "edits": [
                    {"oldText": "hello world\nextra non-prefixed line", "newText": "X"}
                ]}),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
        assert_eq!(fs::read_to_string(d.join("t.txt")).unwrap(), "hello world\n");
    }
}
