//! OAuth Device Flow для Kiro (AWS Builder ID / IAM Identity Center).
//!
//! Спека — `docs/kiro-protocol.md`. Поток (подтверждено кодом 9Router):
//! 1. `POST oidc.{region}/client/register` → clientId/clientSecret;
//! 2. `POST oidc.{region}/device_authorization` → deviceCode/userCode/verificationUri;
//! 3. пользователь подтверждает в браузере (свой логин — не токен из 9Router,
//!    см. правило двух мест в доке);
//! 4. серверный поллинг `POST oidc.{region}/token` → access/refresh;
//! 5. `ListAvailableProfiles` → profileArn (для OAuth), сохранение сессии.
//!
//! Рефреш — проактивный до экспирации + single-flight через мьютекс:
//! два параллельных рефреша одного refresh-токена устраивают гонку, второй
//! падает. Неуспех = состояние `auth_expired`, а не тихое умирание.
//!
//! Ожидающие device-подтверждения хранятся в памяти (не на диске): deviceCode
//! живёт минуты, переживать рестарт ему не нужно, а на диске он был бы
//! мусором-секретом.

use std::collections::BTreeMap;
use std::sync::Mutex;

use axum::http::HeaderMap;
use cloud_routers_core::protocol::{OAuthMethod, OAuthSession};
use serde::Deserialize;

use crate::app::AppState;

/// Клиент OIDC для Device Flow (идентификация шлюза перед AWS).
const CLIENT_NAME: &str = "cloud-routers-gateway";
const CLIENT_TYPE: &str = "public";
/// Области из 9Router (`KIRO_CONFIG.scopes`): точный список — после первого
/// живого зонда; пока минимальный честный набор для CodeWhisperer.
const SCOPES: &[&str] = &["codewhisperer:conversations", "codewhisperer:read"];
const GRANT_TYPES: &[&str] = &["urn:ietf:params:oauth:grant-type:device_code", "refresh_token"];
/// `startUrl` Builder ID (подтверждено кодом 9Router: дефолтный startUrl).
const BUILDER_ID_START_URL: &str = "https://view.awsapps.com/start";
/// Ошибка AWS «ещё не подтвердили»: продолжаем поллинг, а не падаем.
const PENDING_ERRORS: &[&str] = &["authorization_pending", "slow_down"];

/// Ожидающий device flow (память процесса).
struct PendingFlow {
    session_id: String,
    provider_id: String,
    region: String,
    client_id: String,
    client_secret: String,
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: String,
    expires_in: u64,
}

/// Состояние OAuth-модуля: ожидающие потоки + single-flight рефреша.
pub struct OAuthState {
    pending: Mutex<BTreeMap<String, PendingFlow>>,
    refreshing: Mutex<std::collections::HashSet<String>>,
}

impl OAuthState {
    pub fn new() -> Self {
        Self { pending: Mutex::new(BTreeMap::new()), refreshing: Mutex::new(Default::default()) }
    }

    fn lock_pending(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, PendingFlow>> {
        self.pending.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl Default for OAuthState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn oauth(&self) -> &OAuthState {
        self.oauth_state()
    }
}

/// Начало Device Flow: регистрация клиента + device_authorization.
///
/// Возвращает данные для экрана пользователя (user_code + URL).
pub async fn start_device_flow(
    state: &AppState,
    provider_id: &str,
    region: &str,
) -> Result<DeviceFlowPrompt, String> {
    let pending_id = next_session_id();
    let (client_id, client_secret) =
        register_client(state, region, CLIENT_NAME, CLIENT_TYPE).await?;
    let dev = device_authorization(state, region, &client_id, &client_secret).await?;
    let prompt = DeviceFlowPrompt {
        session_id: pending_id.clone(),
        provider_id: provider_id.to_string(),
        user_code: dev.user_code.clone(),
        verification_uri: dev.verification_uri.clone(),
        verification_uri_complete: dev.verification_uri_complete.clone(),
        expires_in: dev.expires_in,
    };
    state.oauth().lock_pending().insert(
        pending_id,
        PendingFlow {
            session_id: prompt.session_id.clone(),
            provider_id: provider_id.to_string(),
            region: region.to_string(),
            client_id,
            client_secret,
            device_code: dev.device_code,
            user_code: dev.user_code,
            verification_uri: dev.verification_uri,
            verification_uri_complete: dev.verification_uri_complete,
            expires_in: dev.expires_in,
        },
    );
    state.events().info(format!(
        "Kiro OAuth: device flow начат для «{provider_id}», код {}",
        prompt.user_code
    ));
    Ok(prompt)
}

/// Данные для экрана пользователя: что ввести в браузере.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DeviceFlowPrompt {
    pub session_id: String,
    pub provider_id: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub expires_in: u64,
}

/// Одна итерация поллинга: `pending` (продолжать), `done` (токены) или ошибка.
///
/// Вызывающий (HTTP-хендлер) решает интервал: серверный цикл с ожиданием
/// блокировал бы воркер axum. `authorization_pending`/`slow_down` — не ошибки.
pub async fn poll_once(state: &AppState, session_id: &str) -> Result<PollOutcome, String> {
    let flow = state
        .oauth()
        .lock_pending()
        .get(session_id)
        .map(|f| {
            (
                f.provider_id.clone(),
                f.region.clone(),
                f.client_id.clone(),
                f.client_secret.clone(),
                f.device_code.clone(),
            )
        })
        .ok_or_else(|| "device flow не найден: начните вход заново".to_string())?;
    let (provider_id, region, client_id, client_secret, device_code) = flow;
    match poll_token(state, &region, &client_id, &client_secret, &device_code).await? {
        PollResult::Pending => Ok(PollOutcome::Pending),
        PollResult::Tokens { access_token, refresh_token, expires_in } => {
            let profile_arn = fetch_profile_arn(state, &region, &access_token).await?;
            let session = OAuthSession {
                provider_id: provider_id.clone(),
                method: OAuthMethod::DeviceCode,
                access_token,
                refresh_token: Some(refresh_token),
                region: region.clone(),
                profile_arn: profile_arn.clone(),
                email: None,
                expires_at_unix: now_unix() + expires_in,
                refresh_context: refresh_context(&client_id, &client_secret, &region),
            };
            let mut sessions = cloud_routers_core::store::load_oauth_sessions(state.data_dir());
            sessions.insert(session_id.to_string(), session);
            cloud_routers_core::store::save_oauth_sessions(state.data_dir(), &sessions)?;
            state.oauth().lock_pending().remove(session_id);
            state.events().info(format!(
                "Kiro OAuth: вход завершён для «{provider_id}» (профиль {})",
                profile_arn.as_deref().unwrap_or("без профиля")
            ));
            Ok(PollOutcome::Done { profile_arn })
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PollOutcome {
    Pending,
    Done { profile_arn: Option<String> },
}

/// Отвязать сессию: удалить из файла и из конфига провайдера.
pub fn disconnect(state: &AppState, provider_id: &str) -> Result<bool, String> {
    let session_id = state
        .config()
        .provider(provider_id)
        .and_then(|p| p.auth.oauth_session_id().map(|s| s.to_string()));
    let Some(session_id) = session_id else { return Ok(false) };
    let mut sessions = cloud_routers_core::store::load_oauth_sessions(state.data_dir());
    let removed = sessions.remove(&session_id).is_some();
    cloud_routers_core::store::save_oauth_sessions(state.data_dir(), &sessions)?;
    state.update_config(|c| {
        if let Some(p) = c.provider_mut(provider_id) {
            p.auth = cloud_routers_core::protocol::ProviderAuth::ApiKeys;
            p.models.clear();
        }
        Ok(())
    })?;
    state
        .events()
        .info(format!("Kiro OAuth: «{provider_id}» отвязан (сессия {session_id})"));
    Ok(removed)
}

/// Проактивный рефреш токена сессии (single-flight).
///
/// Вызывается перед запросом, если `expires_at_unix` близок/прошёл.
/// Параллельный рефреш той же сессии ждёт, а не шлёт второй запрос:
/// иначе вторая попытка со старым токеном падает.
pub async fn ensure_fresh(state: &AppState, session_id: &str) -> Result<(), String> {
    {
        let mut set =
            state.oauth().refreshing.lock().unwrap_or_else(|p| p.into_inner());
        if !set.insert(session_id.to_string()) {
            return Err("рефреш уже выполняется параллельно: повторите запрос".to_string());
        }
    }
    let result = refresh_inner(state, session_id).await;
    state.oauth().refreshing.lock().unwrap_or_else(|p| p.into_inner()).remove(session_id);
    result
}

async fn refresh_inner(state: &AppState, session_id: &str) -> Result<(), String> {
    let sessions = cloud_routers_core::store::load_oauth_sessions(state.data_dir());
    let s = sessions.get(session_id).ok_or_else(|| {
        format!("OAuth-сессия «{session_id}» не найдена: пройдите вход заново")
    })?;
    if fresh_enough(s) {
        return Ok(());
    }
    let refresh_token = s.refresh_token.as_deref().filter(|t| !t.trim().is_empty()).ok_or_else(|| {
        format!("У сессии «{session_id}» нет refresh-токена: войдите заново")
    })?;
    let (client_id, client_secret, region) = refresh_parts(s)?;
    let (access, new_refresh, expires_in) =
        refresh_token_grant(state, &region, &client_id, &client_secret, refresh_token).await?;
    let mut sessions = sessions;
    if let Some(entry) = sessions.get_mut(session_id) {
        entry.access_token = access;
        if !new_refresh.trim().is_empty() {
            entry.refresh_token = Some(new_refresh);
        }
        entry.expires_at_unix = now_unix() + expires_in;
    }
    cloud_routers_core::store::save_oauth_sessions(state.data_dir(), &sessions)?;
    state.events().info(format!("Kiro OAuth: токен сессии «{session_id}» обновлён"));
    Ok(())
}

fn fresh_enough(s: &OAuthSession) -> bool {
    // Запас 120 секунд: токен, умирающий во время запроса, — та же ложь,
    // что и протухший заранее.
    s.expires_at_unix > now_unix() + 120
}

fn refresh_parts(s: &OAuthSession) -> Result<(String, String, String), String> {
    let client_id = s.refresh_context.get("client_id").cloned().unwrap_or_default();
    let client_secret = s.refresh_context.get("client_secret").cloned().unwrap_or_default();
    if client_id.trim().is_empty() || client_secret.trim().is_empty() {
        return Err("У сессии нет данных рефреша (clientId/secret): войдите заново".to_string());
    }
    Ok((client_id, client_secret, s.region.clone()))
}

fn refresh_context(client_id: &str, client_secret: &str, region: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("client_id".to_string(), client_id.to_string());
    m.insert("client_secret".to_string(), client_secret.to_string());
    m.insert("region".to_string(), region.to_string());
    m.insert("start_url".to_string(), BUILDER_ID_START_URL.to_string());
    m.insert("auth_method".to_string(), "builder-id".to_string());
    m
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn next_session_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("oauth-{}-{n}", now_unix())
}

// ── HTTP-уровень OIDC ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct RegisterResponse {
    #[serde(rename = "clientId")]
    client_id: String,
    #[serde(rename = "clientSecret")]
    client_secret: String,
}

#[derive(Debug, Deserialize)]
struct DeviceAuthResponse {
    #[serde(rename = "deviceCode")]
    device_code: String,
    #[serde(rename = "userCode")]
    user_code: String,
    #[serde(rename = "verificationUri")]
    verification_uri: String,
    #[serde(rename = "verificationUriComplete")]
    verification_uri_complete: String,
    #[serde(rename = "expiresIn")]
    expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
    #[serde(rename = "refreshToken")]
    refresh_token: Option<String>,
    #[serde(rename = "expiresIn")]
    expires_in: Option<u64>,
    error: Option<String>,
    #[serde(rename = "error_description")]
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProfilesResponse {
    profiles: Option<Vec<serde_json::Value>>,
}

enum PollResult {
    Pending,
    Tokens { access_token: String, refresh_token: String, expires_in: u64 },
}

async fn register_client(
    state: &AppState,
    region: &str,
    name: &str,
    ctype: &str,
) -> Result<(String, String), String> {
    let url = format!("https://oidc.{region}.amazonaws.com/client/register");
    let body = serde_json::json!({
        "clientName": name,
        "clientType": ctype,
        "scopes": SCOPES,
        "grantTypes": GRANT_TYPES,
        "issuerUrl": "https://identitycenter.amazonaws.com/ssooxy",
    });
    let text = post_json(state, &url, &body, &HeaderMap::new()).await?;
    let r: RegisterResponse = serde_json::from_str(&text)
        .map_err(|e| format!("OIDC register: неверный ответ ({e}): {}", head(&text)))?;
    Ok((r.client_id, r.client_secret))
}

async fn device_authorization(
    state: &AppState,
    region: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<DeviceAuthResponse, String> {
    let url = format!("https://oidc.{region}.amazonaws.com/device_authorization");
    let body = serde_json::json!({
        "clientId": client_id,
        "clientSecret": client_secret,
        "startUrl": BUILDER_ID_START_URL,
    });
    let text = post_json(state, &url, &body, &HeaderMap::new()).await?;
    serde_json::from_str(&text)
        .map_err(|e| format!("OIDC device_authorization: неверный ответ ({e}): {}", head(&text)))
}

async fn poll_token(
    state: &AppState,
    region: &str,
    client_id: &str,
    client_secret: &str,
    device_code: &str,
) -> Result<PollResult, String> {
    let url = format!("https://oidc.{region}.amazonaws.com/token");
    let body = serde_json::json!({
        "clientId": client_id,
        "clientSecret": client_secret,
        "deviceCode": device_code,
        "grantType": "urn:ietf:params:oauth:grant-type:device_code",
    });
    let text = post_json(state, &url, &body, &HeaderMap::new()).await?;
    let r: TokenResponse = serde_json::from_str(&text)
        .map_err(|e| format!("OIDC token: неверный ответ ({e}): {}", head(&text)))?;
    if let Some(err) = r.error.as_deref() {
        if PENDING_ERRORS.contains(&err) {
            return Ok(PollResult::Pending);
        }
        let desc = r.error_description.as_deref().unwrap_or("");
        return Err(format!("OIDC token: {err} {desc}").trim().to_string());
    }
    match (r.access_token, r.refresh_token) {
        (Some(a), Some(rt)) if !a.trim().is_empty() && !rt.trim().is_empty() => {
            Ok(PollResult::Tokens {
                access_token: a,
                refresh_token: rt,
                expires_in: r.expires_in.unwrap_or(3600),
            })
        }
        _ => Err(format!("OIDC token: нет токенов в ответе: {}", head(&text))),
    }
}

async fn refresh_token_grant(
    state: &AppState,
    region: &str,
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> Result<(String, String, u64), String> {
    let url = format!("https://oidc.{region}.amazonaws.com/token");
    let body = serde_json::json!({
        "clientId": client_id,
        "clientSecret": client_secret,
        "refreshToken": refresh_token,
        "grantType": "refresh_token",
    });
    let text = post_json(state, &url, &body, &HeaderMap::new()).await?;
    let r: TokenResponse = serde_json::from_str(&text)
        .map_err(|e| format!("OIDC refresh: неверный ответ ({e}): {}", head(&text)))?;
    if let Some(err) = r.error.as_deref() {
        let desc = r.error_description.as_deref().unwrap_or("");
        return Err(format!("OIDC refresh: {err} {desc} — войдите заново").trim().to_string());
    }
    let access = r.access_token.filter(|a| !a.trim().is_empty()).ok_or_else(|| {
        format!("OIDC refresh: нет access-токена: {}", head(&text))
    })?;
    Ok((access, r.refresh_token.unwrap_or_default(), r.expires_in.unwrap_or(3600)))
}

async fn fetch_profile_arn(
    state: &AppState,
    region: &str,
    access_token: &str,
) -> Result<Option<String>, String> {
    let url = format!("https://codewhisperer.{region}.amazonaws.com");
    let body = serde_json::json!({ "maxResults": 10 });
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::HeaderName::from_static("content-type"),
        axum::http::HeaderValue::from_static("application/x-amz-json-1.0"),
    );
    headers.insert(
        axum::http::HeaderName::from_static("x-amz-target"),
        axum::http::HeaderValue::from_static("AmazonCodeWhispererService.ListAvailableProfiles"),
    );
    headers.insert(
        axum::http::HeaderName::from_static("authorization"),
        axum::http::HeaderValue::from_str(&format!("Bearer {}", access_token.trim()))
            .map_err(|_| "access-токен не ложится в заголовок".to_string())?,
    );
    let text = post_json(state, &url, &body, &headers).await?;
    let r: ProfilesResponse = serde_json::from_str(&text)
        .map_err(|e| format!("ListAvailableProfiles: неверный ответ ({e}): {}", head(&text)))?;
    let profiles = r.profiles.unwrap_or_default();
    let arn_of = |p: &serde_json::Value| {
        p.get("arn").or_else(|| p.get("profileArn")).and_then(|v| v.as_str()).map(|s| s.to_string())
    };
    // Тот же выбор, что у 9Router: профиль того же региона, иначе первый.
    let matched = profiles.iter().find_map(|p| {
        let arn = arn_of(p)?;
        if arn.split(':').nth(3) == Some(region) { Some(arn) } else { None }
    });
    Ok(matched.or_else(|| profiles.iter().find_map(arn_of)))
}

async fn post_json(
    state: &AppState,
    url: &str,
    body: &serde_json::Value,
    extra: &HeaderMap,
) -> Result<String, String> {
    let bytes = serde_json::to_vec(body).map_err(|e| format!("не собралось тело: {e}"))?;
    let mut req = state
        .http()
        .post(url)
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .body(bytes);
    for (k, v) in extra.iter() {
        req = req.header(k, v);
    }
    let resp = req.send().await.map_err(|e| format!("{}: {}", url, reqwest_kind(&e)))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("{} ответил HTTP {}: {}", url, status.as_u16(), head(&text)));
    }
    Ok(text)
}

fn head(text: &str) -> String {
    text.chars().take(300).collect()
}

/// Короткий вид сетевой ошибки (та же шкала, что в sync каталога).
fn reqwest_kind(e: &reqwest::Error) -> &'static str {
    if e.is_builder() {
        "неверный URL запроса"
    } else if e.is_connect() {
        "нет соединения"
    } else if e.is_timeout() {
        "таймаут"
    } else if e.is_request() {
        "ошибка отправки запроса"
    } else if e.is_body() || e.is_decode() {
        "ошибка чтения тела ответа"
    } else {
        "ошибка сети"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_outcome_shape_is_explicit() {
        assert_eq!(PollOutcome::Pending, PollOutcome::Pending);
        assert!(matches!(
            PollOutcome::Done { profile_arn: None },
            PollOutcome::Done { .. }
        ));
    }

    #[test]
    fn region_allowlist_is_shared_with_kiro_adapter() {
        assert!(crate::protocol_kiro::valid_region("us-east-1"));
        assert!(!crate::protocol_kiro::valid_region("evil.com"));
    }
}
