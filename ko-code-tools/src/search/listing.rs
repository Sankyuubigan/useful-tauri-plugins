//! Тулы перечисления файлов: glob (маски), list_directory (дерево).
//! Оба уважают .gitignore через общий обходчик `super::walker`.
//! Read-only: авто-доступ без подтверждения.

use serde_json::Value;

use crate::contract::{CodeTool, ToolCtx, ToolError, resolve_path, truncate};

use super::{DEFAULT_MAX_RESULTS, walker};

/// `glob` — поиск файлов по маске.
pub struct Glob;

impl CodeTool for Glob {
    fn name(&self) -> &str {
        "glob"
    }
    fn description(&self) -> &str {
        "Найти файлы по маске (glob) с учётом .gitignore. pattern — маска, например 'src/**/*.ts' или '*.rs'; path — папка для поиска (по умолчанию корень проекта); max_results — лимит (по умолчанию 200). Возвращает список путей."
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Маска glob (например 'src/**/*.ts')"},
                "path": {"type": "string", "description": "Папка для поиска (по умолчанию корень проекта)"},
                "max_results": {"type": "integer", "description": "Лимит результатов (по умолчанию 200)"}
            },
            "required": ["pattern"]
        })
    }
    fn is_readonly(&self) -> bool {
        true
    }
    fn execute(&self, args: &Value, ctx: &ToolCtx) -> Result<String, ToolError> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::Usage("параметр 'pattern' (строка) обязателен".to_string()))?;
        let max_results = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_MAX_RESULTS as u64)
            .clamp(1, 2000) as usize;
        let base = args
            .get("path")
            .and_then(|v| v.as_str())
            .map(|p| resolve_path(ctx.workspace_root, p))
            .unwrap_or_else(|| ctx.workspace_root.to_path_buf());

        let matcher = globset::Glob::new(pattern)
            .map_err(|e| ToolError::Usage(format!("невалидная маска '{}': {}", pattern, e)))?
            .compile_matcher();

        let mut out = String::new();
        let mut count = 0usize;
        for entry in walker(&base, None) {
            if count >= max_results {
                break;
            }
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            let rel = entry.path().strip_prefix(&base).unwrap_or(entry.path());
            if matcher.is_match(rel) {
                out.push_str(&format!("{}\n", entry.path().display()));
                count += 1;
            }
        }
        if out.is_empty() {
            return Ok(format!("🔍 По маске '{}' файлов не найдено.", pattern));
        }
        Ok(truncate(&out, 16000))
    }
}

/// `list_directory` — дерево папки с учётом .gitignore.
pub struct ListDirectory;

impl CodeTool for ListDirectory {
    fn name(&self) -> &str {
        "list_directory"
    }
    fn description(&self) -> &str {
        "Показать содержимое папки в виде дерева с учётом .gitignore. path — папка (по умолчанию корень проекта); depth — глубина обхода (по умолчанию 2). Чтение доступно по любому пути."
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Папка для показа (по умолчанию корень проекта)"},
                "depth": {"type": "integer", "description": "Глубина обхода (по умолчанию 2)"}
            },
            "required": []
        })
    }
    fn is_readonly(&self) -> bool {
        true
    }
    fn execute(&self, args: &Value, ctx: &ToolCtx) -> Result<String, ToolError> {
        let base = args
            .get("path")
            .and_then(|v| v.as_str())
            .map(|p| resolve_path(ctx.workspace_root, p))
            .unwrap_or_else(|| ctx.workspace_root.to_path_buf());
        let depth = args
            .get("depth")
            .and_then(|v| v.as_u64())
            .unwrap_or(2)
            .clamp(0, 6) as usize;

        if !base.is_dir() {
            return Err(ToolError::NotFound(format!("папка не найдена: {}", base.display())));
        }

        let mut out = format!("📁 {}\n", base.display());
        for entry in walker(&base, Some(depth)) {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let rel = entry.path().strip_prefix(&base).unwrap_or(entry.path());
            let parts: Vec<_> = rel.components().collect();
            let indent = "  ".repeat(parts.len().saturating_sub(1));
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let mark = if is_dir { "📁" } else { "📄" };
            out.push_str(&format!("{}{} {}\n", indent, mark, name));
        }
        Ok(truncate(&out, 16000))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::test_util::{auth_for, tmpdir};
    use crate::contract::ToolCtx;
    use std::fs;

    #[test]
    fn glob_matches_patterns() {
        let d = tmpdir("search", "glob");
        fs::create_dir_all(d.join("src").join("deep")).unwrap();
        fs::write(d.join("src/a.ts"), "").unwrap();
        fs::write(d.join("src/deep/b.ts"), "").unwrap();
        fs::write(d.join("src/c.js"), "").unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let r = Glob
            .execute(&serde_json::json!({"pattern": "**/*.ts"}), &ctx)
            .unwrap();
        assert!(r.contains("a.ts"));
        assert!(r.contains("b.ts"));
        assert!(!r.contains("c.js"));
    }

    #[test]
    fn list_directory_shows_tree() {
        let d = tmpdir("search", "list");
        fs::create_dir_all(d.join("sub")).unwrap();
        fs::write(d.join("top.txt"), "").unwrap();
        fs::write(d.join("sub/nested.txt"), "").unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let r = ListDirectory
            .execute(&serde_json::json!({"depth": 2}), &ctx)
            .unwrap();
        assert!(r.contains("top.txt"));
        assert!(r.contains("sub"));
    }
}
