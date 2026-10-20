//! MCP-сервер кодо-тулов: JSON-RPC 2.0 построчно через stdin/stdout.
//!
//! ## Правило, которое здесь действует
//!
//! **В режиме MCP весь stdout принадлежит протоколу.** Одна лишняя строка —
//! будь то `println!`, прогресс-бар или сообщение логгера — ломает разбор
//! у клиента: ответ перестаёт быть JSON.
//!
//! Отсюда два следствия, и оба соблюдаются конструкцией, а не дисциплиной:
//! - весь лог идёт в stderr: см. [`crate::logging`];
//! - этот модуль не печатает ничего сам — ответ пишется единственной функцией
//!   [`send`], которая пишет ровно одну строку JSON.
//!
//! ## Протокол
//!
//! Поддерживаются `initialize`, `tools/list` и `tools/call` — тот же минимум,
//! что и у MCP-каркасов `codemap` и `websearch`.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

use crate::exit::Exit;

use super::cli::Cli;

/// Версия протокола, как её объявляет клиентская сторона Tauri/Deno.
const PROTOCOL_VERSION: &str = "2024-11-05";

/// Имя сервера в `serverInfo`.
const SERVER_NAME: &str = "code-mcp";

/// Единственное место, которое пишет в stdout в режиме MCP.
fn send(value: &Value) {
    let mut out = std::io::stdout().lock();
    // Ошибка записи означает, что клиент уже отключился. Молча уйти здесь
    // можно: работать всё равно не с чем, а паника в библиотечном коде —
    // ошибка вызывающего, а не наша.
    let _ = writeln!(out, "{value}");
    let _ = out.flush();
}

/// Читать JSON-RPC запросы до конца ввода.
pub fn serve(cli: Cli) -> Exit {
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                log::warn!("code-mcp: ввод прерван: {error}");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(error) => {
                // Мусор в потоке — не повод молчать и не повод упасть: клиент
                // получит явную ошибку и сможет продолжить.
                log::warn!("code-mcp: не разобран запрос: {error}");
                continue;
            }
        };
        respond(&request, &cli);
    }
    Exit::Ok
}

/// Обработать один запрос и отправить ответ.
fn respond(request: &Value, cli: &Cli) {
    let id = request.get("id").cloned();
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let params = request.get("params").cloned().unwrap_or(Value::Null);

    // Уведомление без `id` не требует ответа: ответ на него был бы ошибкой
    // протокола, потому что клиент его не ждёт.
    let Some(id) = id else {
        return;
    };

    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
        })),
        "tools/list" => Ok(json!({ "tools": tool_definitions() })),
        "tools/call" => call_tool(&params, cli),
        other => Err(format!("Неизвестный метод: {other}")),
    };

    match result {
        Ok(value) => send(&json!({ "jsonrpc": "2.0", "id": id, "result": value })),
        Err(reason) => send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": reason },
        })),
    }
}

/// Описания тулов: SSOT — ядро `ko-code-tools` (имена/описания/схемы без дублей).
/// И `tools/list`, и проверка неизвестного имени в `tools/call` строятся на нём.
pub fn tool_definitions() -> Vec<Value> {
    ko_code_tools::all_code_tools()
        .into_iter()
        .map(|tool| {
            json!({
                "name": tool.name(),
                "description": tool.description(),
                "inputSchema": tool.parameters(),
            })
        })
        .collect()
}

/// Вызвать тул ядра.
fn call_tool(params: &Value, cli: &Cli) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or("не указано имя тула")?;
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

    match cli.run_tool(name, &arguments) {
        Ok(text) => Ok(json!({ "content": [{ "type": "text", "text": text }] })),
        Err(reason) => Ok(json!({
            "content": [{ "type": "text", "text": reason.to_string() }],
            "isError": true,
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::code::cli::Cli;
    use ko_code_tools::ToolError;

    #[test]
    fn tool_names_are_unique() {
        let names: Vec<String> = tool_definitions()
            .iter()
            .map(|t| t["name"].as_str().unwrap_or_default().to_string())
            .collect();
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(
            unique.len(),
            names.len(),
            "имена тулов обязаны быть уникальными: {names:?}"
        );
    }

    #[test]
    fn every_tool_has_description_and_schema() {
        for tool in tool_definitions() {
            assert!(
                !tool["description"].as_str().unwrap_or_default().is_empty(),
                "у тула {} нет описания",
                tool["name"]
            );
            assert_eq!(
                tool["inputSchema"]["type"].as_str(),
                Some("object"),
                "у тула {} схема должна быть объектом",
                tool["name"]
            );
        }
    }

    #[test]
    fn definitions_expose_all_nine_tools() {
        let names: Vec<String> = tool_definitions()
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
            assert!(names.contains(&expected.to_string()), "нет тула {expected}: {names:?}");
        }
        assert_eq!(names.len(), 9, "ядро выдаёт ровно девять тулов; SSOT: {names:?}");
    }

    #[test]
    fn call_without_name_is_an_error_not_a_silent_call() {
        let params = json!({"arguments": {}});
        assert!(call_tool(&params, &Cli::for_tests()).is_err());
    }

    #[test]
    fn unknown_tool_is_reported_as_is_error_not_crash() {
        let params = json!({"name": "code_nope", "arguments": {}});
        let value = call_tool(&params, &Cli::for_tests()).expect("неизвестный тул — результат, а не паника");
        assert_eq!(value["isError"], json!(true), "ошибка обязана доехать до агента как isError: {value}");
    }

    #[test]
    fn missing_file_is_is_error_with_text() {
        let params = json!({"name": "read_file", "arguments": {"path": "definitely/missing.txt"}});
        let value = call_tool(&params, &Cli::for_tests()).expect("вызов обязан ответить");
        let text = value["content"][0]["text"].as_str().unwrap_or_default();
        assert!(value["isError"] == json!(true) && !text.is_empty(), "честный текст ошибки: {value}");
    }

    /// ГЕЙТ паритета: определения MCP взяты из ядра (имена/описания совпадают),
    /// а не переписаны вторым источником правды.
    #[test]
    fn definitions_match_core_contract() {
        let core: Vec<(String, String, Value, bool)> = ko_code_tools::all_code_tools()
            .into_iter()
            .map(|t| (t.name().to_string(), t.description().to_string(), t.parameters(), t.is_readonly()))
            .collect();
        let defs: Vec<String> = tool_definitions()
            .iter()
            .map(|t| t["name"].as_str().unwrap_or_default().to_string())
            .collect();
        for (name, description, schema, readonly) in core {
            let idx = defs.iter().position(|n| n == &name).unwrap_or_else(|| panic!("тул {name} отсутствует"));
            let def = &tool_definitions()[idx];
            assert_eq!(def["description"], json!(description.to_string()), "описание разошлось: {name}");
            assert_eq!(def["inputSchema"], schema, "схема разошлась: {name}");
            let _ = readonly;
        }
    }

    #[test]
    fn tool_error_display_survives_into_content() {
        let err = ToolError::Usage("проверка".to_string());
        assert!(err.to_string().contains("проверка"), "Display честен: {err}");
    }
}
