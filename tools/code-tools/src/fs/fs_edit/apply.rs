//! Применение правок к тексту: атомарная последовательная замена и третья
//! попытка — нечёткое совпадение по нормализованным отступам/пробелам.

use crate::fuzzy::{self, FuzzyMatch};

use super::parse::EditSpec;

/// Ошибка применения правки: текст для модели + класс отказа.
///
/// Отдельный тип нужен, чтобы отличить «правку применять нельзя» (не найдено
/// или неоднозначно) от ошибок ввода-вывода: у них разные последствия и
/// разные коды возврата.
pub(super) struct EditFailure {
    pub(super) message: String,
    /// Кандидатов было несколько: сообщение нечёткой попытки полезнее
    /// исходной ошибки exact («не найдено»), поэтому оно доходит до модели.
    pub(super) ambiguous: bool,
}

/// Разрезать текст по байтовому диапазону (границы символов) на «до» и «после».
///
/// Диапазон вырезается целиком: замена блока — это и есть смысл нечёткого
/// совпадения (фрагмент найден с другими отступами, заменяем именно его).
pub(super) fn split_byte_range(text: &str, start: usize, end: usize) -> (String, String) {
    let start = start.min(text.len());
    let end = end.clamp(start, text.len());
    (text[..start].to_string(), text[end..].to_string())
}

/// Третья попытка правки: нечёткое совпадение по нормализованным строкам.
///
/// Правило: кандидат должен быть РОВНО ОДИН. Два кандидата — отказ с
/// перечислением, потому что выбор «на глаз» означал бы правку не того места.
pub(super) fn apply_edits_fuzzy(original: &str, edits: &[EditSpec]) -> Result<(String, String), EditFailure> {
    let mut result = String::with_capacity(original.len());
    let mut rest = original.to_string();
    let mut fuzzy_used = 0usize;

    for (index, edit) in edits.iter().enumerate() {
        // Точное совпадение и здесь имеет приоритет: нечёткий путь — запасной.
        if let Some(pos) = rest.find(&edit.old) {
            result.push_str(&rest[..pos]);
            result.push_str(&edit.new);
            rest = rest[pos + edit.old.len()..].to_string();
            continue;
        }
        match fuzzy::find(&rest, &edit.old) {
            FuzzyMatch::Unique(candidate) => {
                // Отступ замены приводится к отступу найденного фрагмента:
                // иначе код вложенности «поедет» на уровень образца.
                let candidate_first = rest
                    .lines()
                    .nth(candidate.start_line.saturating_sub(1))
                    .unwrap_or_default();
                let pattern_first = edit
                    .old
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or_default();
                let delta = fuzzy::indent_delta(candidate_first, pattern_first);
                let new_text = fuzzy::reindent(&edit.new, delta);
                let (before, after) =
                    split_byte_range(&rest, candidate.start_byte, candidate.end_byte);
                result.push_str(&before);
                result.push_str(&new_text);
                rest = after;
                fuzzy_used += 1;
            }
            FuzzyMatch::Ambiguous(candidates) => {
                let list: Vec<String> = candidates
                    .iter()
                    .map(|c| format!("строки {}-{}", c.start_line, c.end_line))
                    .collect();
                return Err(EditFailure {
                    ambiguous: true,
                    message: format!(
                        "правка {}: нечёткое совпадение неоднозначно — кандидатов {} ({}). \
                         Уточни oldText: впиши больше строк или используй точный фрагмент.",
                        index + 1,
                        candidates.len(),
                        list.join(", ")
                    ),
                });
            }
            FuzzyMatch::None => {
                return Err(EditFailure {
                    ambiguous: false,
                    message: format!(
                        "фрагмент не найден (правка {}): {:?}",
                        index + 1,
                        edit.old.chars().take(80).collect::<String>()
                    ),
                });
            }
        }
    }
    result.push_str(&rest);
    Ok((
        result,
        format!(
            "применено {} правок (из них {} — нечётким совпадением по отступам/пробелам)",
            edits.len(),
            fuzzy_used
        ),
    ))
}

/// Атомарная мульти-правка: применения идут по порядку по ОСТАТКУ исходного
/// контента. Любой oldText не найден → ошибка до записи (ничего не пишем).
pub(super) fn apply_edits_sequential(original: &str, edits: &[EditSpec]) -> Result<(String, String), String> {
    let mut result = String::with_capacity(original.len());
    let mut rest = original;
    for (i, e) in edits.iter().enumerate() {
        match rest.find(&e.old) {
            Some(pos) => {
                result.push_str(&rest[..pos]);
                result.push_str(&e.new);
                rest = &rest[pos + e.old.len()..];
            }
            None => {
                let preview: String = e.old.chars().take(80).collect();
                return Err(format!("edits[{}] oldText не найден в файле: {:?}", i, preview));
            }
        }
    }
    result.push_str(rest);
    Ok((result, format!("Заменено {} фрагмента(ов)", edits.len())))
}

#[cfg(test)]
mod tests {
    use super::super::EditFile;
    use crate::contract::CodeTool;
    use crate::contract::test_util::{auth_for, tmpdir};
    use crate::contract::ToolCtx;
    use std::fs;

    // ── Третья попытка правки: нечёткое совпадение (отступы/пробелы) ──

    /// Отступ результата не задваивается: отступ первой строки остаётся в
    /// файле, а вставляемый текст приходит без своего отступа.
    #[test]
    fn fuzzy_edit_does_not_double_the_indent() {
        let d = tmpdir("fs", "fuzzy_indent_once");
        fs::write(
            d.join("e.rs"),
            "fn main() {\n    if true {\n        let value = 1;\n    }\n}\n",
        )
        .unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        EditFile
            .execute(
                &serde_json::json!({
                    "path": "e.rs",
                    "edits": [{
                        "oldText": "let value   =   1;",
                        "newText": "let value = 42;"
                    }]
                }),
                &ctx,
            )
            .unwrap();
        let content = fs::read_to_string(d.join("e.rs")).unwrap();
        assert!(
            content.contains("        let value = 42;"),
            "отступ остался прежним, а не удвоился: {content}"
        );
        assert!(
            !content.contains("                let value"),
            "задвоенного отступа быть не должно: {content}"
        );
    }

    /// ГЕЙТ Г4: точный miss → нечёткий hit (кандидат один) → правка применена.
    #[test]
    fn fuzzy_edit_applies_when_indent_differs() {
        let d = tmpdir("fs", "fuzzy_hit");
        fs::write(
            d.join("a.rs"),
            "fn main() {\n    let value = 1;\n    println!(\"{value}\");\n}\n",
        )
        .unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        // Образец без отступа — в файле он с четырьмя пробелами.
        let r = EditFile
            .execute(
                &serde_json::json!({
                    "path": "a.rs",
                    "edits": [{
                        "oldText": "let value = 1;\nprintln!(\"{value}\");",
                        "newText": "let value = 2;\nprintln!(\"{value}\");"
                    }]
                }),
                &ctx,
            )
            .unwrap();
        assert!(r.contains("нечёткое совпадение"), "отчёт о попытке: {r}");
        let content = fs::read_to_string(d.join("a.rs")).unwrap();
        assert!(content.contains("let value = 2;"), "правка применена: {content}");
    }

    /// ГЕЙТ Г4: два кандидата → честный отказ с перечнем, файл НЕ изменён.
    ///
    /// Образец отличается от файла внутренними пробелами: иначе точный поиск
    /// нашёл бы первое вхождение, и нечёткая попытка просто не включалась бы.
    #[test]
    fn fuzzy_edit_refuses_when_candidates_are_ambiguous() {
        let d = tmpdir("fs", "fuzzy_ambiguous");
        let source = "fn a() {\n        let v   =   1;\n}\nfn b() {\n        let v   =   1;\n}\n";
        fs::write(d.join("b.rs"), source).unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let err = EditFile
            .execute(
                &serde_json::json!({
                    "path": "b.rs",
                    "edits": [{
                        "oldText": "let v = 1;",
                        "newText": "let v = 2;"
                    }]
                }),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, crate::contract::ToolError::NotFound(_)), "неоднозначность — отказ: {err}");
        let text = err.to_string();
        assert!(text.contains("неоднозначно"), "причина названа: {text}");
        assert!(text.contains("кандидатов 2"), "число кандидатов названо: {text}");
        let content = fs::read_to_string(d.join("b.rs")).unwrap();
        assert_eq!(content, source, "файл не тронут");
    }

    /// Отступ замены пересчитывается под найденный фрагмент.
    #[test]
    fn fuzzy_edit_reindents_the_replacement() {
        let d = tmpdir("fs", "fuzzy_reindent");
        fs::write(
            d.join("c.rs"),
            "fn main() {\n    if true {\n        let x   =   1;\n    }\n}\n",
        )
        .unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        EditFile
            .execute(
                &serde_json::json!({
                    "path": "c.rs",
                    "edits": [{
                        "oldText": "let x = 1;",
                        "newText": "let x = 2;\nlet y = 3;"
                    }]
                }),
                &ctx,
            )
            .unwrap();
        let content = fs::read_to_string(d.join("c.rs")).unwrap();
        assert!(
            content.contains("        let x = 2;\n        let y = 3;"),
            "обе строки на отступе найденного фрагмента: {content}"
        );
    }

    /// Нечёткий путь не мешает обычным правкам: exact по-прежнему главный.
    #[test]
    fn exact_match_still_wins_over_fuzzy() {
        let d = tmpdir("fs", "fuzzy_exact_first");
        fs::write(d.join("d.rs"), "fn a() { let v = 1; }\n").unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let r = EditFile
            .execute(
                &serde_json::json!({
                    "path": "d.rs",
                    "edits": [{"oldText": "let v = 1;", "newText": "let v = 9;"}]
                }),
                &ctx,
            )
            .unwrap();
        assert!(!r.contains("нечёткое"), "точное совпадение не должно трактоваться как нечёткое: {r}");
    }
}
