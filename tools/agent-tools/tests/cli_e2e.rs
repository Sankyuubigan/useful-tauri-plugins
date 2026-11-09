//! Сквозные тесты CLI: запускаем настоящий бинарь и проверяем контракт с агентом —
//! код возврата, содержимое потоков, состояние и сам HTTP-запрос.
//!
//! Сеть наружу не используется: поднимается стаб на `127.0.0.1:0`, который
//! пишет в лог принятые запросы. Это позволяет утверждать не только «ответ пришёл»,
//! но и «запрос НЕ уходил» (лимит исчерпан / ошибка ввода) — такой тест на живом
//! шлюзе поставить невозможно.
//!
//! Модель в тестах — `google-low-tier`: реально существующее combo локального
//! 9Router. Стаб отвечает на любое имя, но проверяется именно то, которое
//! используется в бою.
//!
//! Один тест помечен `#[ignore]` — живой вызов шлюза (нужен запущенный 9Router,
//! он тратит лимит и токены). Запуск: `cargo test -p agent-tools -- --ignored`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Combo, на котором проверяется инструмент в бою.
const TEST_MODEL: &str = "google-low-tier";

/// Ключ, который инструмент обязан положить в `Authorization`.
const TEST_API_KEY: &str = "test-key";

// ───────────────────────────── окружение тестов ─────────────────────────────

static SEQ: AtomicU64 = AtomicU64::new(0);

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let dir = std::env::temp_dir().join(format!(
        "agent_tools_e2e_{tag}_{}_{}_{nanos}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).expect("создать временную директорию");
    dir
}

/// Свободный порт, который гарантированно закрыт: слушатель снят до возврата.
fn closed_port_base_url() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("занять порт");
    let port = listener.local_addr().expect("адрес").port();
    drop(listener);
    format!("http://127.0.0.1:{port}/v1")
}

// ──────────────────────────────── стаб-сервер ────────────────────────────────

#[derive(Debug, Clone)]
struct Captured {
    request_line: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Captured {
    fn path(&self) -> &str {
        self.request_line.split_whitespace().nth(1).unwrap_or_default()
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).expect("тело запроса — валидный JSON")
    }
}

struct Stub {
    base_url: String,
    requests: Arc<Mutex<Vec<Captured>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Stub {
    /// Поднять стаб, отвечающий указанным статусом и телом.
    fn start(status_line: &'static str, body: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("стаб: bind");
        let addr = listener.local_addr().expect("стаб: addr");
        listener
            .set_nonblocking(true)
            .expect("стаб: неблокирующий accept");

        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));

        let worker = {
            let requests = Arc::clone(&requests);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            if let Some(captured) = read_request(&mut stream) {
                                requests.lock().expect("список запросов").push(captured);
                            }
                            let response = format!(
                                "{status_line}\r\ncontent-type: application/json\r\n\
                                 content-length: {}\r\nconnection: close\r\n\r\n{body}",
                                body.len()
                            );
                            let _ = stream.write_all(response.as_bytes());
                            let _ = stream.flush();
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(_) => break,
                    }
                }
            })
        };

        Self {
            base_url: format!("http://{addr}/v1"),
            requests,
            stop,
            worker: Some(worker),
        }
    }

    /// Стаб, отвечающий «эксперта дал развёрнутое решение».
    fn expert() -> Self {
        Self::start(
            "HTTP/1.1 200 OK",
            r#"{"choices":[{"message":{"role":"assistant","content":"Замени `unwrap()` на `?`."}}]}"#,
        )
    }

    fn requests(&self) -> Vec<Captured> {
        self.requests.lock().expect("список запросов").clone()
    }

    /// Ни одного запроса быть не должно: ждём, чтобы отрицание не оказалось
    /// ложным из-за гонки «процесс ещё не дошёл до сокета».
    fn assert_no_requests(&self) {
        std::thread::sleep(Duration::from_millis(150));
        let requests = self.requests();
        assert!(
            requests.is_empty(),
            "к шлюзу нельзя было обращаться, но пришло {} запросов: {:?}",
            requests.len(),
            requests.iter().map(Captured::path).collect::<Vec<_>>()
        );
    }
}

impl Drop for Stub {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Option<Captured> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);

    let mut request_line = String::new();
    if reader.read_line(&mut request_line).ok()? == 0 {
        return None;
    }

    let mut headers: Vec<(String, String)> = Vec::new();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            break;
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if name == "content-length" {
                content_length = value.parse().unwrap_or(0);
            }
            headers.push((name, value));
        }
    }

    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body).ok()?;
    }

    Some(Captured {
        request_line,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

// ──────────────────────────────── запуск CLI ────────────────────────────────

struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl Run {
    fn assert_code(&self, expected: i32) -> &Self {
        assert_eq!(
            self.code,
            Some(expected),
            "код возврата неверный.\nstdout: {}\nstderr: {}",
            self.stdout,
            self.stderr
        );
        self
    }
}

struct Cli {
    dir: PathBuf,
    base_url: String,
}

impl Cli {
    fn new(tag: &str, base_url: String) -> Self {
        Self {
            dir: tmp_dir(tag),
            base_url,
        }
    }

    fn state_file(&self) -> PathBuf {
        self.dir.join(".consult_state.json")
    }

    fn write_report(&self, name: &str, text: &str) -> PathBuf {
        let path = self.dir.join(name);
        std::fs::write(&path, text).expect("записать отчёт");
        path
    }

    fn seed_state(&self, calls: u32, limit: u32) {
        let state = serde_json::json!({ "calls": calls, "limit": limit });
        std::fs::write(
            self.state_file(),
            serde_json::to_string_pretty(&state).expect("сериализовать состояние"),
        )
        .expect("подложить состояние");
    }

    fn calls(&self) -> u32 {
        let raw = std::fs::read_to_string(self.state_file()).expect("файл состояния создан");
        serde_json::from_str::<serde_json::Value>(&raw).expect("состояние — JSON")["calls"]
            .as_u64()
            .expect("calls — число") as u32
    }

    fn run(&self, args: &[&str]) -> Run {
        self.run_with_stdin(args, None)
    }

    fn run_with_stdin(&self, args: &[&str], stdin: Option<&str>) -> Run {
        let mut command = Command::new(env!("CARGO_BIN_EXE_agent-tools"));
        command
            .current_dir(&self.dir)
            .args(args)
            .env("ROUTER_BASE_URL", &self.base_url)
            .env("ROUTER_API_KEY", TEST_API_KEY)
            .env("SMART_MODEL_NAME", TEST_MODEL)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            });

        let mut child = command.spawn().expect("запустить бинарь");
        if let Some(text) = stdin {
            child
                .stdin
                .as_mut()
                .expect("stdin открыт")
                .write_all(text.as_bytes())
                .expect("записать в stdin");
        }
        let output = child.wait_with_output().expect("дождаться бинаря");

        Run {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }
}

impl Drop for Cli {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

// ─────────────────────────────────── тесты ───────────────────────────────────

#[test]
fn success_prints_header_and_answer_and_spends_one_call() {
    let stub = Stub::expert();
    let cli = Cli::new("success", stub.base_url.clone());
    let report = cli.write_report("report.md", "Я застрял: borrow конфликтует с push.");

    let run = cli.run(&["consult", "--model", TEST_MODEL, &report.display().to_string()]);

    run.assert_code(0);
    assert!(
        run.stdout
            .contains("[ОТВЕТ ЭКСПЕРТА | Использовано вызовов: 1/5, осталось: 4]"),
        "шапка с расходом лимита обязана быть в stdout:\n{}",
        run.stdout
    );
    assert!(
        run.stdout.contains("Замени `unwrap()` на `?`."),
        "чистый ответ модели обязателен в stdout:\n{}",
        run.stdout
    );
    assert_eq!(cli.calls(), 1, "успешный вызов тратит ровно одну единицу лимита");
}

#[test]
fn request_goes_to_chat_completions_with_right_auth_model_and_messages() {
    let stub = Stub::expert();
    let cli = Cli::new("contract", stub.base_url.clone());
    let report = cli.write_report("report.md", "Отчёт рабочего агента");

    let run = cli.run(&["consult", "--model", TEST_MODEL, &report.display().to_string()]);
    run.assert_code(0);

    let requests = stub.requests();
    assert_eq!(requests.len(), 1, "ровно один запрос к шлюзу");
    let request = &requests[0];

    assert_eq!(request.path(), "/v1/chat/completions");
    assert_eq!(
        request.header("authorization"),
        Some(format!("Bearer {TEST_API_KEY}").as_str()),
        "ключ из ROUTER_API_KEY обязан уйти в Authorization"
    );

    let body = request.json();
    assert_eq!(body["model"], TEST_MODEL);
    assert_eq!(body["stream"], serde_json::json!(false));
    assert_eq!(body["messages"][0]["role"], "system");
    assert!(
        body["messages"][0]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("ведущий эксперт-архитектор"),
        "системный промпт эксперта обязателен"
    );
    assert_eq!(body["messages"][1]["role"], "user");
    assert_eq!(body["messages"][1]["content"], "Отчёт рабочего агента");
}

#[test]
fn base_url_without_v1_prefix_still_hits_chat_completions() {
    let stub = Stub::expert();
    let host = stub.base_url.trim_end_matches("/v1").to_string();
    let cli = Cli::new("base_no_v1", host);
    let report = cli.write_report("report.md", "Отчёт");

    let run = cli.run(&["consult", "--model", TEST_MODEL, &report.display().to_string()]);
    run.assert_code(0);

    assert_eq!(stub.requests()[0].path(), "/v1/chat/completions");
}

#[test]
fn model_can_come_from_environment() {
    let stub = Stub::expert();
    let cli = Cli::new("env_model", stub.base_url.clone());
    let report = cli.write_report("report.md", "Отчёт");

    let run = cli.run(&["consult", &report.display().to_string()]);
    run.assert_code(0);

    // SMART_MODEL_NAME=google-low-tier выставлен в окружении запуска.
    assert_eq!(stub.requests()[0].json()["model"], TEST_MODEL);
}

#[test]
fn report_can_be_piped_through_stdin() {
    let stub = Stub::expert();
    let cli = Cli::new("stdin", stub.base_url.clone());

    let run = cli.run_with_stdin(
        &["consult", "--model", TEST_MODEL],
        Some("Отчёт со stdin: цикл не сходится."),
    );

    run.assert_code(0);
    assert!(run.stdout.contains("Использовано вызовов: 1/5"));
    assert_eq!(stub.requests()[0].json()["messages"][1]["content"], "Отчёт со stdin: цикл не сходится.");
}

#[test]
fn dash_means_stdin() {
    let stub = Stub::expert();
    let cli = Cli::new("dash", stub.base_url.clone());

    let run = cli.run_with_stdin(&["consult", "-"], Some("Отчёт через дефис"));

    run.assert_code(0);
    assert_eq!(stub.requests()[0].json()["messages"][1]["content"], "Отчёт через дефис");
}

#[test]
fn state_file_lands_in_working_directory_by_default() {
    let stub = Stub::expert();
    let cli = Cli::new("default_state", stub.base_url.clone());
    let report = cli.write_report("report.md", "Отчёт");

    let run = cli.run(&["consult", "--model", TEST_MODEL, &report.display().to_string()]);
    run.assert_code(0);

    assert!(
        cli.state_file().is_file(),
        "по умолчанию состояние пишется в ./.consult_state.json"
    );
    assert_eq!(cli.calls(), 1);
}

#[test]
fn exhausted_limit_blocks_the_call_and_tells_the_agent_to_stop() {
    let stub = Stub::expert();
    let cli = Cli::new("limit", stub.base_url.clone());
    let report = cli.write_report("report.md", "Отчёт");
    cli.seed_state(5, 5);

    let run = cli.run(&["consult", "--model", TEST_MODEL, &report.display().to_string()]);

    run.assert_code(1);
    assert!(
        run.stderr.contains("КРИТИЧЕСКАЯ ОСТАНОВКА"),
        "директива остановки обязана быть в stderr:\n{}",
        run.stderr
    );
    assert!(run.stderr.contains("[LIMIT_EXCEEDED]"));
    assert!(run.stderr.contains("Лимит обращений к эксперту исчерпан (5/5)!"));
    assert!(run.stderr.contains("СТРОГО ЗАПРЕЩЕНО"));
    assert!(
        run.stdout.trim().is_empty(),
        "ответ модели при исчерпанном лимите не печатается: {}",
        run.stdout
    );
    assert_eq!(cli.calls(), 5, "отказ не двигает счётчик");
    stub.assert_no_requests();
}

#[test]
fn zero_limit_blocks_every_call() {
    let stub = Stub::expert();
    let cli = Cli::new("zero_limit", stub.base_url.clone());
    let report = cli.write_report("report.md", "Отчёт");
    cli.seed_state(0, 0);

    let run = cli.run(&["consult", "--model", TEST_MODEL, &report.display().to_string()]);

    run.assert_code(1);
    stub.assert_no_requests();
}

#[test]
fn missing_report_file_is_input_error() {
    let stub = Stub::expert();
    let cli = Cli::new("missing", stub.base_url.clone());

    let run = cli.run(&["consult", "--model", TEST_MODEL, "нет-такого-файла.md"]);

    run.assert_code(2);
    assert!(run.stderr.contains("не найден"), "stderr: {}", run.stderr);
    assert!(!cli.state_file().exists(), "ошибка ввода не создаёт состояние");
    stub.assert_no_requests();
}

#[test]
fn empty_report_file_is_input_error() {
    let stub = Stub::expert();
    let cli = Cli::new("empty", stub.base_url.clone());
    let report = cli.write_report("empty.md", "   \n\t  ");

    let run = cli.run(&["consult", "--model", TEST_MODEL, &report.display().to_string()]);

    run.assert_code(2);
    assert!(run.stderr.contains("пуст"), "stderr: {}", run.stderr);
    stub.assert_no_requests();
}

#[test]
fn empty_stdin_is_input_error() {
    let stub = Stub::expert();
    let cli = Cli::new("empty_stdin", stub.base_url.clone());

    let run = cli.run_with_stdin(&["consult", "--model", TEST_MODEL], Some(""));

    run.assert_code(2);
    stub.assert_no_requests();
}

#[test]
fn server_error_is_network_error_and_call_is_refunded() {
    let stub = Stub::start(
        "HTTP/1.1 500 Internal Server Error",
        r#"{"error":{"message":"combo is down"}}"#,
    );
    let cli = Cli::new("http_500", stub.base_url.clone());
    let report = cli.write_report("report.md", "Отчёт");

    let run = cli.run(&["consult", "--model", TEST_MODEL, &report.display().to_string()]);

    run.assert_code(3);
    assert!(run.stderr.contains("500"), "stderr: {}", run.stderr);
    assert_eq!(
        cli.calls(),
        0,
        "неудачный запрос не должен стоить лимита"
    );
}

#[test]
fn unauthorized_from_router_is_network_error_and_call_is_refunded() {
    let stub = Stub::start(
        "HTTP/1.1 401 Unauthorized",
        r#"{"error":{"message":"bad key"}}"#,
    );
    let cli = Cli::new("http_401", stub.base_url.clone());
    let report = cli.write_report("report.md", "Отчёт");

    let run = cli.run(&["consult", "--model", TEST_MODEL, &report.display().to_string()]);

    run.assert_code(3);
    assert_eq!(cli.calls(), 0);
}

#[test]
fn unreachable_router_is_network_error_and_call_is_refunded() {
    let cli = Cli::new("no_router", closed_port_base_url());
    let report = cli.write_report("report.md", "Отчёт");

    let started = Instant::now();
    let run = cli.run(&["consult", "--model", TEST_MODEL, &report.display().to_string()]);

    run.assert_code(3);
    assert_eq!(cli.calls(), 0, "обрыв связи не должен стоить лимита");
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "недоступный шлюз обязан определяться быстро, а не по таймауту 300 с"
    );
}

#[test]
fn malformed_answer_is_network_error_and_call_is_refunded() {
    let stub = Stub::start("HTTP/1.1 200 OK", "не-json в ответе");
    let cli = Cli::new("bad_json", stub.base_url.clone());
    let report = cli.write_report("report.md", "Отчёт");

    let run = cli.run(&["consult", "--model", TEST_MODEL, &report.display().to_string()]);

    run.assert_code(3);
    assert_eq!(cli.calls(), 0);
}

#[test]
fn status_reports_usage_without_touching_the_router() {
    let stub = Stub::expert();
    let cli = Cli::new("status", stub.base_url.clone());

    let run = cli.run(&["consult", "--status"]);

    run.assert_code(0);
    assert_eq!(run.stdout.trim(), "Использовано 0 из 5. Осталось: 5");
    stub.assert_no_requests();
}

#[test]
fn set_limit_then_status_then_reset() {
    let stub = Stub::expert();
    let cli = Cli::new("manage", stub.base_url.clone());
    cli.seed_state(3, 5);

    let set = cli.run(&["consult", "--set-limit", "2"]);
    set.assert_code(0);

    let status = cli.run(&["consult", "--status"]);
    status.assert_code(0);
    assert_eq!(status.stdout.trim(), "Использовано 3 из 2. Осталось: 0");

    let reset = cli.run(&["consult", "--reset"]);
    reset.assert_code(0);

    let status = cli.run(&["consult", "--status"]);
    status.assert_code(0);
    assert_eq!(status.stdout.trim(), "Использовано 0 из 2. Осталось: 2");
    stub.assert_no_requests();
}

#[test]
fn management_flag_wins_over_report_argument() {
    let stub = Stub::expert();
    let cli = Cli::new("status_wins", stub.base_url.clone());
    let report = cli.write_report("report.md", "Отчёт");

    let run = cli.run(&["consult", "--status", &report.display().to_string()]);

    run.assert_code(0);
    assert!(run.stdout.contains("Использовано 0 из 5"));
    stub.assert_no_requests();
}

#[test]
fn corrupt_state_is_reported_and_repaired_by_reset() {
    let cli = Cli::new("corrupt", closed_port_base_url());
    std::fs::write(cli.state_file(), "{\"calls\": 3, \"lim").expect("испортить состояние");

    let status = cli.run(&["consult", "--status"]);
    status.assert_code(4);
    assert!(
        status.stderr.contains("--reset"),
        "агент должен получить подсказку, как починить: {}",
        status.stderr
    );

    let reset = cli.run(&["consult", "--reset"]);
    reset.assert_code(0);

    let status = cli.run(&["consult", "--status"]);
    status.assert_code(0);
    assert_eq!(status.stdout.trim(), "Использовано 0 из 5. Осталось: 5");
}

#[test]
fn unknown_flag_is_input_error() {
    let cli = Cli::new("bad_flag", closed_port_base_url());

    let run = cli.run(&["consult", "--несуществующий"]);

    run.assert_code(2);
}

#[test]
fn unknown_tool_is_input_error() {
    let cli = Cli::new("bad_tool", closed_port_base_url());

    let run = cli.run(&["не-инструмент"]);

    run.assert_code(2);
    assert!(run.stderr.contains("Неизвестный инструмент"));
}

#[test]
fn tool_help_is_free_and_does_not_spend_calls() {
    let cli = Cli::new("help", closed_port_base_url());

    let run = cli.run(&["consult", "--help"]);

    run.assert_code(0);
    assert!(run.stdout.contains("--set-limit"));
    assert!(run.stdout.contains("--status"));
    assert!(!cli.state_file().exists());
}

#[test]
fn bare_invocation_prints_tool_list() {
    let cli = Cli::new("bare", closed_port_base_url());

    let run = cli.run(&[]);

    run.assert_code(0);
    assert!(run.stdout.contains("consult"));
}

#[test]
fn custom_state_file_isolates_counters() {
    let stub = Stub::expert();
    let cli = Cli::new("custom_state", stub.base_url.clone());
    let report = cli.write_report("report.md", "Отчёт");
    let custom = cli.dir.join("nested").join("consult.json");

    cli.seed_state(5, 5);
    let run = cli.run(&[
        "consult",
        "--model",
        TEST_MODEL,
        "--state-file",
        &custom.display().to_string(),
        &report.display().to_string(),
    ]);

    run.assert_code(0);
    assert_eq!(
        cli.calls(),
        5,
        "общий счётчик не должен тронуться при отдельном файле состояния"
    );
    assert!(custom.is_file(), "отдельный файл состояния должен быть создан");
}

// ─────────────────────────────── живой шлюз (--ignored) ──────────────────────

/// Сквозной контракт `--no-cache` на настоящем бинаре: карта чужого дерева
/// приходит в stdout, а на диске не появляется ничего — ни кеша, ни правок
/// exclude`.git/info`.
///
/// Режим существует для крейтов из cargo registry и соседних репозиториев:
/// составлять их индекс «рядом с кодом» категорически запрещено.
#[test]
fn codemap_no_cache_maps_a_foreign_folder_and_writes_nothing() {
    let dir = tmp_dir("codemap_no_cache");
    std::fs::create_dir_all(dir.join(".git")).expect("имитация чужого репозитория");

    let source = dir.join("src");
    std::fs::create_dir_all(&source).expect("каталог исходников чужого дерева");
    // Кросс-файловая ссылка: карта ранжирует определения по связям МЕЖДУ
    // файлами, и файл, пользующийся только собственными символами, в карту
    // не попадает вовсе (см. rank::rank_definitions_opts) — это поведение
    // флага --no-cache не должно маскировать.
    std::fs::write(
        source.join("lib.rs"),
        "pub fn foreign_value() -> u32 { foreign_inner() }\n\
         pub fn foreign_inner() -> u32 { 7 }\n",
    )
    .expect("исходник чужого дерева");
    std::fs::write(
        source.join("main.rs"),
        "pub fn foreign_entry() -> u32 { foreign_value() }\n",
    )
    .expect("исходник чужого дерева");

    let output = Command::new(env!("CARGO_BIN_EXE_agent-tools"))
        .current_dir(&dir)
        .args(["codemap", "map", "--path", ".", "--no-cache", "--tokens", "512"])
        .env_remove("ROUTER_BASE_URL")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("запустить бинарь");

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert_eq!(
        output.status.code(),
        Some(0),
        "карта чужого дерева провалилась.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("# Карта проекта"),
        "карта обязана прийти в stdout, а не в файл:\n{stdout}"
    );
    assert!(
        stdout.contains("foreign_value"),
        "символ чужого дерева обязан попасть в карту:\n{stdout}"
    );
    assert!(
        !dir.join(".codemap").exists(),
        "в чужом корне обязан не появиться каталог кеша"
    );
    assert!(
        !dir.join(".git/info/exclude").exists(),
        "exclude чужого корня обязан остаться нетронутым"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Сквозной отказ: `index --no-cache` обязан вернуть код ввода и объяснение,
/// а не generic-ошибку разбора аргументов.
#[test]
fn codemap_index_with_no_cache_rejects_with_honest_message() {
    let dir = tmp_dir("codemap_index_reject");

    let output = Command::new(env!("CARGO_BIN_EXE_agent-tools"))
        .current_dir(&dir)
        .args(["codemap", "index", "--no-cache"])
        .env_remove("ROUTER_BASE_URL")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("запустить бинарь");

    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert_eq!(output.status.code(), Some(2), "код обязан быть ошибкой ввода: {stderr}");
    assert!(
        stderr.contains("--no-cache") && stderr.contains("map"),
        "отказ обязан объяснять альтернативу (map/search/callers), {stderr}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}


/// Боевая проверка: настоящий 9Router на `http://localhost:20128/v1` и настоящее
/// combo `google-low-tier`. Требует запущенного шлюза и тратит один вызов.
///
/// Запуск: `cargo test -p agent-tools --test cli_e2e -- --ignored --nocapture`
#[test]
#[ignore = "живой вызов 9Router: нужен запущенный шлюз, тратит лимит и токены"]
fn live_router_answers_on_google_low_tier() {
    let dir = tmp_dir("live");
    let state = dir.join("consult_live.json");
    let report = dir.join("report.md");
    std::fs::write(
        &report,
        "Инструмент agent-tools собран и проверен: код возврата 1 печатает директиву \
остановки при исчерпании лимита. Ответь одним коротким предложением: работает ли \
связь агента с умной моделью через шлюз?",
    )
    .expect("записать отчёт");

    let output = Command::new(env!("CARGO_BIN_EXE_agent-tools"))
        .current_dir(&dir)
        .args(["consult", "--model", TEST_MODEL])
        .arg("--state-file")
        .arg(&state)
        .arg(&report)
        .env_remove("ROUTER_BASE_URL")
        .env_remove("ROUTER_API_KEY")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("запустить бинарь");

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert_eq!(
        output.status.code(),
        Some(0),
        "живой вызов провалился.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("Использовано вызовов: 1/"),
        "в ответе нет шапки расхода лимита:\n{stdout}"
    );
    let answer = stdout
        .split_once("[ОТВЕТ ЭКСПЕРТА |")
        .and_then(|(_, tail)| tail.split_once('\n'))
        .map(|(_, body)| body.trim().to_string())
        .unwrap_or_default();
    assert!(!answer.is_empty(), "модель вернула пустой ответ:\n{stdout}");
    eprintln!("ОТВЕТ МОДЕЛИ:\n{answer}");

    let _ = std::fs::remove_dir_all(&dir);
}