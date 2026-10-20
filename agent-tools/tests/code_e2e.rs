//! Сквозные тесты тула `code`: настоящий бинарь, настоящий JSON-RPC по stdin.
//!
//! Проверяется контракт, о котором договаривается агент:
//! - код возврата процесса (0 — успех, 2 — ошибка ввода, 4 — отказ исполнения);
//! - содержимое stdout/stderr;
//! - что запись ВНЕ корня не происходит и честно объясняет причину;
//! - что MCP-режим отвечает построчным JSON-RPC и не теряет уведомления.
//!
//! Сеть наружу не используется: всё работает по файлам в временной директории.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Каталог с временными файлами теста.
struct Sandbox {
    dir: PathBuf,
}

impl Sandbox {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let dir = std::env::temp_dir().join(format!(
            "agent_tools_code_e2e_{tag}_{}_{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("создать временную директорию");
        Sandbox { dir }
    }

    fn write(&self, name: &str, content: &str) -> PathBuf {
        let path = self.dir.join(name);
        std::fs::write(&path, content).expect("записать файл-фикстуру");
        path
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Результат запуска бинаря.
struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl Run {
    fn assert_code(&self, expected: i32) {
        assert_eq!(
            self.code,
            Some(expected),
            "код возврата: stdout='{}' stderr='{}'",
            self.stdout,
            self.stderr
        );
    }
}

/// Запустить бинарь в каталоге песочницы.
fn run_in(dir: &Path, args: &[&str]) -> Run {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-tools"))
        .current_dir(dir)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("бинарь запускается и завершается");
    Run {
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// Запустить бинарь с JSON-RPC на stdin и вернуть строки ответа.
fn mcp_roundtrip(dir: &Path, requests: &[&str]) -> Vec<serde_json::Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agent-tools"))
        .current_dir(dir)
        .args(["code", "mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("запустить MCP-режим");

    {
        let stdin = child.stdin.as_mut().expect("stdin открыт");
        for request in requests {
            stdin.write_all(request.as_bytes()).expect("записать запрос");
            stdin.write_all(b"\n").expect("записать перевод строки");
        }
    }
    // stdin закрывается — сервер обязан завершиться сам.
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("дождаться MCP-сервера");
    assert_eq!(
        output.status.code(),
        Some(0),
        "MCP-сервер обязан завершиться честно: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .unwrap_or_else(|e| panic!("строка ответа — валидный JSON ({e}): {line}"))
        })
        .collect()
}

/// Ответ по id.
fn response(responses: &[serde_json::Value], id: i64) -> serde_json::Value {
    responses
        .iter()
        .find(|value| value["id"] == serde_json::json!(id))
        .unwrap_or_else(|| panic!("нет ответа с id={id}: {responses:?}"))
        .clone()
}

// ─────────────────────────────── чтение/запись ───────────────────────────────

#[test]
fn read_file_prints_numbered_lines() {
    let sandbox = Sandbox::new("read");
    sandbox.write("a.txt", "первая\nвторая\n");

    let run = run_in(
        &sandbox.dir,
        &["code", "call", "read_file", "--args", r#"{"path":"a.txt"}"#],
    );

    run.assert_code(0);
    assert!(
        run.stdout.contains("1→ первая") && run.stdout.contains("2→ вторая"),
        "формат строк 'N→ текст' обязателен: {}",
        run.stdout
    );
}

#[test]
fn write_inside_root_creates_the_file() {
    let sandbox = Sandbox::new("write_ok");

    let run = run_in(
        &sandbox.dir,
        &[
            "code",
            "call",
            "write_file",
            "--args",
            r#"{"path":"new.txt","content":"данные"}"#,
        ],
    );

    run.assert_code(0);
    let written = std::fs::read_to_string(sandbox.path("new.txt")).expect("файл создан");
    assert_eq!(written, "данные", "содержимое записано без искажений");
}

/// ГЕЙТ Г5: запись вне корня обязана быть отказом с причиной, а не тихой записью.
#[test]
fn write_outside_root_is_refused_with_a_reason() {
    let sandbox = Sandbox::new("write_denied");
    let outside = Sandbox::new("write_denied_target");
    let target = outside.path("evil.txt");
    let target_arg = target.to_string_lossy().replace('\\', "\\\\");

    let run = run_in(
        &sandbox.dir,
        &[
            "code",
            "call",
            "write_file",
            "--args",
            &format!(r#"{{"path":"{target_arg}","content":"x"}}"#),
        ],
    );

    run.assert_code(4);
    assert!(
        run.stderr.contains("Запрещено") || run.stderr.contains("запрещена"),
        "причина отказа обязана быть в stderr: {}",
        run.stderr
    );
    assert!(
        !target.exists(),
        "файл вне корня не должен быть создан вообще"
    );
}

#[test]
fn invalid_json_arguments_are_an_input_error() {
    let sandbox = Sandbox::new("bad_json");

    let run = run_in(
        &sandbox.dir,
        &["code", "call", "read_file", "--args", "{не json}"],
    );

    run.assert_code(2);
    assert!(
        run.stderr.contains("не является валидным JSON"),
        "причина ошибки ввода обязана быть названа: {}",
        run.stderr
    );
}

#[test]
fn unknown_tool_is_an_input_error_not_a_silent_noop() {
    let sandbox = Sandbox::new("unknown_tool");

    let run = run_in(&sandbox.dir, &["code", "call", "code_nope"]);

    run.assert_code(2);
    assert!(
        run.stderr.contains("Неизвестный тул"),
        "неизвестный тул назван по имени: {}",
        run.stderr
    );
}

// ──────────────────────────────────── MCP ────────────────────────────────────

#[test]
fn mcp_initialize_reports_protocol_and_server() {
    let sandbox = Sandbox::new("mcp_init");

    let responses = mcp_roundtrip(
        &sandbox.dir,
        &[r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#],
    );

    let answer = response(&responses, 1);
    assert_eq!(answer["result"]["protocolVersion"], serde_json::json!("2024-11-05"));
    assert_eq!(answer["result"]["serverInfo"]["name"], serde_json::json!("code-mcp"));
}

#[test]
fn mcp_tools_list_exposes_the_core_tools_with_schemas() {
    let sandbox = Sandbox::new("mcp_list");

    let responses = mcp_roundtrip(
        &sandbox.dir,
        &[r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#],
    );

    let tools = response(&responses, 1)["result"]["tools"].clone();
    let tools = tools.as_array().expect("tools — массив");
    let names: Vec<String> = tools
        .iter()
        .map(|t| t["name"].as_str().unwrap_or_default().to_string())
        .collect();
    for expected in [
        "read_file",
        "read_many_files",
        "write_file",
        "edit_file",
        "grep",
        "glob",
        "list_directory",
        "bash",
        "run_tests",
    ] {
        assert!(
            names.contains(&expected.to_string()),
            "тул {expected} обязан быть в списке: {names:?}"
        );
    }
    for tool in tools {
        assert_eq!(
            tool["inputSchema"]["type"], serde_json::json!("object"),
            "схема тула {} — объект",
            tool["name"]
        );
        assert!(
            !tool["description"].as_str().unwrap_or_default().is_empty(),
            "у тула {} должно быть описание",
            tool["name"]
        );
    }
}

#[test]
fn mcp_call_returns_content_and_is_error_for_a_miss() {
    let sandbox = Sandbox::new("mcp_call");
    sandbox.write("a.txt", "строка\n");

    let responses = mcp_roundtrip(
        &sandbox.dir,
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"read_file","arguments":{"path":"a.txt"}}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"read_file","arguments":{"path":"нет-такого.txt"}}}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"code_nope","arguments":{}}}"#,
        ],
    );

    let ok = response(&responses, 1);
    let text = ok["result"]["content"][0]["text"].as_str().unwrap_or_default();
    assert!(text.contains("строка"), "прочитанное содержимое доехало: {text}");
    assert!(
        ok["result"].get("isError").is_none(),
        "успешный вызов не помечается ошибкой: {ok}"
    );

    let missing = response(&responses, 2);
    assert_eq!(missing["result"]["isError"], serde_json::json!(true));
    let reason = missing["result"]["content"][0]["text"].as_str().unwrap_or_default();
    assert!(
        reason.contains("Не найдено") || reason.contains("не удается найти"),
        "причина промаха названа: {reason}"
    );

    let unknown = response(&responses, 3);
    assert_eq!(
        unknown["result"]["isError"],
        serde_json::json!(true),
        "неизвестный тул — это ошибка вызова, а не пустой успех: {unknown}"
    );
}

/// Уведомление без `id` не должно порождать ответ: клиент его не ждёт.
#[test]
fn mcp_notification_gets_no_answer() {
    let sandbox = Sandbox::new("mcp_notify");

    let responses = mcp_roundtrip(
        &sandbox.dir,
        &[
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        ],
    );

    assert_eq!(
        responses.len(),
        1,
        "на уведомление отвечать нельзя: {responses:?}"
    );
    assert_eq!(response(&responses, 1)["id"], serde_json::json!(1));
}

/// Мусор в потоке не должен ронять сервер: следующий запрос обязан быть обработан.
#[test]
fn mcp_survives_garbage_line() {
    let sandbox = Sandbox::new("mcp_garbage");

    let responses = mcp_roundtrip(
        &sandbox.dir,
        &[
            "не json вовсе",
            r#"{"jsonrpc":"2.0","id":7,"method":"initialize","params":{}}"#,
        ],
    );

    assert_eq!(response(&responses, 7)["id"], serde_json::json!(7));
}
