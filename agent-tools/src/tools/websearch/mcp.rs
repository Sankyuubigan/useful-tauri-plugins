//! MCP-режим тулов `websearch` — порт `mcp_base.ts` + секция `createMcpServer`
//! из web_search.ts: JSON-RPC 2.0 построчно через stdin/stdout.
//!
//! Отличия от `codemap/mcp.rs` (принципиальные — потому что контракт TS-сервера):
//! - неизвестные методы НЕ отвечают ошибкой — Deno-сервер просто не отвечает на
//!   форматы вне initialize/tools_*, и клиент King Orch рассчитан ровно на это;
//! - ошибка обработчика ответа приходит НЕ как jsonrpc error, а как
//!   `result: { type: "error", content: [{ type: "text", text }] }` — форма
//!   mcp_base.ts (tools/call catch). `isError`-вариант codemap здесь не годится.
//!
//! При этом правило то же: **весь stdout — протокол**, одно сообщение = одна
//! строка, единственный писатель — [`send`].

use std::io::{BufRead, Write};

use serde_json::{Value, json};

use crate::exit::Exit;

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "web-search-mcp";
/// Версия фиксирована как в TS (`createMcpServer` не выводит её из Cargo).
const SERVER_VERSION: &str = "1.0.0";
const TOOL_NAME: &str = "WebSearch";

fn send(value: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{value}");
    let _ = out.flush();
}

/// Читать запросы до EOF. Каждый async-обработчик прогоняется через общий
/// многопоточный рантайм (см. [`super::runtime`]).
pub(crate) fn serve() -> Exit {
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                eprintln!("[mcp_base] ввод прерван: {error}");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(error) => {
                // Как в mcp_base.ts: мусор в потоке логируется в stderr,
                // цикл продолжается.
                eprintln!("Ошибка парсинга JSON-RPC: {error}");
                continue;
            }
        };
        respond(&request);
    }
    Exit::Ok
}

fn respond(request: &Value) {
    // В TS sendResponse кладёт id в объект и JSON.stringify опускает поле,
    // если оно undefined: запрос без id всё равно получает ответ (без поля id).
    let id = request.get("id").cloned();
    let mut envelope = serde_json::Map::new();
    envelope.insert("jsonrpc".into(), json!("2.0"));
    if let Some(id) = id {
        envelope.insert("id".into(), id);
    }
    let method = request.get("method").and_then(Value::as_str).unwrap_or_default();
    match method {
        "initialize" => {
            envelope.insert(
                "result".into(),
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION },
                }),
            );
            send(&Value::Object(envelope));
        }
        "tools/list" => {
            envelope.insert("result".into(), json!({ "tools": tool_definitions() }));
            send(&Value::Object(envelope));
        }
        "tools/call" => {
            let result = call_tool(&request.get("params").cloned().unwrap_or(json!({})));
            envelope.insert("result".into(), result);
            send(&Value::Object(envelope));
        }
        // Прочее молча игнорируется — контракт mcp_base.ts.
        _ => {}
    }
}

/// Описание тула `WebSearch` — дословный перенос из web_search.ts.
pub(crate) fn tool_definitions() -> Vec<Value> {
    vec![json!({
        "name": TOOL_NAME,
        "description": "Поиск в интернете (без API-ключей). Мульти-движок: wiby, duckduckgo, marginalia, brave, startpage, sogou, juejin, csdn, baidu, bing, exa. По умолчанию — последовательная цепочка фолбэков по надёжности; можно указать engines (массив) для параллельного поиска по конкретным движкам. Поддерживает multi-query: передай queries (массив 1-5 запросов) — каждый выполняется параллельно, результаты дедуплицируются по домену+URL.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Поисковый запрос (один). Альтернатива: queries." },
                "queries": { "type": "array", "items": { "type": "string" }, "description": "Опционально: массив 1-5 запросов для multi-query поиска (параллельно, дедуп по домену+URL). Запросы — на языке пользователя, естественные фразы, без операторов (OR/AND/site: и т.п.)." },
                "engines": { "type": "array", "items": { "type": "string" }, "description": "Опционально: движки (wiby, duckduckgo, marginalia, brave, startpage, sogou, juejin, csdn, baidu, bing, exa). Без него — цепочка фолбэков." },
                "limit": { "type": "number", "description": "Опционально: максимум результатов (по умолчанию 8)" }
            }
        }
    })]
}

/// Выполнить tools/call: форма успеха — `{ content: [...] }`, форма ошибки —
/// `{ type: "error", content: [...] }` (обе — дословно mcp_base.ts).
fn call_tool(params: &Value) -> Value {
    let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
    if name != TOOL_NAME {
        return json!({
            "type": "error",
            "content": [{ "type": "text", "text": format!("Неизвестный инструмент: {name}") }]
        });
    }
    let runtime = super::runtime();
    match runtime.block_on(super::handle(&arguments)) {
        Ok(text) => json!({ "content": [{ "type": "text", "text": text }] }),
        Err(err) => json!({
            "type": "error",
            "content": [{ "type": "text", "text": err.message }]
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_exists_with_full_schema_and_description() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0]["name"], TOOL_NAME);
        assert!(!defs[0]["description"].as_str().unwrap_or_default().is_empty());
        assert_eq!(defs[0]["inputSchema"]["type"], "object");
        for key in ["query", "queries", "engines", "limit"] {
            assert!(
                defs[0]["inputSchema"]["properties"].get(key).is_some(),
                "свойство {key} обязано быть в схеме"
            );
        }
    }

    #[test]
    fn unknown_tool_answers_with_ts_error_shape() {
        let result = call_tool(&json!({ "name": "Nope", "arguments": {} }));
        assert!(result["content"].is_array());
        assert_eq!(result["type"], "error");
        assert_eq!(result["content"][0]["type"], "text");
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("Неизвестный инструмент: Nope"));
    }
}
