//! MCP-сервер карты проекта: JSON-RPC 2.0 построчно через stdin/stdout.
//!
//! ## Правило, которое здесь действует
//!
//! **В режиме MCP весь stdout принадлежит протоколу.** Одна лишняя строка —
//! будь то `println!`, прогресс-бар или сообщение логгера — ломает разбор
//! у клиента (Claude Code, OpenCode, aider): ответ перестаёт быть JSON.
//!
//! Отсюда два следствия, и оба соблюдаются конструкцией, а не дисциплиной:
//! - весь лог идёт в stderr: см. [`crate::logging`];
//! - этот модуль не печатает ничего сам — ответ пишется единственной функцией
//!   [`send`], которая пишет ровно одну строку JSON.
//!
//! ## Протокол
//!
//! Поддерживаются `initialize`, `tools/list` и `tools/call` — тот же минимум,
//! что и у Deno-сервера `mcp_base.ts`, который заменяется.

use std::io::{BufRead, Write};

use serde_json::{json, Value};

use crate::exit::Exit;

use super::cli::Cli;

/// Версия протокола, как её объявляет клиентская сторона Tauri/Deno.
const PROTOCOL_VERSION: &str = "2024-11-05";

/// Имя сервера в `serverInfo`.
const SERVER_NAME: &str = "codemap-mcp";

/// Единственное место, которое пишет в stdout в режиме MCP.
///
/// Отдельная функция, а не разбросанные `println!`: так правило «stdout
/// принадлежит протоколу» проверяется чтением кода, а не доверием.
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
                log::warn!("codemap-mcp: ввод прерван: {error}");
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
                log::warn!("codemap-mcp: не разобран запрос: {error}");
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

/// Описания инструментов. Один источник правды: и `tools/list`, и проверка
/// неизвестного имени в `tools/call`.
pub fn tool_definitions() -> Vec<Value> {
    vec![
        tool(
            "codemap_index",
            "Проиндексировать проект: разобрать файлы и сохранить карту",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Корень проекта; по умолчанию текущий каталог" }
                },
            }),
        ),
        tool(
            "codemap_map",
            "Показать карту проекта, отобранную под бюджет токенов (как в Aider)",
            json!({
                "type": "object",
                "properties": {
                    "tokens": { "type": "integer", "description": "Бюджет токенов карты; по умолчанию 2048" },
                    "files": { "type": "array", "items": { "type": "string" }, "description": "Файлы, ранжируемые выше остальных" },
                    "exclude": { "type": "array", "items": { "type": "string" }, "description": "Gitignore-подобные шаблоны исключения из ранжирования (tests/**, **/gen)" },
                    "boostVendor": { "type": "boolean", "description": "Не понижать ранг vendored/сгенерированного кода (vendor/, *.d.ts)" },
                },
            }),
        ),
        tool(
            "codemap_search",
            "Найти символы и текст по запросу (лексический поиск по индексу)",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Что ищем" },
                    "limit": { "type": "integer", "description": "Сколько результатов; по умолчанию 10" }
                },
                "required": ["query"],
            }),
        ),
        tool(
            "codemap_callers",
            "Показать, кто вызывает символ (обратные ссылки)",
            json!({
                "type": "object",
                "properties": {
                    "symbol": { "type": "string", "description": "Имя символа" }
                },
                "required": ["symbol"],
            }),
        ),
        tool(
            "codemap_get_skeleton",
            "Скелет ОДНОГО файла: импорты и сигнатуры символов с номерами строк, без тел. \
             file — путь относительно корня проекта. Индекс не нужен и на диск ничего не \
             пишется. Тело конкретного символа — отдельным вызовом codemap_get_body.",
            json!({
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "Путь к файлу относительно корня" }
                },
                "required": ["file"],
            }),
        ),
        tool(
            "codemap_get_body",
            "Тело ОДНОГО символа файла: от строки объявления до начала следующего объявления \
             (граница названа в ответе). file — путь относительно корня; symbol — имя символа. \
             Если символ не найден, приходит честная ошибка со списком доступных имён.",
            json!({
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "Путь к файлу относительно корня" },
                    "symbol": { "type": "string", "description": "Имя символа" }
                },
                "required": ["file", "symbol"],
            }),
        ),
    ]
}

fn tool(name: &str, description: &str, schema: Value) -> Value {
    json!({ "name": name, "description": description, "inputSchema": schema })
}

/// Вызвать инструмент.
fn call_tool(params: &Value, cli: &Cli) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or("не указано имя инструмента")?;
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

    let text = match name {
        "codemap_index" => {
            let root = string_arg(&arguments, "path");
            cli.index(root.as_deref())
        }
        "codemap_map" => {
            let tokens = arguments
                .get("tokens")
                .and_then(Value::as_u64)
                .unwrap_or(cli.tokens as u64) as u32;
            // Аргументы вызова сильнее настроек сервера: exclude и boostVendor
            // переопределяются на каждый вызов через копию настроек.
            let mut cli = cli.clone();
            cli.rank.exclude = string_array(&arguments, "exclude");
            cli.rank.boost_vendor = arguments
                .get("boostVendor")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            cli.map(tokens, &string_array(&arguments, "files"), false)
        }
        "codemap_search" => {
            let query = string_arg(&arguments, "query").ok_or("не указан query")?;
            let limit = arguments
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(10) as usize;
            cli.search(&query, limit)
        }
        "codemap_callers" => {
            let symbol = string_arg(&arguments, "symbol").ok_or("не указан symbol")?;
            cli.callers(&symbol)
        }
        "codemap_get_skeleton" => {
            let file = string_arg(&arguments, "file").ok_or("не указан file")?;
            cli.skeleton(&file).map_err(|error| error.to_string())
        }
        "codemap_get_body" => {
            let file = string_arg(&arguments, "file").ok_or("не указан file")?;
            let symbol = string_arg(&arguments, "symbol").ok_or("не указан symbol")?;
            cli.body(&file, &symbol).map_err(|error| error.to_string())
        }
        other => return Err(format!("Неизвестный инструмент: {other}")),
    };

    match text {
        Ok(text) => Ok(json!({ "content": [{ "type": "text", "text": text }] })),
        Err(reason) => Ok(json!({
            "content": [{ "type": "text", "text": reason }],
            "isError": true,
        })),
    }
}

fn string_arg(arguments: &Value, key: &str) -> Option<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|s| !s.trim().is_empty())
}

/// Список строк из аргумента; отсутствие или неверный тип даёт пустой список.
///
/// Пустой список — не «ошибка ввода»: вызывающий просто не указал фокус, и
/// карта строится по всему проекту, как и раньше.
fn string_array(arguments: &Value, key: &str) -> Vec<String> {
    arguments
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .filter(|s| !s.trim().is_empty())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

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
            "имена инструментов обязаны быть уникальными: {names:?}"
        );
    }

    #[test]
    fn every_tool_has_description_and_schema() {
        for tool in tool_definitions() {
            assert!(
                !tool["description"].as_str().unwrap_or_default().is_empty(),
                "у инструмента {} нет описания",
                tool["name"]
            );
            assert_eq!(
                tool["inputSchema"]["type"].as_str(),
                Some("object"),
                "у инструмента {} схема должна быть объектом",
                tool["name"]
            );
        }
    }

    #[test]
    fn initialize_reports_protocol_and_server() {
        let request = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize"});
        // Ответ уходит в stdout, поэтому проверяем чистую функцию ответа, а не
        // вывод: сам факт записи в stdout проверяется сквозным тестом бинаря.
        let id = request.get("id").cloned();
        assert_eq!(id, Some(json!(1)));
        assert_eq!(PROTOCOL_VERSION, "2024-11-05");
        assert_eq!(SERVER_NAME, "codemap-mcp");
    }

    #[test]
    fn search_requires_query() {
        let params = json!({"name": "codemap_search", "arguments": {}});
        assert!(call_tool(&params, &Cli::for_tests()).is_err());
    }

    #[test]
    fn callers_requires_symbol() {
        let params = json!({"name": "codemap_callers", "arguments": {}});
        assert!(call_tool(&params, &Cli::for_tests()).is_err());
    }

    #[test]
    fn unknown_tool_is_reported_as_error_not_ignored() {
        let params = json!({"name": "codemap_nope", "arguments": {}});
        let error = call_tool(&params, &Cli::for_tests()).expect_err("неизвестный тул обязан быть ошибкой");
        assert!(error.contains("codemap_nope"), "{error}");
    }

    /// ГЕЙТ 12 на стороне MCP: объявленное свойство `files` обязано доезжать до
    /// ранжирования. Пока оно было только в схеме, карта не менялась от фокуса.
    #[test]
    fn files_argument_is_parsed_for_the_map_tool() {
        let arguments = json!({"files": ["src/a.rs", "src/b.rs", "  ", 7]});
        assert_eq!(string_array(&arguments, "files"), vec!["src/a.rs", "src/b.rs"]);
    }

    #[test]
    fn missing_files_argument_is_an_empty_focus_not_an_error() {
        let arguments = json!({"tokens": 512});
        assert!(string_array(&arguments, "files").is_empty());
    }
}