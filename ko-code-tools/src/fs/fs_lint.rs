//! Хук проверки качества: baseline снимается ДО записи, после записи гейт
//! проверяет файл и при появлении новых синтаксических ошибок запись
//! откатывается с окружением строк вокруг проблемы.

use std::fs;
use std::path::Path;

use crate::contract::{ToolCtx, ToolError};

/// Сколько строк вокруг проблемы показывать в сообщении об откате.
pub const LINT_CONTEXT_LINES: u32 = 5;

/// Состояние проверки качества на момент записи.
pub(crate) enum LintBaseline {
    /// Гейт не подключён — проверки нет (это объявлено в описании тула).
    NoGate,
    /// Снимок взят — проверка будет выполнена после записи.
    Checked(Vec<crate::contract::LintProblem>),
    /// Проверка недоступна (сервера нет или сбой) — запись состоится, но агент
    /// увидит предупреждение: непроверенный код не должен выглядеть проверенным.
    Unavailable(String),
}

/// Снять baseline проверки качества ДО записи.
pub(crate) fn lint_baseline(ctx: &ToolCtx, path: &Path) -> LintBaseline {
    let Some(gate) = ctx.lint_gate() else {
        return LintBaseline::NoGate;
    };
    match gate.snapshot(path) {
        Ok(Some(baseline)) => LintBaseline::Checked(baseline),
        Ok(None) => {
            log::warn!(
                "lint-gate: {} — проверка качества не выполнена (сервер проверки недоступен)",
                path.display()
            );
            LintBaseline::Unavailable("сервер проверки недоступен".to_string())
        }
        Err(reason) => {
            log::warn!("lint-gate: {} — проверка не выполнена: {}", path.display(), reason);
            LintBaseline::Unavailable(reason)
        }
    }
}

/// Проверить изменённый файл хуком качества и при необходимости откатить запись.
///
/// ## Что происходит
///
/// - хук не подключён → ничего не проверяем (это объявлено в описании тула);
/// - хук ответил списком новых ошибок → запись ОТКАТЫВАЕТСЯ, а вызывающий
///   получает ошибку с окружением строк вокруг каждой проблемы;
/// - хук не смог ответить (например, проверка недоступна) → запись
///   остаётся, но сбой проверки попадает и в лог, и в ответ тула: молчать
///   о непроверенном коде нельзя.
pub(crate) fn run_lint_gate(
    ctx: &ToolCtx,
    path: &Path,
    baseline: LintBaseline,
    previous: Option<&str>,
    written: &str,
) -> Result<Option<String>, ToolError> {
    let Some(gate) = ctx.lint_gate() else {
        return Ok(None);
    };
    let baseline = match baseline {
        LintBaseline::NoGate => return Ok(None),
        LintBaseline::Unavailable(reason) => {
            return Ok(Some(format!(
                "⚠️ проверка качества кода не выполнена ({}): {}",
                path.display(),
                reason
            )));
        }
        LintBaseline::Checked(baseline) => baseline,
    };
    match gate.check(path, &baseline) {
        Ok(Some(problems)) => {
            if problems.is_empty() {
                log::debug!(
                    "lint-gate: {} — новых ошибок нет ({} символов)",
                    path.display(),
                    written.chars().count()
                );
                return Ok(None);
            }
            // Откат: возвращаем прежнее содержимое (или убираем новый файл).
            log::info!(
                "lint-gate: {} — новых ошибок {}, запись откатывается",
                path.display(),
                problems.len()
            );
            match previous {
                Some(old) => {
                    fs::write(path, old).map_err(|e| {
                        ToolError::Io(format!("не удалось откатить {}: {}", path.display(), e))
                    })?;
                }
                None => {
                    let _ = fs::remove_file(path);
                }
            }
            let detail = describe_lint_problems(written, &problems);
            Err(ToolError::Lint(detail))
        }
        Ok(None) => {
            log::warn!(
                "lint-gate: {} — проверка качества не выполнена (сервер проверки недоступен)",
                path.display()
            );
            Ok(Some(format!(
                "⚠️ проверка качества кода не выполнена ({}): сервер проверки недоступен",
                path.display()
            )))
        }
        Err(reason) => {
            log::warn!(
                "lint-gate: {} — проверка не выполнена: {}",
                path.display(),
                reason
            );
            Ok(Some(format!(
                "⚠️ проверка качества кода не выполнена ({}): {}",
                path.display(),
                reason
            )))
        }
    }
}

/// Описать проблемы с окружением строк вокруг каждой.
fn describe_lint_problems(content: &str, problems: &[crate::contract::LintProblem]) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut out = String::new();
    for (index, problem) in problems.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        out.push_str(&format!(
            "строка {}: {}\n",
            problem.line, problem.message
        ));
        let center = problem.line.max(1) as usize;
        let start = center.saturating_sub(LINT_CONTEXT_LINES as usize).max(1);
        let end = (center + LINT_CONTEXT_LINES as usize).min(lines.len().max(1));
        for number in start..=end {
            if number > lines.len() {
                break;
            }
            out.push_str(&format!("{:>5}→ {}\n", number, lines[number - 1]));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::CodeTool;
    use crate::contract::ToolCtx;
    use crate::contract::test_util::{auth_for, tmpdir};
    use crate::fs::{EditFile, WriteFile};
    use std::fs;
    use std::path::Path;

    // ── Линт-гейт: правка, сломавшая код, откатывается ──

    /// Тестовый хук: возвращает заранее заданный ответ.
    struct StubGate {
        problems: Vec<crate::contract::LintProblem>,
        fail: Option<String>,
    }

    impl crate::contract::LintGate for StubGate {
        fn snapshot(
            &self,
            _path: &Path,
        ) -> Result<Option<Vec<crate::contract::LintProblem>>, String> {
            match &self.fail {
                Some(reason) => Err(reason.clone()),
                None => Ok(Some(Vec::new())),
            }
        }
        fn check(
            &self,
            _path: &Path,
            _baseline: &[crate::contract::LintProblem],
        ) -> Result<Option<Vec<crate::contract::LintProblem>>, String> {
            match &self.fail {
                Some(reason) => Err(reason.clone()),
                None => Ok(Some(self.problems.clone())),
            }
        }
    }

    /// ГЕЙТ Г4: новая синтаксическая ошибка → запись откатывается, ошибка
    /// содержит окружение строк.
    #[test]
    fn lint_gate_rolls_back_a_broken_write() {
        let d = tmpdir("fs", "lint_rollback");
        fs::write(d.join("a.rs"), "fn ok() {}\n").unwrap();
        let gate = StubGate {
            problems: vec![crate::contract::LintProblem {
                line: 1,
                message: "expected `;`".to_string(),
            }],
            fail: None,
        };
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth).with_lint_gate(&gate);
        let err = WriteFile
            .execute(
                &serde_json::json!({"path": "a.rs", "content": "fn broken(\n"}),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, ToolError::Lint(_)), "отказ гейта — свой класс: {err}");
        let text = err.to_string();
        assert!(text.contains("expected `;`"), "сообщение об ошибке: {text}");
        assert!(text.contains("1→ fn broken("), "окружение строк показано: {text}");
        assert_eq!(
            fs::read_to_string(d.join("a.rs")).unwrap(),
            "fn ok() {}\n",
            "запись откатилась к прежнему содержимому"
        );
    }

    /// ГЕЙТ Г4: хук не задан → запись проходит (и это не «тихий пропуск» —
    /// отсутствие гейта объявлено в описании тула).
    #[test]
    fn write_without_lint_gate_succeeds() {
        let d = tmpdir("fs", "lint_absent");
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let r = WriteFile
            .execute(&serde_json::json!({"path": "b.rs", "content": "fn ok() {}\n"}), &ctx)
            .unwrap();
        assert!(r.contains("создан"), "запись состоялась: {r}");
        assert_eq!(fs::read_to_string(d.join("b.rs")).unwrap(), "fn ok() {}\n");
    }

    /// Сбой проверки (сервер не ответил) — запись остаётся, но сбой виден.
    #[test]
    fn lint_gate_failure_keeps_the_write_but_reports_it() {
        let d = tmpdir("fs", "lint_failed");
        let gate = StubGate {
            problems: Vec::new(),
            fail: Some("сервер не запущен".to_string()),
        };
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth).with_lint_gate(&gate);
        let r = WriteFile
            .execute(&serde_json::json!({"path": "c.rs", "content": "fn ok() {}\n"}), &ctx)
            .unwrap();
        assert!(r.contains("не выполнена"), "сбой проверки виден в ответе: {r}");
        assert!(r.contains("сервер не запущен"), "причина сбоя названа: {r}");
        assert_eq!(
            fs::read_to_string(d.join("c.rs")).unwrap(),
            "fn ok() {}\n",
            "запись осталась на месте"
        );
    }

    /// Правка через edit_file тоже проходит через гейт и откатывается.
    #[test]
    fn lint_gate_rolls_back_a_broken_edit() {
        let d = tmpdir("fs", "lint_edit_rollback");
        fs::write(d.join("d.rs"), "fn alpha() {}\n").unwrap();
        let gate = StubGate {
            problems: vec![crate::contract::LintProblem {
                line: 1,
                message: "unexpected token".to_string(),
            }],
            fail: None,
        };
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth).with_lint_gate(&gate);
        let err = EditFile
            .execute(
                &serde_json::json!({
                    "path": "d.rs",
                    "edits": [{"oldText": "fn alpha() {}", "newText": "fn alpha( {"}]
                }),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, ToolError::Lint(_)), "правка отклонена: {err}");
        assert_eq!(
            fs::read_to_string(d.join("d.rs")).unwrap(),
            "fn alpha() {}\n",
            "правка откатилась"
        );
    }
}
