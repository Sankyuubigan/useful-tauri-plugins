//! Разбор аргументов и операции тул `code`.
//!
//! Операции живут здесь, а не в `main.rs`: CLI и MCP-сервер вызывают один и
//! тот же слой [`Cli`] — источник правды один, вывод не дублируется.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use serde_json::Value;

use crate::exit::Exit;

use super::mcp;
use ko_code_tools::{AllowWithinRoot, ToolCtx, ToolError, all_code_tools};

/// Разбор одной сабкоманды: имя программы + имя тула + аргументы.
#[derive(Debug, Parser)]
#[command(name = "agent-tools", bin_name = "agent-tools")]
pub struct CodeParser {
    #[command(subcommand)]
    pub command: CodeSub,
}

/// Подкоманды `code`.
#[derive(Debug, Subcommand)]
pub enum CodeSub {
    /// Вызвать тул ядра по имени (JSON-аргументы).
    #[command(about = ABOUT)]
    Call(CallArgs),
    /// Работать как MCP-сервер (JSON-RPC через stdin/stdout).
    #[command(about = ABOUT)]
    Mcp(McpArgs),
}

const ABOUT: &str = "Файловые, поисковые и shell-тулы code-агента (SSOT кодо-тулов \
King Orch): read_file/read_many_files/write_file/edit_file/grep/glob/\
list_directory/bash/run_tests. Запись внутри корня авто, вне — запрет.";

#[derive(Debug, Args, Clone, Default)]
pub struct CallArgs {
    /// Корень проекта: база относительных путей чтения и авто-запись (по умолчанию текущий каталог).
    #[arg(long, value_name = "PATH")]
    pub root: Option<PathBuf>,
    /// Имя тула ядра (см. `agent-tools code mcp` → `tools/list`).
    #[arg(value_name = "TOOL")]
    pub tool: String,
    /// JSON-строка аргументов тула (например: --args '{"path": "src/a.rs"}').
    #[arg(long, value_name = "JSON", default_value = "{}")]
    pub args: String,
}

#[derive(Debug, Args, Clone, Default)]
pub struct McpArgs {
    /// Корень проекта (по умолчанию текущий каталог).
    #[arg(long, value_name = "PATH")]
    pub root: Option<PathBuf>,
}

/// Общий рабочий слой CLI и MCP: корень + вызов тул ядра.
#[derive(Debug, Clone)]
pub struct Cli {
    /// Корень проекта.
    pub root: PathBuf,
}

impl Cli {
    /// Настройки для тестов: пустой корень — операции обязаны честно вернуть
    /// ошибку, а не выдумать результат.
    pub fn for_tests() -> Cli {
        Cli { root: PathBuf::from(".") }
    }

    /// Вызвать тул ядра по имени с произвольными JSON-аргументами.
    ///
    /// Политика записи — дефолтная ядерная: запись внутри корня авто,
    /// вне — честный запрет (CLI не дублирует сессионные политики хоста).
    pub fn run_tool(&self, name: &str, args: &Value) -> Result<String, ToolError> {
        let auth = AllowWithinRoot::new(self.root.clone());
        let ctx = ToolCtx::new(&self.root, &auth);
        for tool in all_code_tools() {
            if tool.name() == name {
                return tool.execute(args, &ctx);
            }
        }
        Err(ToolError::Usage(format!("Неизвестный тул '{}'", name)))
    }
}

/// Точка входа тул: разбор аргументов и выбор режима.
pub fn run(argv: Vec<String>) -> Exit {
    let rest: &[String] = argv.get(2..).unwrap_or(&[]);
    // clap требует argv[0] = имя программы (см. разбор `codemap`): без этого
    // подкоманда принимается за имя программы и флаги рушатся.
    let mut parser_argv: Vec<String> = vec!["agent-tools".to_string()];
    parser_argv.extend_from_slice(rest);
    let parsed = match CodeParser::try_parse_from(&parser_argv) {
        Ok(parsed) => parsed,
        Err(error) => {
            // clap печатает справку сам; дублировать её не нужно.
            eprint!("{error}");
            return Exit::Input;
        }
    };

    match parsed.command {
        CodeSub::Call(args) => {
            let cli = Cli { root: root_of(args.root) };
            // Невалидный JSON обязан быть честной ошибкой ввода, а не молчаливым
            // '{}': потерянный по невнимательности путь не должен выглядеть
            // как пустой успешный вызов.
            let parsed_args: Value = match serde_json::from_str(&args.args) {
                Ok(value) => value,
                Err(error) => {
                    eprintln!("code call: --args не является валидным JSON: {error}");
                    return Exit::Input;
                }
            };
            let result = cli.run_tool(&args.tool, &parsed_args);
            finish(result)
        }
        CodeSub::Mcp(args) => {
            let cli = Cli { root: root_of(args.root) };
            mcp::serve(cli)
        }
    }
}

/// Напечатать результат операции и вернуть код процесса.
fn finish(result: Result<String, ToolError>) -> Exit {
    match result {
        Ok(text) => {
            println!("{text}");
            Exit::Ok
        }
        // В CLI это единственный канал: ошибка — тоже результат работы.
        // Usage — ошибка вызова (контракт `Exit::Input`); прочие исходы
        // исполнения (не найдено/запрещено/IO/таймаут) — внутренний отказ.
        Err(ToolError::Usage(reason)) => {
            eprintln!("code: неверные аргументы: {reason}");
            Exit::Input
        }
        Err(reason) => {
            eprintln!("code: {reason}");
            Exit::Internal
        }
    }
}

/// Корень проекта: явный путь либо текущий каталог.
fn root_of(path: Option<PathBuf>) -> PathBuf {
    path.unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn parse(items: &[&str]) -> CodeSub {
        let mut argv: Vec<String> = vec!["agent-tools".to_string()];
        argv.extend(items.iter().map(|item| item.to_string()));
        CodeParser::try_parse_from(&argv).expect("разбор").command
    }

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("agent_tools_code_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn parses_call_with_tool_and_args() {
        match parse(&["call", "read_file", "--args", r#"{"path":"a.rs"}"#]) {
            CodeSub::Call(args) => {
                assert_eq!(args.tool, "read_file");
                assert_eq!(args.args, r#"{"path":"a.rs"}"#);
            }
            other => panic!("ожидался call, получен {other:?}"),
        }
    }

    #[test]
    fn call_defaults_args_to_empty_object_and_root_to_cwd() {
        match parse(&["call", "read_file"]) {
            CodeSub::Call(args) => {
                assert_eq!(args.args, "{}");
                assert_eq!(args.root, None);
            }
            other => panic!("ожидался call, получен {other:?}"),
        }
    }

    #[test]
    fn mcp_subcommand_parses() {
        assert!(matches!(parse(&["mcp"]), CodeSub::Mcp(_)));
    }

    #[test]
    fn misses_required_tool_positionally() {
        let argv: Vec<String> = ["code", "call"].iter().map(|s| s.to_string()).collect();
        assert!(
            CodeParser::try_parse_from(&argv).is_err(),
            "вызов без имени тула — ошибка ввода, а не молчаливое пустое действие"
        );
    }

    #[test]
    fn run_tool_returns_text_for_read_file() {
        let d = tmpdir("read");
        fs::write(d.join("a.txt"), "l1\nl2").unwrap();
        let cli = Cli { root: d.clone() };
        let out = cli
            .run_tool("read_file", &serde_json::json!({"path": "a.txt"}))
            .expect("чтение");
        assert!(out.contains("l1"), "формат 'N→ текст': {out}");
    }

    #[test]
    fn run_tool_usage_for_unknown_tool() {
        let cli = Cli::for_tests();
        let err = cli
            .run_tool("totally_unknown", &serde_json::json!({}))
            .unwrap_err();
        assert!(matches!(err, ToolError::Usage(_)));
    }

    #[test]
    fn run_tool_notfound_when_file_missing() {
        let cli = Cli::for_tests();
        let err = cli
            .run_tool("read_file", &serde_json::json!({"path": "nope.txt"}))
            .unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
    }

    /// Ключевое тестовое свойство CLI: запись ВНЕ корня завершается честным
    /// запретом (AllowWithinRoot), а не молчаливой записью.
    #[test]
    fn run_tool_deny_write_outside_root() {
        let d = tmpdir("deny_outside");
        let other = tmpdir("outside_test");
        let cli = Cli { root: d.clone() };
        let err = cli
            .run_tool(
                "write_file",
                &serde_json::json!({
                    "path": other.join("c.txt").to_string_lossy().to_string(),
                    "content": "x"
                }),
            )
            .unwrap_err();
        assert!(matches!(err, ToolError::Forbidden(_)), "запись вне корня обязана запрещаться: {err}");
    }

    #[test]
    fn exit_mapping_usage_is_input_others_internal() {
        assert_eq!(
            finish(Err(ToolError::Usage("n".to_string()))),
            Exit::Input
        );
        assert_eq!(
            finish(Err(ToolError::NotFound("n".to_string()))),
            Exit::Internal
        );
        assert_eq!(
            finish(Err(ToolError::Forbidden("n".to_string()))),
            Exit::Internal
        );
        assert_eq!(finish(Ok("x".into())), Exit::Ok);
    }
}
