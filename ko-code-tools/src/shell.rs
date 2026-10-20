//! Shell-тулы ядра: `bash` (выполнение команд с таймаутом и запретами) и
//! `run_tests` (запуск тестов/сборки по БЕЛОМУ СПИСКУ команд — без плашки).

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::contract::{CodeTool, ToolCtx, ToolError, is_within_root, resolve_path, truncate};
use crate::pruner::{self, Family};

/// Запрещённые операции в bash (правило 1.1: git-мутации без разрешения запрещены;
/// разрушительные команды — запрещены всегда). Проверка по ключевым словам.
const FORBIDDEN_BASH: &[&str] = &[
    "git commit",
    "git push",
    "git pull",
    "git rebase",
    "git merge",
    "git checkout",
    "git reset",
    "git clean",
    "git stash",
    "git revert",
    "git rm",
    "git branch -d",
    "git branch -D",
    "git tag -d",
    "git fetch",
    "rm -rf /",
    "rm -rf ~",
    "del /s /q c:",
    "format c:",
    "rd /s /q c:",
    "shutdown",
    "taskkill",
    "mkfs",
    "dd if=",
    "chmod -R 777 /",
];

fn command_forbidden(command: &str) -> Option<&'static str> {
    let lower = command.to_lowercase();
    FORBIDDEN_BASH.iter().find(|p| lower.contains(**p)).copied()
}

/// Найти исполняемый файл в PATH (на Windows npm/cargo — это npm.cmd и т.п.).
fn resolve_executable(program: &str) -> String {
    if !cfg!(windows) {
        return program.to_string();
    }
    let mut candidates: Vec<String> = vec![program.to_string()];
    let has_ext = Path::new(program).extension().is_some();
    if !has_ext {
        candidates.insert(0, format!("{}.cmd", program));
        candidates.insert(1, format!("{}.exe", program));
        candidates.insert(2, format!("{}.bat", program));
    }
    let paths = std::env::var("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&paths) {
        for c in &candidates {
            let p = dir.join(c);
            if p.is_file() {
                return p.to_string_lossy().to_string();
            }
        }
    }
    program.to_string()
}

/// Запуск команды с таймаутом. Возвращает (stdout+stderr, exit_code).
/// Убивает процесс по истечении таймаута. На Windows npm/cargo — это .cmd/.exe
/// обёртки: резолвим исполняемый файл по PATH.
fn run_command(
    program: &str,
    args: &[&str],
    cwd: &Path,
    timeout: Duration,
) -> Result<(String, i32), ToolError> {
    let resolved = resolve_executable(program);
    let mut cmd = Command::new(&resolved);
    cmd.args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW — без пустых консольных окон
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| ToolError::Io(format!("не удалось запустить {}: {}", resolved, e)))?;

    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ToolError::Timeout(format!(
                        "команда превысила таймаут {} сек",
                        timeout.as_secs()
                    )));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(ToolError::Io(format!("ошибка ожидания процесса: {}", e))),
        }
    };

    let mut output = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        use std::io::Read;
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        output.push_str(&String::from_utf8_lossy(&buf));
    }
    if let Some(mut stderr) = child.stderr.take() {
        use std::io::Read;
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        let stderr = String::from_utf8_lossy(&buf);
        if !stderr.trim().is_empty() {
            output.push_str(&format!("\n[stderr]\n{}", stderr));
        }
    }

    let code = status.code().unwrap_or(-1);
    Ok((output, code))
}

/// `bash` — выполнение shell-команды в корне проекта.
pub struct Bash;

impl CodeTool for Bash {
    fn name(&self) -> &str {
        "bash"
    }
    fn description(&self) -> &str {
        "Выполнить shell-команду в корне проекта (cwd — корень или указанная папка внутри корня). command — команда; timeout_sec — таймаут (по умолчанию 30, максимум 120); cwd — рабочая папка относительно корня. ЗАПРЕЩЕНЫ git-мутации (commit/push/pull/rebase/merge/checkout/reset и т.п.) и разрушительные команды. Результат — stdout+stderr. Запуск в корне авто-разрешён (логируется); вне корня — запросит подтверждение."
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {"type": "string", "description": "Shell-команда для выполнения"},
                "timeout_sec": {"type": "integer", "description": "Таймаут в секундах (по умолчанию 30, максимум 120)"},
                "cwd": {"type": "string", "description": "Рабочая папка относительно корня (по умолчанию корень)"}
            },
            "required": ["command"]
        })
    }
    fn is_readonly(&self) -> bool {
        false
    }
    fn execute(&self, args: &Value, ctx: &ToolCtx) -> Result<String, ToolError> {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::Usage("параметр 'command' (строка) обязателен".to_string()))?;
        if command.trim().is_empty() {
            return Err(ToolError::Usage("command пуст".to_string()));
        }
        if let Some(bad) = command_forbidden(command) {
            return Err(ToolError::Forbidden(format!(
                "команда содержит запрещённую операцию: {}",
                bad
            )));
        }
        let timeout = Duration::from_secs(
            args.get("timeout_sec")
                .and_then(|v| v.as_u64())
                .unwrap_or(30)
                .clamp(1, 120),
        );
        let cwd = args
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(|p| resolve_path(ctx.workspace_root, p))
            .unwrap_or_else(|| ctx.workspace_root.to_path_buf());
        // bash — исполнитель кода в рабочем воркспейсе: разрешение проверяем по
        // `workspace_root`, а НЕ по узкой файловой зоне записи. Файловые мутаторы
        // (write_file/edit_file) строго ограничены авторизатором записи хоста.
        if !is_within_root(ctx.workspace_root, &cwd) {
            return Err(ToolError::Forbidden(format!(
                "запуск bash вне рабочего воркспейса '{}' запрещён (cwd: {})",
                ctx.workspace_root.display(),
                cwd.display()
            )));
        }

        let program = if cfg!(target_os = "windows") {
            "cmd"
        } else {
            "sh"
        };
        let args: Vec<&str> = if cfg!(target_os = "windows") {
            vec!["/C", command]
        } else {
            vec!["-c", command]
        };
        let (output, code) = run_command(program, &args, &cwd, timeout)?;
        let status = if code == 0 { "✅" } else { "❌" };
        let out = if output.trim().is_empty() {
            format!("{} Команда завершилась с кодом {}", status, code)
        } else {
            format!("{} Команда завершилась с кодом {}\n{}", status, code, truncate(&output, 12000))
        };
        Ok(out)
    }
}

/// Белый список команд `run_tests`. Имена — ключи, значение — (программа, аргументы).
fn test_commands() -> Vec<(&'static str, &'static str, &'static [&'static str])> {
    vec![
        ("npm_test", "npm", &["test"]),
        ("npm_build", "npm", &["run", "build"]),
        ("npm_lint", "npm", &["run", "lint"]),
        ("npm_typecheck", "npm", &["run", "typecheck"]),
        ("cargo_test", "cargo", &["test"]),
        ("cargo_check", "cargo", &["check"]),
    ]
}

/// Семейство команды для чистки вывода: у сборки и линтера PASS-шума нет.
fn family_for(command: &str) -> Family {
    match command {
        "cargo_test" => Family::Cargo,
        "npm_test" => Family::Vitest,
        _ => Family::None,
    }
}

/// `run_tests` — запуск тестов/сборки по белому списку (без подтверждения).
pub struct RunTests;

impl CodeTool for RunTests {
    fn name(&self) -> &str {
        "run_tests"
    }
    fn description(&self) -> &str {
        "Запустить тесты/сборку/линт по БЕЛОМУ СПИСКУ команд (без подтверждения пользователя — это безопасно, команды предопределены и выполняются в корне). command — одно из: npm_test, npm_build, npm_lint, npm_typecheck, cargo_test, cargo_check. filter — необязательный фильтр тестов (передаётся после команды). timeout_sec — таймаут (по умолчанию 120, максимум 300). Вывод тестов очищается от PASS-шума: остаются сводка, имена упавших тестов и их вывод со стектрейсами; огромный блок упавшего теста режется ПОСЕРЕДИНЕ (голова и хвост), потому что стектрейс в конце. Применимо для проверки, что твой код компилируется/тесты проходят."
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {"type": "string", "enum": ["npm_test", "npm_build", "npm_lint", "npm_typecheck", "cargo_test", "cargo_check"], "description": "Команда из белого списка"},
                "filter": {"type": "string", "description": "Необязательный фильтр тестов (например имя теста или файл)"},
                "timeout_sec": {"type": "integer", "description": "Таймаут в секундах (по умолчанию 120, максимум 300)"}
            },
            "required": ["command"]
        })
    }
    fn is_readonly(&self) -> bool {
        false
    }
    fn execute(&self, args: &Value, ctx: &ToolCtx) -> Result<String, ToolError> {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::Usage("параметр 'command' обязателен".to_string()))?;
        let (program, base_args) = test_commands()
            .into_iter()
            .find(|(name, _, _)| name == &command)
            .map(|(_, p, a)| (p, a.to_vec()))
            .ok_or_else(|| ToolError::Usage(format!("неизвестная команда '{}' (см. список в описании)", command)))?;
        let timeout = Duration::from_secs(
            args.get("timeout_sec")
                .and_then(|v| v.as_u64())
                .unwrap_or(120)
                .clamp(1, 300),
        );
        let filter = args.get("filter").and_then(|v| v.as_str()).unwrap_or("");

        let mut full_args: Vec<&str> = base_args.iter().copied().collect();
        if !filter.is_empty() {
            full_args.push("--");
            full_args.push(filter);
        }

        let (output, code) = run_command(program, &full_args, ctx.workspace_root, timeout)?;
        let status = if code == 0 { "✅" } else { "❌" };
        // PASS-шум выкидывается ДО обрезки по длине: иначе обрезка съест хвост,
        // где находится стектрейс упавшего теста.
        let cleaned = pruner::prune(family_for(&command), &output);
        log::info!(
            "run_tests: {} — вывод до {} символов, после чистки {} символов",
            command,
            output.chars().count(),
            cleaned.chars().count()
        );
        let out = if cleaned.trim().is_empty() {
            format!("{} {} завершилась с кодом {}", status, command, code)
        } else {
            format!("{} {} завершилась с кодом {}\n{}", status, command, code, truncate(&cleaned, 16000))
        };
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::test_util::{auth_for, tmpdir};
    use crate::contract::ToolCtx;

    #[test]
    fn bash_echo_works() {
        let d = tmpdir("shell", "bash_echo");
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let r = Bash
            .execute(&serde_json::json!({"command": "echo hello_world"}), &ctx)
            .unwrap();
        assert!(r.contains("hello_world"));
        assert!(r.contains("✅") || r.contains("кодом 0"));
    }

    #[test]
    fn bash_rejects_git_mutation() {
        let d = tmpdir("shell", "bash_git");
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let err = Bash
            .execute(&serde_json::json!({"command": "git commit -m x"}), &ctx)
            .unwrap_err();
        assert!(matches!(err, ToolError::Forbidden(_)));
    }

    #[test]
    fn bash_timeout_kills_long_command() {
        let d = tmpdir("shell", "bash_timeout");
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let cmd = if cfg!(target_os = "windows") {
            "ping -n 10 127.0.0.1 > nul"
        } else {
            "sleep 30"
        };
        let err = Bash
            .execute(&serde_json::json!({"command": cmd, "timeout_sec": 1}), &ctx)
            .unwrap_err();
        assert!(matches!(err, ToolError::Timeout(_)));
    }

    #[test]
    fn run_tests_unknown_command_is_usage() {
        let d = tmpdir("shell", "rt_unknown");
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let err = RunTests
            .execute(&serde_json::json!({"command": "rm -rf"}), &ctx)
            .unwrap_err();
        assert!(matches!(err, ToolError::Usage(_)));
    }

    #[test]
    fn run_tests_npm_build_fails_gracefully_in_empty_dir() {
        let d = tmpdir("shell", "rt_npm");
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        // В пустой папке npm test упадёт с кодом != 0 — но это НЕ падение тула,
        // а результат команды (не код 0 → статус ❌ в выводе).
        let r = RunTests
            .execute(&serde_json::json!({"command": "npm_test", "timeout_sec": 30}), &ctx)
            .unwrap_or_else(|e| format!("ERR:{}", e));
        assert!(!r.starts_with("ERR:"), "run_tests не должен падать: {}", r);
    }
}
