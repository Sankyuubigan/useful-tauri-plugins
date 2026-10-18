//! Выключение экрана логина дашборда 9router — по состоянию сервера.
//!
//! ## Контракт (state-check вместо конфиг-флага)
//!
//! После каждого подтверждения, что сервер жив (обе точки вызова
//! `start_server`: ветка «уже запущен» и сразу после успешного `wait_port`),
//! [`enforce_login_state`] опрашивает публичный статус
//! `GET /api/settings/require-login`:
//!
//! - `requireLogin: false` → ничего не делаем (честный no-op);
//! - `requireLogin: true` → ровно одна попытка на процесс сервера: вход с
//!   дефолтным паролем (`POST /api/auth/login {"password": "123456"}`) и
//!   `PATCH /api/settings {"requireLogin": false}`, затем верификация тем же
//!   публичным статусом.
//!
//! Настройки меняются ТОЛЬКО через официальный API самого роутера; прямой
//! правки SQLite нет — тем же путем, каким пользователь отключил бы галочку
//! «требовать логин» в настройках 9router.
//!
//! ## Почему упразднён конфиг-флаг (`was_fresh` → `pending_no_login`)
//!
//! Раньше попытка записывалась в конфиг только «свежей» установке
//! (`installed_version` пуст). Реальные сценарии — «удалил каталог и нажал
//! Установить», «обновился», «достроил поверх» — оставляли
//! `installed_version` заполненной, флаг не ставился, и экран логина
//! переживал переустановку. Решение по конфиг-метаданным не покрывало ни
//! один реальный путь, поэтому заменено проверкой самого сервера: если
//! логин включён и пароль дефолтный — выключаем, откуда ни родилась
//! установка. Поле `pending_no_login` осталось в конфиге только для
//! обратной совместимости и в решениях больше не участвует.
//!
//! ## Почему легален логин с дефолтным паролем
//!
//! Запрос идёт из процесса хоста напрямую на `http://127.0.0.1:<порт>` (env
//! `HOSTNAME=127.0.0.1` — сервер слушает только loopback). Сервер 9router
//! (`custom-server.js`) штампует каждому запросу адрес из TCP-сокета
//! (`x-9r-real-ip`) и per-process секрет (`x-9r-peer-token`), поэтому
//! `dashboardGuard.isLocalRequest` распознаёт наш запрос как локальный, и ветка
//! «403: смените дефолтный пароль перед удалённым доступом» — для удалённых
//! клиентов — не срабатывает. `INITIAL_PASSWORD` не задаётся намеренно: эта
//! ветка защищает дашборд от входа публичным дефолтным паролем извне
//! (CVE-2026-56679), и воспроизводить обход ради «навсегда» между стартами
//! было бы чужой дырой в плагине.
//!
//! ## Уважение к выбору пользователя (не «инвариант на каждый старт»)
//!
//! - Свой пароль (логин отвечает 401): requireLogin не трогаем, повторов нет
//!   в рамках процесса сервера. Поставленный пользователем НЕ-дефолтный
//!   пароль переживает любое количество стартов плагина.
//! - 429 от лимитера логина: повторов в рамках процесса сервера нет —
//!   долбить лимитер поллингом запрещено; повтор возможен не раньше нового
//!   процесса сервера (новый pid).
//! - Дешёвый GET-статус ретраится до успеха; дорогое (логин/PATCH) — ровно
//!   одна попытка на процесс сервера: дедуп по (роутер, pid) в памяти
//!   плагина.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

use crate::commands::RouterId;

/// Дефолтный пароль дашборда 9router. SSOT: литерал больше нигде в коде.
pub const DEFAULT_INITIAL_PASSWORD: &str = "123456";

/// Tauri-событие «неуспешный итог проверки экрана логина»
/// (`{ router, outcome, detail }` — фронт показывает detail тостом).
pub const LOGIN_STATE_EVENT: &str = "cloud-routers-login-state";

/// Роутеры, для которых плагин умеет выключать экран логина.
///
/// Публичное API (`POST /api/auth/login` + `PATCH /api/settings`) есть у всего
/// node-семейства, но контракт подтверждён чтением исходника только для
/// 9router (master, `route.js` + `dashboardGuard.js` + `custom-server.js`).
/// Остальные роутеры включаются после такой же проверки, не раньше.
pub fn supports_login_disable(router_id: RouterId) -> bool {
    matches!(router_id, RouterId::NineRouter)
}

/// Итог логин-фазы в рамках одного процесса сервера.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoginStateOutcome {
    /// Фазы A/B прошли, верификация подтвердила `requireLogin=false`.
    Cleared,
    /// 401 — у пользователя свой пароль. requireLogin не трогаем.
    Protected,
    /// 429 — лимитер логина препятствует. Повтора в этом процессе сервера нет.
    RateLimited,
}

/// Проверка экрана логина по состоянию сервера + дедуп по процессу сервера.
///
/// Вызывается из `start_server` после каждого подтверждения, что сервер жив
/// (ветка «уже запущен» и сразу после успешного `wait_port`).
///
/// - Дешёвый `GET /api/settings/require-login` ретраится до успеха: он
///   публичный, побочных эффектов не имеет и отказывает только в аномальном
///   состоянии сервера.
/// - Логин-фаза — РОВНО одна попытка за процесс сервера: pid запоминается до
///   старта фаз. 401 → [`LoginStateOutcome::Protected`] (свой пароль —
///   уважение), 429 → [`LoginStateOutcome::RateLimited`] (лимитер не долбим),
///   прочий отказ — предупреждение без повторов. Новая попытка возможна не
///   раньше нового процесса сервера.
pub(crate) fn enforce_login_state(app: &AppHandle, router_id: RouterId, port: u16, server_pid: u32) {
    if !supports_login_disable(router_id) {
        return;
    }
    let key = (router_id.as_str().to_string(), server_pid);
    if already_checked(&key) {
        log::debug!(
            "no_login: {} (pid {}) — логин-фаза в этом процессе сервера уже была",
            router_id,
            server_pid
        );
        return;
    }

    let client = match blocking_client() {
        Ok(c) => c,
        Err(e) => {
            log::warn!("no_login: {} — {}", router_id, e);
            return;
        }
    };
    let base = format!("http://127.0.0.1:{}", port);
    match fetch_require_login(&client, &base) {
        Ok(false) => {
            remember_checked(key);
            log::info!("no_login: {} — requireLogin=false, экран логина выключен", router_id);
        }
        Ok(true) => {
            // pid запоминается до фаз: логин/PATCH — одна попытка за процесс
            // сервера, поллинг логина и PATCH запрещён.
            remember_checked(key);
            log::info!(
                "no_login: {} — экран логина включён (pid {}): одна попытка дефолтным паролем",
                router_id,
                server_pid
            );
            match disable_via_default_login(router_id, port) {
                Ok(LoginStateOutcome::Cleared) => log::info!(
                    "no_login: {} — экран логина выключен (дефолтный пароль принят, requireLogin=false)",
                    router_id
                ),
                Ok(LoginStateOutcome::Protected) => {
                    log::warn!(
                        "no_login: {} — у роутера свой пароль (401): requireLogin не трогаем, \
                         до рестарта сервера попыток не будет",
                        router_id
                    );
                    emit_login_state(
                        app,
                        router_id,
                        "protected",
                        "Экран логина 9router остаётся: у роутера установлен свой пароль, и плагин не имеет права его менять.".to_string(),
                    );
                }
                Ok(LoginStateOutcome::RateLimited) => {
                    log::warn!(
                        "no_login: {} — лимитер логина (429): не долбим, \
                         следующая попытка — после рестарта роутера",
                        router_id
                    );
                    emit_login_state(
                        app,
                        router_id,
                        "rate_limited",
                        "9router ограничил попытки входа (429): экран логина остаётся до перезапуска роутера.".to_string(),
                    );
                }
                Err(e) => log::warn!(
                    "no_login: {} — попытка не удалась ({}); до рестарта сервера повторов не будет",
                    router_id,
                    e
                ),
            }
        }
        Err(e) => log::warn!(
            "no_login: {} — статус экрана логина недоступен ({}); повтор при следующем обращении",
            router_id,
            e
        ),
    }
}

/// Blocking-клиент для localhost: без прокси, короткие таймауты.
fn blocking_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| format!("HTTP client: {}", e))
}

/// Публичный GET `/api/settings/require-login` → значение флага.
///
/// Err на любые отклонения (сеть, не-2xx, не JSON, поля нет): вызывающий сам
/// решает, ретраить его (дешёвый GET — да) или нет.
pub(crate) fn fetch_require_login(
    client: &reqwest::blocking::Client,
    base: &str,
) -> Result<bool, String> {
    let url = format!("{}/api/settings/require-login", base);
    let resp = client
        .get(&url)
        .send()
        .map_err(|e| format!("статус экрана логина: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("статус экрана логина: HTTP {}", resp.status()));
    }
    let value: serde_json::Value = resp
        .json()
        .map_err(|e| format!("статус экрана логина: не JSON ({})", e))?;
    value
        .get("requireLogin")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| format!("статус экрана логина: нет булева requireLogin: {}", value))
}

/// Фазы A/B для сервера, чей requireLogin уже подтверждён как true.
///
/// Фаза A: официальный вход дефолтным паролем — Cookie `auth_token` приходит
/// только при успехе. Фаза B: `PATCH /api/settings {"requireLogin": false}`
/// под этой Cookie и верификация тем же публичным статусом.
pub(crate) fn disable_via_default_login(
    router_id: RouterId,
    port: u16,
) -> Result<LoginStateOutcome, String> {
    if !supports_login_disable(router_id) {
        return Err(format!(
            "{}: отключение экрана логина не поддерживается",
            router_id
        ));
    }
    let client = blocking_client()?;
    let base = format!("http://127.0.0.1:{}", port);

    log::info!("no_login: {} фаза A — логин с дефолтным паролем", router_id);
    let login_resp = client
        .post(format!("{}/api/auth/login", base))
        .json(&serde_json::json!({ "password": DEFAULT_INITIAL_PASSWORD }))
        .send()
        .map_err(|e| format!("логин: {}", e))?;
    let login_status = login_resp.status();
    if login_status == reqwest::StatusCode::UNAUTHORIZED {
        // Свой пароль — уважение к выбору пользователя (см. доки модуля).
        log::info!("no_login: {} — 401: свой пароль, requireLogin не трогаем", router_id);
        return Ok(LoginStateOutcome::Protected);
    }
    if login_status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        log::info!("no_login: {} — 429: лимитер логина", router_id);
        return Ok(LoginStateOutcome::RateLimited);
    }
    if !login_status.is_success() {
        return Err(format!("логин: HTTP {}", login_status));
    }
    let cookie = extract_auth_cookie(login_resp.headers())?;
    log::info!("no_login: {} — сессия получена", router_id);

    // Фаза B: официальный переключатель "не требовать логин".
    log::info!(
        "no_login: {} фаза B — PATCH /api/settings {{requireLogin: false}}",
        router_id
    );
    let patch_resp = client
        .patch(format!("{}/api/settings", base))
        .header(reqwest::header::COOKIE, cookie)
        .json(&serde_json::json!({ "requireLogin": false }))
        .send()
        .map_err(|e| format!("PATCH settings: {}", e))?;
    if !patch_resp.status().is_success() {
        return Err(format!("PATCH settings: HTTP {}", patch_resp.status()));
    }

    // Верификация публичным статусом (без авторизации): экран логина выключен?
    let after = fetch_require_login(&client, &base)?;
    if after {
        return Err("верификация: экран логина остался включённым после PATCH".to_string());
    }
    log::info!(
        "no_login: {} — экран логина выключен (verify requireLogin=false)",
        router_id
    );
    Ok(LoginStateOutcome::Cleared)
}

/// Extract pair `auth_token=...` из первого Set-Cookie, оставив только пару
/// `name=value` (без Path/HttpOnly-атрибутов).
fn extract_auth_cookie(headers: &reqwest::header::HeaderMap) -> Result<String, String> {
    for value in headers.get_all(reqwest::header::SET_COOKIE) {
        let raw = value
            .to_str()
            .map_err(|e| format!("Set-Cookie не UTF-8: {}", e))?;
        if let Some(pair) = raw.split(';').next() {
            if pair.trim_start().starts_with("auth_token=") {
                return Ok(pair.trim().to_string());
            }
        }
    }
    Err("Set-Cookie с auth_token не найден".to_string())
}

/// Процессы серверов, уже прошедшие логин-фазу в этом сеансе плагина.
///
/// Ключ (роутер, pid): рестарт сервера даёт новый pid → проверка повторится
/// (актуально для 429 и когда пользователь вернул дефолтный пароль). Набор
/// живёт не дольше процесса плагина — отдельной чистки не требуется.
fn checked() -> &'static Mutex<HashSet<(String, u32)>> {
    static CHECKED: OnceLock<Mutex<HashSet<(String, u32)>>> = OnceLock::new();
    CHECKED.get_or_init(|| Mutex::new(HashSet::new()))
}

fn already_checked(key: &(String, u32)) -> bool {
    checked()
        .lock()
        .map(|set| set.contains(key))
        .unwrap_or(true) // отравленный мьютекс: безопаснее считать «уже была»
}

fn remember_checked(key: (String, u32)) {
    if let Ok(mut set) = checked().lock() {
        set.insert(key);
    }
}

/// Неуспешный итог проверки → фронт (тост честен и при закрытой панели).
fn emit_login_state(app: &AppHandle, router_id: RouterId, outcome: &str, detail: String) {
    let payload = serde_json::json!({
        "router": router_id.as_str(),
        "outcome": outcome,
        "detail": detail,
    });
    if let Err(e) = app.emit(LOGIN_STATE_EVENT, payload) {
        log::warn!(
            "no_login: {} — событие {} не отправлено: {}",
            router_id,
            LOGIN_STATE_EVENT,
            e
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;
    use std::thread;

    type Calls = Arc<Mutex<Vec<(String, String)>>>;

    /// Скрипт ответа: (method, path, номер вызова) → (код, Set-Cookie, тело).
    type Script = Box<
        dyn Fn(&str, &str, usize) -> (u16, Option<&'static str>, String) + Send + 'static,
    >;

    /// HTTP-стаб на 127.0.0.1:0: отдаёт скриптованные ответы и считает вызовы.
    ///
    /// Один запрос — одно соединение (`Connection: close`); запрос дочитывается
    /// по заголовкам + Content-Length, чтобы POST успел уйти целиком.
    fn spawn_stub(script: Script) -> (u16, Calls) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind stub");
        let port = listener.local_addr().expect("addr").port();
        let calls: Calls = Arc::new(Mutex::new(Vec::new()));
        let log = calls.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = match stream {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let request = read_request(&mut stream);
                let (method, path) = split_request(&request);
                let idx = {
                    let mut seen = log.lock().unwrap();
                    let idx = seen
                        .iter()
                        .filter(|(m, p)| m.as_str() == method && p.as_str() == path)
                        .count();
                    seen.push((method.clone(), path.clone()));
                    idx
                };
                let (code, cookie, body) = script(&method, &path, idx);
                let reason = match code {
                    200 => "OK",
                    401 => "Unauthorized",
                    429 => "Too Many Requests",
                    _ => "Status",
                };
                let mut resp = format!(
                    "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
                    code, reason, body.len()
                );
                if let Some(c) = cookie {
                    resp.push_str(&format!("Set-Cookie: {}\r\n", c));
                }
                resp.push_str("\r\n");
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.write_all(body.as_bytes());
                let _ = stream.flush();
            }
        });
        (port, calls)
    }

    fn read_request(stream: &mut TcpStream) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(end) = find_headers_end(&buf) {
                        if buf.len() >= end + content_length(&buf[..end]) {
                            break;
                        }
                    }
                }
                Err(_) => break,
            }
        }
        buf
    }

    fn find_headers_end(buf: &[u8]) -> Option<usize> {
        buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4)
    }

    fn content_length(headers: &[u8]) -> usize {
        String::from_utf8_lossy(headers)
            .lines()
            .find_map(|l| {
                let low = l.to_ascii_lowercase();
                low.strip_prefix("content-length:")
                    .and_then(|v| v.trim().parse::<usize>().ok())
            })
            .unwrap_or(0)
    }

    fn split_request(buf: &[u8]) -> (String, String) {
        let text = String::from_utf8_lossy(buf);
        let start = text.lines().next().unwrap_or("");
        let mut parts = start.split_whitespace();
        let method = parts.next().unwrap_or("").to_string();
        let path = parts.next().unwrap_or("").to_string();
        (method, path)
    }

    fn count(calls: &Calls, method: &str, path: &str) -> usize {
        calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(m, p)| m == method && p == path)
            .count()
    }

    fn base(port: u16) -> String {
        format!("http://127.0.0.1:{}", port)
    }

    #[test]
    fn login_disable_only_for_nine_router() {
        assert!(supports_login_disable(RouterId::NineRouter));
        assert!(!supports_login_disable(RouterId::ExtremeRouter));
        assert!(!supports_login_disable(RouterId::OmniRoute));
        assert!(!supports_login_disable(RouterId::Gateway));
    }

    #[test]
    fn phase_ab_rejects_unsupported_router() {
        let err = disable_via_default_login(RouterId::Gateway, 1).unwrap_err();
        assert!(err.contains("не поддерживается"), "{}", err);
    }

    #[test]
    fn extract_auth_cookie_takes_first_pair() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::SET_COOKIE,
            "auth_token=abc.def; Path=/; HttpOnly; SameSite=Lax".parse().unwrap(),
        );
        assert_eq!(extract_auth_cookie(&headers).unwrap(), "auth_token=abc.def");
    }

    #[test]
    fn extract_auth_cookie_missing_is_error() {
        let headers = reqwest::header::HeaderMap::new();
        assert!(extract_auth_cookie(&headers).is_err());
    }

    #[test]
    fn probe_false_flags_off() {
        let (port, calls) = spawn_stub(Box::new(|_m, _p, _i| {
            (200, None, r#"{"requireLogin":false,"tunnelDashboardAccess":true}"#.to_string())
        }));
        let client = blocking_client().unwrap();
        assert!(!fetch_require_login(&client, &base(port)).unwrap());
        assert_eq!(count(&calls, "POST", "/api/auth/login"), 0);
        assert_eq!(count(&calls, "PATCH", "/api/settings"), 0);
    }

    #[test]
    fn probe_true_flags_on() {
        let (port, _calls) = spawn_stub(Box::new(|_m, _p, _i| {
            (200, None, r#"{"requireLogin":true}"#.to_string())
        }));
        let client = blocking_client().unwrap();
        assert!(fetch_require_login(&client, &base(port)).unwrap());
    }

    #[test]
    fn probe_invalid_body_is_error() {
        let (port, _calls) = spawn_stub(Box::new(|_m, _p, _i| (200, None, "не json".to_string())));
        let client = blocking_client().unwrap();
        assert!(fetch_require_login(&client, &base(port)).is_err());
    }

    #[test]
    fn probe_missing_field_is_error() {
        let (port, _calls) = spawn_stub(Box::new(|_m, _p, _i| (200, None, r#"{"x":1}"#.to_string())));
        let client = blocking_client().unwrap();
        assert!(fetch_require_login(&client, &base(port)).is_err());
    }

    #[test]
    fn default_password_clears_screen() {
        // PATCH считается применившимся: верификационный GET после него
        // возвращает requireLogin=false (как реальный сервер).
        let (port, calls) = spawn_stub(Box::new(|method, path, _i| {
            match (method, path) {
                ("POST", "/api/auth/login") => {
                    (200, Some("auth_token=tok.1; Path=/; HttpOnly"), "{}".to_string())
                }
                ("PATCH", "/api/settings") => (200, None, "{}".to_string()),
                _ => (200, None, r#"{"requireLogin":false}"#.to_string()),
            }
        }));
        let outcome = disable_via_default_login(RouterId::NineRouter, port);
        assert_eq!(outcome.unwrap(), LoginStateOutcome::Cleared);
        assert_eq!(count(&calls, "POST", "/api/auth/login"), 1);
        assert_eq!(count(&calls, "PATCH", "/api/settings"), 1);
    }

    #[test]
    fn own_password_is_respected() {
        let (port, calls) = spawn_stub(Box::new(|method, path, _i| {
            match (method, path) {
                ("POST", "/api/auth/login") => (401, None, "{}".to_string()),
                _ => (200, None, r#"{"requireLogin":true}"#.to_string()),
            }
        }));
        let outcome = disable_via_default_login(RouterId::NineRouter, port);
        assert_eq!(outcome.unwrap(), LoginStateOutcome::Protected);
        assert_eq!(count(&calls, "PATCH", "/api/settings"), 0);
    }

    #[test]
    fn rate_limited_stops_without_patch() {
        let (port, calls) = spawn_stub(Box::new(|method, path, _i| {
            match (method, path) {
                ("POST", "/api/auth/login") => (429, None, "{}".to_string()),
                _ => (200, None, r#"{"requireLogin":true}"#.to_string()),
            }
        }));
        let outcome = disable_via_default_login(RouterId::NineRouter, port);
        assert_eq!(outcome.unwrap(), LoginStateOutcome::RateLimited);
        assert_eq!(count(&calls, "PATCH", "/api/settings"), 0);
    }

    #[test]
    fn verify_still_on_is_error() {
        let (port, _calls) = spawn_stub(Box::new(|method, path, _i| {
            match (method, path) {
                ("POST", "/api/auth/login") => (200, Some("auth_token=tok.2"), "{}".to_string()),
                ("PATCH", "/api/settings") => (200, None, "{}".to_string()),
                _ => (200, None, r#"{"requireLogin":true}"#.to_string()),
            }
        }));
        let outcome = disable_via_default_login(RouterId::NineRouter, port);
        assert!(outcome.is_err());
    }

    #[test]
    fn login_without_cookie_is_error() {
        let (port, _calls) = spawn_stub(Box::new(|method, path, _i| {
            match (method, path) {
                ("POST", "/api/auth/login") => (200, None, "{}".to_string()),
                _ => (200, None, r#"{"requireLogin":true}"#.to_string()),
            }
        }));
        let outcome = disable_via_default_login(RouterId::NineRouter, port);
        assert!(outcome.is_err());
    }
}
