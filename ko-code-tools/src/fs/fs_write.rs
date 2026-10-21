//! Запись файла целиком: write_file — создать/перезаписать (внутри корня
//! — авто, вне — запрос подтверждения хоста).

use std::fs;
use std::io::Write;
use std::path::Path;

use serde_json::Value;

use crate::contract::{CodeTool, ToolCtx, ToolError, resolve_path};

use super::arg_str;
use super::fs_lint::{lint_baseline, run_lint_gate};

fn write_diff_summary(path: &Path, old: &str, new: &str) -> String {
    if old.is_empty() {
        format!("✅ Файл создан: {} ({} символов)", path.display(), new.chars().count())
    } else if old == new {
        format!("ℹ️ Файл {} уже содержит это содержимое — запись не требуется.", path.display())
    } else {
        let old_lines = old.lines().count();
        let new_lines = new.lines().count();
        format!(
            "✅ Файл обновлён: {} (строк: {} → {})",
            path.display(),
            old_lines,
            new_lines
        )
    }
}

/// `write_file` — создать/перезаписать файл (внутри корня — авто).
pub struct WriteFile;

impl CodeTool for WriteFile {
    fn name(&self) -> &str {
        "write_file"
    }
    fn description(&self) -> &str {
        "Создать новый файл или перезаписать существующий. path — путь (абсолютный или относительно корня проекта). content — полное содержимое файла. Запись внутри корня разрешена автоматически; запись вне корня — запросит подтверждение пользователя. Если хост подключил проверку качества и после записи появилась новая синтаксическая ошибка — запись откатывается, а ошибка содержит строки вокруг проблемы; без подключённой проверки код остаётся непроверенным. Для точечных правок используй edit_file, а не перезапись целиком."
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Путь к файлу (абсолютный или относительно корня проекта)"},
                "content": {"type": "string", "description": "Полное содержимое файла"}
            },
            "required": ["path", "content"]
        })
    }
    fn is_readonly(&self) -> bool {
        false
    }
    fn execute(&self, args: &Value, ctx: &ToolCtx) -> Result<String, ToolError> {
        let path_str = arg_str(args, "path")?;
        let content = arg_str(args, "content")?;
        let abs = resolve_path(ctx.workspace_root, &path_str);
        ctx.authorize_write(&abs, "write_file")?;
        // Прежнее содержимое нужно для отката: файл мог не существовать —
        // тогда откат это удаление, а не восстановление.
        let previous = fs::read_to_string(&abs).ok();
        let old = previous.clone().unwrap_or_default();
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| ToolError::Io(format!("не удалось создать папку {}: {}", parent.display(), e)))?;
        }
        let bytes = content.as_bytes();
        // Baseline проверки качества снимается ДО записи: «до» и «после»
        // должны сравниваться по одному и тому же состоянию файла.
        let baseline = lint_baseline(ctx, &abs);
        let mut f = fs::File::create(&abs)
            .map_err(|e| ToolError::Io(format!("не удалось открыть {}: {}", abs.display(), e)))?;
        f.write_all(bytes)
            .map_err(|e| ToolError::Io(format!("не удалось записать {}: {}", abs.display(), e)))?;
        let mut message = write_diff_summary(&abs, &old, &content);
        match run_lint_gate(ctx, &abs, baseline, previous.as_deref(), &content) {
            Ok(None) => Ok(message),
            Ok(Some(warning)) => {
                message.push('\n');
                message.push_str(&warning);
                Ok(message)
            }
            Err(error) => Err(error),
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
    fn write_file_creates_and_overwrites() {
        let d = tmpdir("fs", "write");
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let r1 = WriteFile
            .execute(&serde_json::json!({"path": "b.ts", "content": "hello"}), &ctx)
            .unwrap();
        assert!(r1.contains("создан"));
        assert_eq!(fs::read_to_string(d.join("b.ts")).unwrap(), "hello");
        let r2 = WriteFile
            .execute(&serde_json::json!({"path": "b.ts", "content": "world"}), &ctx)
            .unwrap();
        assert!(r2.contains("обновлён"));
        assert_eq!(fs::read_to_string(d.join("b.ts")).unwrap(), "world");
    }

    #[test]
    fn write_outside_root_requires_approval() {
        let d = tmpdir("fs", "write_outside");
        let outside = tmpdir("fs", "outside_test");
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        // Тестовый авторизатор отклоняет всё вне корня → Forbidden.
        let err = WriteFile
            .execute(
                &serde_json::json!({"path": outside.to_string_lossy().to_string(), "content": "x"}),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, ToolError::Forbidden(_)));
    }
}
