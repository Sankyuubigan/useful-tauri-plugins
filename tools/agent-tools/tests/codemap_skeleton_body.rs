//! Гейт Г4 (поведение фикстур) для операций разведки: `skeleton` и `body`.
//!
//! Проверяется на СИНТЕТИЧЕСКИХ фикстурах трёх языков (`test/fixtures/`):
//! - скелет содержит все сигнатуры с верными номерами строк;
//! - тело символа — точное (строки объявления до следующего объявления);
//! - ненайденный символ даёт честную ошибку с кодом 2, а не пустую строку;
//! - то же самое работает в MCP-режиме (JSON-RPC).

use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Каталог фикстур.
fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test").join("fixtures")
}

/// Запустить бинарь в каталоге фикстур.
fn run(args: &[&str]) -> (Option<i32>, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-tools"))
        .current_dir(fixtures())
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("бинарь запускается");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// Один запрос к MCP-серверу: ответ — распарсенный JSON.
fn mcp_call(request: &str) -> serde_json::Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agent-tools"))
        .current_dir(fixtures())
        .args(["codemap", "mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("запустить MCP-режим");
    {
        let stdin = child.stdin.as_mut().expect("stdin открыт");
        stdin.write_all(request.as_bytes()).expect("записать запрос");
        stdin.write_all(b"\n").expect("перевод строки");
    }
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("дождаться сервера");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first = stdout.lines().next().expect("ответ сервера");
    serde_json::from_str(first).expect("ответ — валидный JSON")
}

use std::io::Write;

// ─────────────────────────────── скелет ───────────────────────────────

#[test]
fn rust_skeleton_lists_imports_and_signatures_with_true_lines() {
    let (code, stdout, stderr) = run(&["codemap", "skeleton", "sample.rs"]);
    assert_eq!(code, Some(0), "stderr: {stderr}");
    assert!(stdout.contains("use std::fmt;"), "импорт виден: {stdout}");
    assert!(
        stdout.contains("    3 | pub struct Service {"),
        "структура на третьей строке: {stdout}"
    );
    assert!(
        stdout.contains("    8 |     pub fn run(&self, input: u32) -> u32 {"),
        "метод на восьмой строке: {stdout}"
    );
    assert!(
        stdout.contains("   13 | pub fn entry() -> u32 {"),
        "функция на тринадцатой строке: {stdout}"
    );
}

#[test]
fn ts_skeleton_lists_imports_and_signatures_with_true_lines() {
    let (code, stdout, stderr) = run(&["codemap", "skeleton", "sample.ts"]);
    assert_eq!(code, Some(0), "stderr: {stderr}");
    assert!(
        stdout.contains("import { helper } from './helper';"),
        "импорт виден: {stdout}"
    );
    assert!(
        stdout.contains("    7 | export function build(config: Config): string {"),
        "функция на седьмой строке: {stdout}"
    );
}

#[test]
fn python_skeleton_lists_imports_and_signatures_with_true_lines() {
    let (code, stdout, stderr) = run(&["codemap", "skeleton", "sample.py"]);
    assert_eq!(code, Some(0), "stderr: {stderr}");
    assert!(stdout.contains("from os import path"), "импорт виден: {stdout}");
    assert!(
        stdout.contains("    4 | def greet(name):"),
        "функция на четвёртой строке: {stdout}"
    );
    assert!(
        stdout.contains("    8 | class Service:"),
        "класс на восьмой строке: {stdout}"
    );
}

// ──────────────────────────────── тело ────────────────────────────────

#[test]
fn rust_body_returns_the_exact_function_body() {
    let (code, stdout, stderr) = run(&["codemap", "body", "sample.rs", "entry"]);
    assert_eq!(code, Some(0), "stderr: {stderr}");
    assert!(
        stdout.contains("pub fn entry() -> u32 {"),
        "тело начинается с объявления: {stdout}"
    );
    assert!(
        stdout.contains("Service { field: 0 }.run(1)"),
        "тело содержит тело функции: {stdout}"
    );
}

#[test]
fn python_body_returns_the_exact_function_body() {
    let (code, stdout, stderr) = run(&["codemap", "body", "sample.py", "greet"]);
    assert_eq!(code, Some(0), "stderr: {stderr}");
    assert!(stdout.contains("def greet(name):"), "объявление на месте: {stdout}");
    assert!(
        stdout.contains("return f\"hi {name}\""),
        "тело функции на месте: {stdout}"
    );
}

#[test]
fn missing_symbol_is_an_input_error_with_code_two() {
    let (code, stdout, stderr) = run(&["codemap", "body", "sample.rs", "нет_такого"]);
    assert_eq!(code, Some(2), "ненайденный символ — ошибка запроса: {stderr}");
    assert!(
        stdout.is_empty(),
        "пустой успех вместо ошибки недопустим: {stdout}"
    );
    assert!(
        stderr.contains("не найден") && stderr.contains("entry"),
        "причина названа и сопровождена списком символов: {stderr}"
    );
}

#[test]
fn missing_file_is_an_input_error_with_code_two() {
    let (code, stdout, stderr) = run(&["codemap", "skeleton", "нет-такого.rs"]);
    assert_eq!(code, Some(2), "отсутствующий файл — ошибка запроса: {stderr}");
    assert!(stdout.is_empty(), "пустой успех недопустим: {stdout}");
}

// ───────────────────────────────── MCP ─────────────────────────────────

#[test]
fn mcp_skeleton_and_body_answer_the_same_way_as_the_cli() {
    let skeleton = mcp_call(
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"codemap_get_skeleton","arguments":{"file":"sample.rs"}}}"#,
    );
    let text = skeleton["result"]["content"][0]["text"].as_str().unwrap_or_default();
    assert!(
        text.contains("pub struct Service {"),
        "MCP-скилет совпадает с CLI: {text}"
    );

    let body = mcp_call(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"codemap_get_body","arguments":{"file":"sample.rs","symbol":"entry"}}}"#,
    );
    let text = body["result"]["content"][0]["text"].as_str().unwrap_or_default();
    assert!(
        text.contains("Service { field: 0 }.run(1)"),
        "MCP-тело совпадает с CLI: {text}"
    );
}

#[test]
fn mcp_missing_symbol_is_an_error_not_an_empty_success() {
    let answer = mcp_call(
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"codemap_get_body","arguments":{"file":"sample.rs","symbol":"нет_такого"}}}"#,
    );
    assert_eq!(answer["result"]["isError"], serde_json::json!(true));
    let text = answer["result"]["content"][0]["text"].as_str().unwrap_or_default();
    assert!(text.contains("не найден"), "причина названа: {text}");
}

#[test]
fn mcp_tool_list_exposes_the_new_tools() {
    let answer = mcp_call(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#);
    let tools = answer["result"]["tools"].as_array().expect("tools — массив");
    let names: Vec<String> = tools
        .iter()
        .map(|t| t["name"].as_str().unwrap_or_default().to_string())
        .collect();
    assert!(names.contains(&"codemap_get_skeleton".to_string()), "{names:?}");
    assert!(names.contains(&"codemap_get_body".to_string()), "{names:?}");
}
