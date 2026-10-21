//! Точечная правка файла: edit_file с тремя попытками поиска фрагмента —
//! точное совпадение, снятие префиксов номеров строк из вывода read_file
//! (модуль parse), нечёткое совпадение (модуль apply; нормализация отступов
//! и пробелов при единственном кандидате).

mod apply;
mod parse;

use self::apply::{apply_edits_fuzzy, apply_edits_sequential, split_byte_range};
use self::parse::{EditSpec, parse_edits_array, strip_line_number_prefixes_from_block};

use std::fs;
use std::io::Write;

use serde_json::Value;

use crate::contract::{CodeTool, ToolCtx, ToolError, resolve_path};
use crate::fuzzy::{self, FuzzyMatch};

use super::arg_str;
use super::fs_lint::{LintBaseline, lint_baseline, run_lint_gate};

/// `edit_file` — точечная правка по точному совпадению фрагмента (exact-match).
pub struct EditFile;

impl CodeTool for EditFile {
    fn name(&self) -> &str {
        "edit_file"
    }
    fn description(&self) -> &str {
        "Точечная замена фрагментов файла. Три попытки найти фрагмент, по порядку: 1) точное совпадение; 2) снятие префиксов номеров строк (если вставил фрагмент из вывода read_file); 3) нечёткое совпадение — отступы и внутренние пробелы нормализуются, кандидат должен быть ровно один, иначе отказ с перечнем кандидатов. Режимы: edits — массив {oldText,newText} (несколько правок за один вызов, атомарно, вырезы НЕ должны перекрываться); old_string/new_string (legacy) + replace_all. Ошибка возвращается как результат (тишина недопустима). Если хост подключил проверку качества и после записи появилась новая синтаксическая ошибка — запись откатывается, а ошибка содержит строки вокруг проблемы; без подключённой проверки код остаётся непроверенным. Запись вне корня — запросит подтверждение."
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Путь к файлу (абсолютный или относительно корня проекта)"},
                "edits": {"type": "array", "items": {"type": "object", "properties": {
                    "oldText": {"type": "string", "description": "Искомый фрагмент (точное совпадение)"},
                    "newText": {"type": "string", "description": "Замена"}
                }, "required": ["oldText", "newText"]}, "description": "Массив правок (рекомендуется)"},
                "old_string": {"type": "string", "description": "Искомый фрагмент (точное совпадение, legacy)"},
                "new_string": {"type": "string", "description": "Замена (legacy)"},
                "replace_all": {"type": "boolean", "description": "Заменить все вхождения (legacy, по умолчанию false — только первое)"}
            },
            "required": ["path"]
        })
    }
    fn is_readonly(&self) -> bool {
        false
    }
    fn execute(&self, args: &Value, ctx: &ToolCtx) -> Result<String, ToolError> {
        let path = resolve_path(ctx.workspace_root, &arg_str(args, "path")?);
        let has_edits = args.get("edits").is_some();
        let has_legacy = args.get("old_string").is_some() || args.get("new_string").is_some();
        if has_edits && has_legacy {
            return Err(ToolError::Usage(
                "укажи либо edits[], либо old_string/new_string — не оба".to_string(),
            ));
        }
        if !has_edits && !has_legacy {
            return Err(ToolError::Usage(
                "нужен либо edits[] (массив {oldText,newText}), либо old_string + new_string".to_string(),
            ));
        }

        ctx.authorize_write(&path, "edit_file")?;
        let content = fs::read_to_string(&path)
            .map_err(|e| ToolError::NotFound(format!("{}: {}", path.display(), e)))?;

        if has_legacy {
            let old = arg_str(args, "old_string")?;
            if old.is_empty() {
                return Err(ToolError::Usage(
                    "old_string не может быть пустым — использовать write_file для создания файла".to_string(),
                ));
            }
            let new = arg_str(args, "new_string")?;
            let replace_all = args.get("replace_all").and_then(|v| v.as_bool()).unwrap_or(false);
            if !content.contains(&old) {
                // Вторая жизнь того же правила: если точного вхождения нет, ищем
                // нечёткое (нормализованные отступы/пробелы). При нескольких
                // кандидатах — честный отказ, а не выбор «на глаз».
                if replace_all {
                    return Err(ToolError::NotFound(format!(
                        "фрагмент не найден в {}: {:?}",
                        path.display(),
                        &old.chars().take(80).collect::<String>()
                    )));
                }
                let candidate = match fuzzy::find(&content, &old) {
                    FuzzyMatch::Unique(candidate) => candidate,
                    FuzzyMatch::Ambiguous(candidates) => {
                        let list: Vec<String> = candidates
                            .iter()
                            .map(|c| format!("строки {}-{}", c.start_line, c.end_line))
                            .collect();
                        return Err(ToolError::NotFound(format!(
                            "нечёткое совпадение неоднозначно в {} — кандидатов {} ({}). \
                             Уточни old_string",
                            path.display(),
                            candidates.len(),
                            list.join(", ")
                        )));
                    }
                    FuzzyMatch::None => {
                        return Err(ToolError::NotFound(format!(
                            "фрагмент не найден в {}: {:?}",
                            path.display(),
                            &old.chars().take(80).collect::<String>()
                        )));
                    }
                };
                // Отступ берётся из СТРОКИ ФАЙЛА, где начинается найденный
                // диапазон: сам диапазон отступ не содержит (он отброшен при
                // нормализации), и отступ первой строки остаётся в файле.
                let candidate_first = content
                    .lines()
                    .nth(candidate.start_line.saturating_sub(1))
                    .unwrap_or_default();
                let pattern_first = old
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or_default();
                let delta = fuzzy::indent_delta(candidate_first, pattern_first);
                let new_text = fuzzy::reindent(&new, delta);
                let (before, after) =
                    split_byte_range(&content, candidate.start_byte, candidate.end_byte);
                let new_content = format!("{before}{new_text}{after}");
                let baseline = lint_baseline(ctx, &path);
                let mut f = fs::File::create(&path)
                    .map_err(|e| ToolError::Io(format!("не удалось открыть {}: {}", path.display(), e)))?;
                f.write_all(new_content.as_bytes())
                    .map_err(|e| ToolError::Io(format!("не удалось записать {}: {}", path.display(), e)))?;
                let mut message = format!(
                    "✅ Замена выполнена в {} (нечёткое совпадение: отступы/пробелы нормализованы; строки {}-{})",
                    path.display(),
                    candidate.start_line,
                    candidate.end_line
                );
                return match run_lint_gate(ctx, &path, baseline, Some(&content), &new_content) {
                    Ok(None) => Ok(message),
                    Ok(Some(warning)) => {
                        message.push('\n');
                        message.push_str(&warning);
                        Ok(message)
                    }
                    Err(error) => Err(error),
                };
            }
            let new_content = if replace_all {
                content.replace(&old, &new)
            } else {
                content.replacen(&old, &new, 1)
            };
            let baseline = lint_baseline(ctx, &path);
            let mut f = fs::File::create(&path)
                .map_err(|e| ToolError::Io(format!("не удалось открыть {}: {}", path.display(), e)))?;
            f.write_all(new_content.as_bytes())
                .map_err(|e| ToolError::Io(format!("не удалось записать {}: {}", path.display(), e)))?;
            let mut message = format!(
                "✅ Замена выполнена в {} ({} → {}; {} вхождений)",
                path.display(),
                old.chars().count(),
                new.chars().count(),
                if replace_all { content.matches(&old).count() } else { 1 }
            );
            return match run_lint_gate(ctx, &path, baseline, Some(&content), &new_content) {
                Ok(None) => Ok(message),
                Ok(Some(warning)) => {
                    message.push('\n');
                    message.push_str(&warning);
                    Ok(message)
                }
                Err(error) => Err(error),
            };
        }

        // Режим edits[]: первая попытка + один fail-only ретрай с префикс-стрипом
        // + третья попытка (нечёткое совпадение).
        let edits = parse_edits_array(args)?;
        let apply = |specs: &[EditSpec]| -> Result<(String, String, LintBaseline), ToolError> {
            let (new_content, summary) =
                apply_edits_sequential(&content, specs).map_err(ToolError::NotFound)?;
            let baseline = lint_baseline(ctx, &path);
            let mut f = fs::File::create(&path)
                .map_err(|e| ToolError::Io(format!("не удалось открыть {}: {}", path.display(), e)))?;
            f.write_all(new_content.as_bytes())
                .map_err(|e| ToolError::Io(format!("не удалось записать {}: {}", path.display(), e)))?;
            Ok((summary, new_content, baseline))
        };

        match apply(&edits) {
            Ok((summary, new_content, baseline)) => {
                let mut message = format!("✅ {} в {}", summary, path.display());
                return match run_lint_gate(ctx, &path, baseline, Some(&content), &new_content) {
                    Ok(None) => Ok(message),
                    Ok(Some(warning)) => {
                        message.push('\n');
                        message.push_str(&warning);
                        Ok(message)
                    }
                    Err(error) => Err(error),
                };
            }
            // Ретрай — только если упало на не-нахождении (NotFound о тексте),
            // а не на реальной IO-ошибке.
            Err(ToolError::NotFound(first_err)) => {
                // Fail-only ретрай: только если КАЖДЫЙ блок нормализуется (префикс-формы).
                let mut normalized = Vec::with_capacity(edits.len());
                let mut changed = false;
                for e in &edits {
                    let old = strip_line_number_prefixes_from_block(&e.old)
                        .unwrap_or_else(|| e.old.clone());
                    let new = strip_line_number_prefixes_from_block(&e.new)
                        .unwrap_or_else(|| e.new.clone());
                    if old != e.old || new != e.new {
                        changed = true;
                    }
                    normalized.push(EditSpec { old, new });
                }
                if changed {
                    match apply(&normalized) {
                        Ok((summary, new_content, baseline)) => {
                            let mut message = format!(
                                "✅ {} в {} (префиксы номеров строк сняты)",
                                summary,
                                path.display()
                            );
                            return match run_lint_gate(ctx, &path, baseline, Some(&content), &new_content) {
                                Ok(None) => Ok(message),
                                Ok(Some(warning)) => {
                                    message.push('\n');
                                    message.push_str(&warning);
                                    Ok(message)
                                }
                                Err(error) => Err(error),
                            };
                        }
                        Err(_) => {
                            // Снятие префиксов не помогло — падаем к нечёткой попытке.
                        }
                    }
                }
                // Попытка 3 (последняя): нечёткое совпадение — нормализация
                // отступов и пробелов, кандидат должен быть ровно один.
                let fuzzy_edits = if changed { normalized } else { edits.clone() };
                match apply_edits_fuzzy(&content, &fuzzy_edits) {
                    Ok((new_content, summary)) => {
                        let baseline = lint_baseline(ctx, &path);
                        let mut f = fs::File::create(&path).map_err(|e| {
                            ToolError::Io(format!("не удалось открыть {}: {}", path.display(), e))
                        })?;
                        f.write_all(new_content.as_bytes()).map_err(|e| {
                            ToolError::Io(format!("не удалось записать {}: {}", path.display(), e))
                        })?;
                        let mut message = format!(
                            "✅ {} в {} (нечёткое совпадение: отступы/пробелы нормализованы)",
                            summary,
                            path.display()
                        );
                        match run_lint_gate(ctx, &path, baseline, Some(&content), &new_content) {
                            Ok(None) => Ok(message),
                            Ok(Some(warning)) => {
                                message.push('\n');
                                message.push_str(&warning);
                                Ok(message)
                            }
                            Err(error) => Err(error),
                        }
                    }
                    Err(failure) => {
                        // Три попытки не помогли. При неоднозначности модель
                        // получает перечень кандидатов (с ней можно работать),
                        // в остальных случаях — исходную ошибку exact: она
                        // указывает РЕАЛЬНЫЙ ожидаемый текст, а не догадку.
                        if failure.ambiguous {
                            return Err(ToolError::NotFound(failure.message));
                        }
                        Err(ToolError::NotFound(first_err))
                    }
                }
            }
            Err(e) => Err(e), // IO/другие ошибки — без бессмысленного ретрая.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::test_util::{auth_for, tmpdir};
    use crate::contract::ToolCtx;
    use std::fs;

    #[test]
    fn edit_file_exact_match_and_error_on_missing() {
        let d = tmpdir("fs", "edit");
        fs::write(d.join("c.rs"), "fn a(){}\nfn b(){}\n").unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let r = EditFile
            .execute(
                &serde_json::json!({"path": "c.rs", "old_string": "fn b(){}", "new_string": "fn b2(){}"}),
                &ctx,
            )
            .unwrap();
        assert!(r.contains("Замена выполнена"));
        let content = fs::read_to_string(d.join("c.rs")).unwrap();
        assert!(content.contains("fn b2(){}"));
        assert!(content.contains("fn a(){}"));
        // Не найдено — НЕ молчим.
        let err = EditFile
            .execute(
                &serde_json::json!({"path": "c.rs", "old_string": "zzz", "new_string": "x"}),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
    }

    #[test]
    fn edit_file_replace_all() {
        let d = tmpdir("fs", "edit_all");
        fs::write(d.join("d.txt"), "aXaXa").unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        EditFile
            .execute(
                &serde_json::json!({"path": "d.txt", "old_string": "X", "new_string": "Y", "replace_all": true}),
                &ctx,
            )
            .unwrap();
        assert_eq!(fs::read_to_string(d.join("d.txt")).unwrap(), "aYaYa");
    }

    #[test]
    fn edit_file_multi_edits_atomic() {
        let d = tmpdir("fs", "edit_multi");
        fs::write(d.join("m.rs"), "fn a(){}\nfn b(){}\nfn c(){}\n").unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let r = EditFile
            .execute(
                &serde_json::json!({"path": "m.rs", "edits": [
                    {"oldText": "fn a(){}", "newText": "fn a2(){}"},
                    {"oldText": "fn c(){}", "newText": "fn c2(){}"}
                ]}),
                &ctx,
            )
            .unwrap();
        assert!(r.contains("Заменено 2"));
        let content = fs::read_to_string(d.join("m.rs")).unwrap();
        assert!(content.contains("fn a2(){}"));
        assert!(content.contains("fn b(){}"));
        assert!(content.contains("fn c2(){}"));
        assert!(!content.contains("fn a(){}"));
    }

    #[test]
    fn edit_file_multi_edits_do_not_partially_write_on_failure() {
        let d = tmpdir("fs", "edit_multi_fail");
        fs::write(d.join("n.rs"), "fn a(){}\nfn b(){}\n").unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let err = EditFile
            .execute(
                &serde_json::json!({"path": "n.rs", "edits": [
                    {"oldText": "fn a(){}", "newText": "fn a2(){}"},
                    {"oldText": "ZZZ_NOPE", "newText": "x"}
                ]}),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
        // Атомарность: первая правка НЕ применена, файл не изменён.
        let content = fs::read_to_string(d.join("n.rs")).unwrap();
        assert!(content.contains("fn a(){}"), "частичная запись при провале!");
        assert!(!content.contains("fn a2(){}"));
    }

    #[test]
    fn edit_file_requires_edits_or_legacy() {
        let d = tmpdir("fs", "edit_require");
        fs::write(d.join("u.txt"), "x").unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let err = EditFile
            .execute(&serde_json::json!({"path": "u.txt"}), &ctx)
            .unwrap_err();
        assert!(matches!(err, ToolError::Usage(_)));
        let err2 = EditFile
            .execute(
                &serde_json::json!({"path": "u.txt", "old_string": "x", "new_string": "y", "edits": []}),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err2, ToolError::Usage(_)));
    }

    #[test]
    fn edit_file_empty_oldText_rejected_in_edits() {
        let d = tmpdir("fs", "edit_empty");
        fs::write(d.join("v.txt"), "x").unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let err = EditFile
            .execute(
                &serde_json::json!({"path": "v.txt", "edits": [{"oldText": "", "newText": "y"}]}),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, ToolError::Usage(_)));
    }
}
