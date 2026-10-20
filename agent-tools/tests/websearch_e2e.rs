//! Гейт Г3 (PRP `tasks/18.10.26 Порт поисковых тулов в agent-tools.md`):
//! e2e тула `websearch` на локальном стабе — реальный бинарь, сеть наружу не ходит.
//!
//! env `AGENT_TOOLS_WEBSEARCH_BASE_URL` перенаправляет все запросы на
//! `TcpListener 127.0.0.1:0` (см. `http.rs::rewrite_for_tests`), а
//! `KING_ORCH_BINS_DIR` изолирует кеш/статистику в отдельной tmp-директории
//! на каждый тест. Стаб различает движки по признакам запроса (проверены
//! по URL каждого движка в `engines/*.rs`):
//! - путь содержит `/html` → страница DDG html-этапа;
//! - у запроса есть `format=json` → Instant Answer API DDG;
//! - путь содержит `/lite` → lite-этап DDG;
//! - иначе → wiby `/?q=…`.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

type Handler = Arc<dyn Fn(&str) -> (u16, String) + Send + Sync>;

/// Wiby-фикстура — те же блоки, что в юнитах `wiby.rs`: запрос-эхо (?q=).
const WIBY_HTML: &str = concat!(
    r#"<blockquote><a class="tlink" href="/pages/1">Старый сайт</a><br><p class="url">oldsite.t</p><p>Быстрый поиск ржаной муки</p></blockquote>"#,
    r#"<blockquote><a class="tlink" href="/pages/2">Второй сайт</a><br><p class="url">other.t</p><p>Дополнительный результат</p></blockquote>"#
);

/// Антибот-страница DDG (wsыймаркиры из `is_anomaly`).
const DDG_ANOMALY_HTML: &str = "<html><body><script>execDeep();</script>anomaly detected</body></html>";

/// Пустой ответ Instant Answer API (без RelatedTopics).
const IA_JSON: &str = "{}";

/// Инициализация стаба; возвращает base-URL. Стаб живёт до конца процесса тестов
/// (accept-цикл), т.е. его можно дёргать из нескольких тестов с разными URL.
fn stub(handler: Handler) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("занять порт");
    let base = format!("http://127.0.0.1:{}", listener.local_addr().expect("адрес").port());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(10)));
            let request = read_http_request(&mut stream);
            if request.path.is_empty() {
                continue;
            }
            let (status, body) = handler(&request.path);
            write_response(&mut stream, status, &body);
            // Connection: close — новая коннекция на каждый запрос.
            let _ = stream.flush();
        }
    });
    base
}

struct HttpRequest {
    path: String,
}

/// Прочитать запрос до \r\n\r\n (тело не нужно — стаб отвечает по пути/маркерам).
fn read_http_request(stream: &mut TcpStream) -> HttpRequest {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        if let Ok(n) = stream.read(&mut chunk) {
            if n == 0 { break; }
            buf.extend_from_slice(&chunk[..n]);
            if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        } else {
            break;
        }
    }
    let head = String::from_utf8_lossy(&buf).into_owned();
    let first_line = head.lines().next().unwrap_or_default().to_string();
    let path = first_line
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .to_string();
    HttpRequest { path }
}

fn write_response(stream: &mut TcpStream, status: u16, body: &str) {
    let phrase = match status {
        200 => "OK",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let head = format!(
        "HTTP/1.1 {status} {phrase}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.as_bytes().len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
}

/// Диспетчер стаба по маркерам запроса (описан в шапке файла).
fn default_handler() -> Handler {
    Arc::new(|path: &str| {
        if path.contains("/html") {
            (200, DDG_ANOMALY_HTML.to_string())
        } else if path.contains("format=json") {
            (200, IA_JSON.to_string())
        } else if path.contains("/lite") {
            (200, IA_JSON.to_string())
        } else {
            (200, WIBY_HTML.to_string())
        }
    })
}

fn always(status: u16, body: &str) -> Handler {
    let body = body.to_string();
    Arc::new(move |_path: &str| (status, body.clone()))
}

static SEQ: Mutex<u64> = Mutex::new(0);

fn tmp_dir() -> PathBuf {
    let mut seq = SEQ.lock().expect("счётчик tmp");
    *seq += 1;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let dir = std::env::temp_dir().join(format!(
        "websearch_e2e_{}_{}_{}",
        std::process::id(),
        seq,
        nanos
    ));
    std::fs::create_dir_all(&dir).expect("создать tmp");
    dir
}

fn run_cli(args: &[&str], base: &str, bins: &PathBuf) -> Output {
    Command::new(env!("CARGO_BIN_EXE_agent-tools"))
        .args(args)
        .env("AGENT_TOOLS_WEBSEARCH_BASE_URL", base)
        .env("KING_ORCH_BINS_DIR", bins)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("бинарь запускается и завершается")
}

// ──────────────────────────────── Г3-кейсы ────────────────────────────────

#[test]
fn rate_limit_reported_with_exact_reason_exit_3() {
    let base = stub(always(429, ""));
    let bins = tmp_dir();
    let out = run_cli(
        &["websearch", "probe-rate-limit", "--engines", "wiby"],
        &base,
        &bins,
    );
    assert_eq!(out.status.code(), Some(3), "429 у всех движков → код 3");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Поиск не дал результатов."));
    // Точная причина в выводе — ручной контракт честности ошибок.
    assert!(
        stderr.contains("wiby: лимит запросов (rate-limit) — попробуй позже"),
        "stdout: '{}' stderr: '{}'",
        String::from_utf8_lossy(&out.stdout),
        stderr
    );
}

#[test]
fn antibot_page_reported_to_agent_exit_3() {
    let base = stub(default_handler());
    let bins = tmp_dir();
    let out = run_cli(
        &["websearch", "probe-antibot", "--engines", "duckduckgo"],
        &base,
        &bins,
    );
    assert_eq!(out.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("DDG html: аномальная страница (anomaly)"),
        "stderr: '{}'",
        stderr
    );
    assert!(stderr.contains("Поиск не дал результатов."));
}

#[test]
fn partial_failures_listed_after_results_exit_zero() {
    // ddg падает (анибут-страница), wiby отдаёт выдачу — итоговый код 0,
    // а в выводе — секция «Движки с ошибками» с точной причиной.
    let base = stub(default_handler());
    let bins = tmp_dir();
    let out = run_cli(
        &["websearch", "probe-partial", "--engines", "duckduckgo,wiby"],
        &base,
        &bins,
    );
    assert_eq!(out.status.code(), Some(0), "частичный успех → код 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("⚠️ Движки с ошибками:"), "stdout: '{}'", stdout);
    assert!(stdout.contains("duckduckgo ("), "stdout: '{}'", stdout);
    assert!(stdout.contains("[1] Старый сайт"), "stdout: '{}'", stdout);
}

#[test]
fn empty_chain_is_honest_network_error() {
    // Все движки отвечают 200 пустышкой: не ошибка ввода, а «не удалось найти».
    let base = stub(always(200, ""));
    let bins = tmp_dir();
    let out = run_cli(
        &["websearch", "probe-empty", "--engines", "wiby"],
        &base,
        &bins,
    );
    assert_eq!(out.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Все движки вернули пусто."), "stderr: '{}'", stderr);
}

// ─────────────────────────────── MCP-режим ────────────────────────────────

#[test]
fn mcp_walkthrough_initialize_list_call_on_stub() {
    let base = stub(default_handler());
    let bins = tmp_dir();
    let mut child = Command::new(env!("CARGO_BIN_EXE_agent-tools"))
        .args(["websearch", "mcp"])
        .env("AGENT_TOOLS_WEBSEARCH_BASE_URL", &base)
        .env("KING_ORCH_BINS_DIR", &bins)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("бинарь запускается");
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        let init = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}
        })
        .to_string();
        let list = serde_json::json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}
        })
        .to_string();
        let call = serde_json::json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": { "name": "WebSearch", "arguments": {
                "query": "probe-mcp", "engines": ["wiby"] } }
        })
        .to_string();
        writeln!(stdin, "{init}\n{list}\n{call}").expect("записать JSON-RPC");
    }
    // EOF на stdin → сервер завершится; ответы уже в stdout.
    let out = child.wait_with_output().expect("дождаться выхода");
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<String> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(String::from)
        .collect();
    assert_eq!(lines.len(), 3, "три ответа, по одной строке каждый");

    let init: serde_json::Value =
        serde_json::from_str(&lines[0]).expect("initialize — JSON");
    assert_eq!(init["id"], 1);
    assert_eq!(init["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(init["result"]["serverInfo"]["name"], "web-search-mcp");

    let list: serde_json::Value =
        serde_json::from_str(&lines[1]).expect("tools/list — JSON");
    assert_eq!(list["id"], 2);
    let tools = &list["result"]["tools"];
    assert_eq!(tools[0]["name"], "WebSearch");
    for key in ["query", "queries", "engines", "limit"] {
        assert!(
            tools[0]["inputSchema"]["properties"].get(key).is_some(),
            "свойство {key} есть в схеме"
        );
    }

    let call: serde_json::Value =
        serde_json::from_str(&lines[2]).expect("tools/call — JSON");
    assert_eq!(call["id"], 3);
    let text = call["result"]["content"][0]["text"].as_str().expect("текст вызова");
    assert!(text.contains("[1] Старый сайт"), "text: '{}'", text);
    assert!(
        call["result"].get("type").is_none(),
        "успех без поля type — форма mcp_base.ts"
    );
}

#[test]
fn mcp_failure_shapes_error_like_ts() {
    let base = stub(always(429, ""));
    let bins = tmp_dir();
    let mut child = Command::new(env!("CARGO_BIN_EXE_agent-tools"))
        .args(["websearch", "mcp"])
        .env("AGENT_TOOLS_WEBSEARCH_BASE_URL", &base)
        .env("KING_ORCH_BINS_DIR", &bins)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("бинарь запускается");
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        let call = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "WebSearch", "arguments": {
                "query": "probe-mcp-fail", "engines": ["wiby"] } }
        })
        .to_string();
        writeln!(stdin, "{call}").expect("записать вызов");
    }
    let out = child.wait_with_output().expect("дождаться выхода");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let resp: serde_json::Value = serde_json::from_str(
        stdout.lines().next().expect("первая строка stdout"),
    )
    .expect("ответ — JSON");
    // Форма ошибки — mcp_base.ts ({ type: "error", content: [...] }),
    // не jsonrpc-error и не isError (codemap-вариант).
    assert_eq!(resp["result"]["type"], "error");
    let text = resp["result"]["content"][0]["text"].as_str().expect("текст ошибки");
    assert!(
        text.contains("wiby: лимит запросов (rate-limit)"),
        "text: '{}'",
        text
    );
}
