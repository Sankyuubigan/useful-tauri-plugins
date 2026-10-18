//! Разовое выключение экрана логина дашборда 9router после установки.
//!
//! ## Контракт
//!
//! Установка node-роутера только что разместила файлы (`installer.rs` ставит в
//! конфиг `pending_no_login` для свежей установки — когда предыдущей
//! установленной версии не было). При первом старте сервера
//! [`consume_pending`] один раз выполняет официальный вход в дашборд с
//! дефолтным паролем и `PATCH /api/settings {"requireLogin": false}` — тем же
//! путём, каким пользователь отключил бы галочку «требовать логин» в
//! настройках 9router. Прямой правки SQLite нет: настройки меняются только
//! через публичный API самого роутера.
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
//! ## Уважение к выбору пользователя
//!
//! Флаг потребляется ОДНОКРАТНО. Если у пользователя уже стоит свой пароль
//! (логин отвечает 401), requireLogin не трогается, в лог пишется подсказка —
//! и повторных попыток нет. Если он позже сам включит требование логина, плагин
//! его настройки больше не пишет: отключение экрана логина — часть установки,
//! а не «инвариант на каждый старт».

use std::time::Duration;
use tauri::AppHandle;

use crate::commands::RouterId;
use crate::router::config;

/// Дефолтный пароль дашборда 9router. SSOT: литерал больше нигде в коде.
pub const DEFAULT_INITIAL_PASSWORD: &str = "123456";

/// Роутеры, для которых плагин умеет выключать экран логина.
///
/// Публичное API (`POST /api/auth/login` + `PATCH /api/settings`) есть у всего
/// node-семейства, но контракт подтверждён чтением исходника только для
/// 9router (master, `route.js` + `dashboardGuard.js` + `custom-server.js`).
/// Остальные роутеры включаются после такой же проверки, не раньше.
pub fn supports_login_disable(router_id: RouterId) -> bool {
    matches!(router_id, RouterId::NineRouter)
}

/// Погасить `pending_no_login`, если он поставлен свежей установкой.
///
/// Вызывается из двух мест `start_server` (ранний возврат «уже запущен» и
/// после успешного `wait_port`): попытка делается ОДИН раз за жизнь флага;
/// успех гасит флаг навсегда, временный отказ (сеть/429) — оставляет, и на
/// следующем старте сервера попытка повторяется. Это не «инвариант на каждый
/// старт»: попыток максимум одна за старт, и только пока установка свежая.
pub fn consume_pending(app: &AppHandle, router_id: RouterId, port: u16) {
    let cfg = config::load_config(app, router_id);
    if !cfg.pending_no_login || !supports_login_disable(router_id) {
        return;
    }
    log::info!(
        "no_login: {} — установка свежая, однократно выключаем экран логина",
        router_id
    );
    match apply_once(router_id, port) {
        Ok(()) => {
            let mut cfg = config::load_config(app, router_id);
            cfg.pending_no_login = false;
            if let Err(e) = config::save_config(app, router_id, &cfg) {
                log::warn!("no_login: {} — флаг не погасился: {}", router_id, e);
            }
        }
        Err(e) => log::warn!(
            "no_login: {} — попытка не удалась ({}), повтор при следующем старте сервера",
            router_id,
            e
        ),
    }
}

/// Выключить экран логина: логин дефолтным паролем → PATCH requireLogin=false.
///
/// Ok в двух случаях: обе фазы прошли, либо у пользователя уже свой пароль
/// (401 — трогать requireLogin не имеем права). Остальные отказы — Err.
pub fn apply_once(router_id: RouterId, port: u16) -> Result<(), String> {
    if !supports_login_disable(router_id) {
        return Err(format!(
            "{}: отключение экрана логина не поддерживается",
            router_id
        ));
    }
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| format!("HTTP client: {}", e))?;
    let base = format!("http://127.0.0.1:{}", port);
    let status_url = format!("{}/api/settings/require-login", base);

    // Предварительный статус: публичный путь без авторизации. Если экран логина
    // уже выключен (или выключен ранее нами) — ни логина, ни PATCH не нужно,
    // и попытка ничего не расходует.
    match client.get(&status_url).send() {
        Ok(resp) if resp.status().is_success() => {
            let value: serde_json::Value = resp.json().unwrap_or_default();
            if value.get("requireLogin").and_then(|v| v.as_bool()) == Some(false) {
                log::info!(
                    "no_login: {} — requireLogin уже false, отключение не требуется",
                    router_id
                );
                return Ok(());
            }
        }
        // Нестандартный/недоступный сервис — продолжаем попытку: вердикт по
        // результату фаз A/B, а не по отсутствию статуса.
        Ok(resp) => log::warn!(
            "no_login: {} — статус-screen HTTP {}, продолжаем попытку",
            router_id,
            resp.status()
        ),
        Err(e) => log::warn!("no_login: {} — статус недоступен ({}), продолжаем", router_id, e),
    }

    // Фаза A: официальный логин с дефолтным паролем.Cookie `auth_token` уходит
    // с ответом только при успехе.
    let login_url = format!("{}/api/auth/login", base);
    log::info!("no_login: {} фаза A — логин с дефолтным паролем", router_id);
    let login_resp = client
        .post(&login_url)
        .json(&serde_json::json!({ "password": DEFAULT_INITIAL_PASSWORD }))
        .send()
        .map_err(|e| format!("логин: {}", e))?;
    let login_status = login_resp.status();
    if login_status == reqwest::StatusCode::UNAUTHORIZED {
        // Свой пароль уже стоит — respects пользователя (см. доки модуля).
        log::warn!(
            "no_login: {} — у пользователя уже свой пароль, requireLogin НЕ трогаем",
            router_id
        );
        return Ok(());
    }
    if !login_status.is_success() {
        return Err(format!("логин: HTTP {}", login_status));
    }
    let cookie = extract_auth_cookie(login_resp.headers())?;
    log::info!("no_login: {} — сессия получена", router_id);

    // Фаза B: официальный переключатель "не требовать логин".
    let settings_url = format!("{}/api/settings", base);
    log::info!(
        "no_login: {} фаза B — PATCH /api/settings {{requireLogin: false}}",
        router_id
    );
    let patch_resp = client
        .patch(&settings_url)
        .header(reqwest::header::COOKIE, cookie)
        .json(&serde_json::json!({ "requireLogin": false }))
        .send()
        .map_err(|e| format!("PATCH settings: {}", e))?;
    if !patch_resp.status().is_success() {
        return Err(format!("PATCH settings: HTTP {}", patch_resp.status()));
    }

    // Верификация публичным статусом (без авторизации): экран логина выключен?
    let verify_resp = client
        .get(&status_url)
        .send()
        .map_err(|e| format!("верификация: {}", e))?;
    if !verify_resp.status().is_success() {
        return Err(format!("верификация: HTTP {}", verify_resp.status()));
    }
    let value: serde_json::Value = verify_resp.json().unwrap_or_default();
    if value.get("requireLogin").and_then(|v| v.as_bool()) == Some(false) {
        log::info!("no_login: {} — экран логина выключен (verify requireLogin=false)", router_id);
        Ok(())
    } else {
        Err(format!(
            "верификация: экран логина не выключился (requireLogin={})",
            value.get("requireLogin").and_then(|v| v.as_bool()).map(|b| b.to_string()).unwrap_or_else(|| "?".into())
        ))
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_disable_only_for_nine_router() {
        assert!(supports_login_disable(RouterId::NineRouter));
        assert!(!supports_login_disable(RouterId::ExtremeRouter));
        assert!(!supports_login_disable(RouterId::OmniRoute));
        assert!(!supports_login_disable(RouterId::Gateway));
    }

    #[test]
    fn apply_once_rejects_unsupported_router() {
        let err = apply_once(RouterId::Gateway, 1).unwrap_err();
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
}
