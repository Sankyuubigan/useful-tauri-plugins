//! REST-API управления шлюзом (используется только встроенным дашбордом).
//!
//! ## Секреты
//!
//! Конфигурация **никогда** не отдаётся наружу целиком: ключи заменяются на
//! маску (`gsk_12********16`, см. [`crate::logbuf::mask_secret`]). Причина
//! простая и не зависит от политики CORS: дашборд открыт по HTTP без
//! авторизации, поэтому любой локальный процесс и любая страница в браузере
//! видят то, что отдаёт `/api/config`. Маска позволяет узнать свой ключ в
//! списке и не позволяет им воспользоваться.
//!
//! ## Секреты на входе
//!
//! `PUT`/`POST` принимают ключи в открытом виде и **никогда** не возвращают их.
//! При обновлении провайдера без поля `keys` уже сохранённые ключи не трогаются —
//! иначе дашборд, не знающий маски, стёр бы их при каждом сохранении.
//!
//! ## Ошибки
//!
//! Любая ошибка возвращается как `{ "error": "текст" }` с `4xx/5xx`.
//! Сообщения на русском и конкретные: «Провайдер не найден» бесполезно,
//! «У провайдера «groq» нет включённых ключей — добавьте ключ» помогает.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use cloud_routers_core::config::{AccountKey, ProviderConfig, trim_base_url};
use cloud_routers_core::presets;
use cloud_routers_core::store as core_store;

use crate::app::AppState;
use crate::logbuf::mask_secret;
use crate::proxy;
use crate::relay;

/// Тело ошибки управления.
#[derive(Debug, Serialize)]
struct ApiError {
    error: String,
}

fn err(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(ApiError { error: message.into() })).into_response()
}

fn bad_request(message: impl Into<String>) -> Response {
    err(StatusCode::BAD_REQUEST, message)
}

fn not_found(message: impl Into<String>) -> Response {
    err(StatusCode::NOT_FOUND, message)
}

fn internal(message: impl Into<String>) -> Response {
    err(StatusCode::INTERNAL_SERVER_ERROR, message)
}

/// Ошибка в формате `Result<T, Response>` — чтобы обработчики не плодили `match`.
type ApiResult<T> = Result<T, Response>;

// ────────────────────────────── Чтение состояния ──────────────────────────────

/// Статус шлюза для дашборда.
#[derive(Debug, Serialize)]
struct StatusDto {
    version: String,
    port: u16,
    host: String,
    base_url: String,
    uptime_sec: u64,
    providers_total: usize,
    providers_enabled: usize,
    combos_total: usize,
    presets_count: usize,
    presets_source: &'static str,
    presets_updated_at: String,
    relay_url: Option<String>,
    relay_token_set: bool,
    auto_start: bool,
    presets_sync_url: String,
    log_hint: String,
}

async fn status(State(state): State<AppState>) -> Response {
    let config = state.config();
    let (catalog, source) = presets::load(state.data_dir());
    let dto = StatusDto {
        version: cloud_routers_core::VERSION.to_string(),
        port: config.port,
        host: config.host.clone(),
        base_url: config.base_url(),
        uptime_sec: state.uptime().as_secs(),
        providers_total: config.providers.len(),
        providers_enabled: config.providers.iter().filter(|p| p.is_enabled).count(),
        combos_total: cloud_routers_core::models::list_combos(&config.providers).len(),
        presets_count: catalog.providers.len(),
        presets_source: source.as_str(),
        presets_updated_at: catalog.updated_at.clone(),
        relay_url: config.vercel_relay_url.clone(),
        relay_token_set: config
            .vercel_relay_token
            .as_deref()
            .map(|t| !t.trim().is_empty())
            .unwrap_or(false),
        auto_start: config.auto_start,
        presets_sync_url: config
            .presets_sync_url
            .clone()
            .unwrap_or_else(|| presets::DEFAULT_SYNC_URL.to_string()),
        log_hint: "События последних запросов и ретраев".to_string(),
    };
    Json(dto).into_response()
}

// ────────────────────────────── Конфигурация ──────────────────────────────

/// Провайдер в виде для UI: ключи замаскированы.
#[derive(Debug, Serialize)]
struct ProviderDto {
    id: String,
    name: String,
    base_url: String,
    is_enabled: bool,
    use_relay: bool,
    /// Ключи с маской вместо секрета.
    keys: Vec<KeyDto>,
    models: Vec<String>,
    models_count: usize,
    /// Готов к маршрутизации (есть ключ и модели).
    routable: bool,
    /// Остаток кулдауна по каждому ключу, секунды.
    cooldowns: Vec<KeyCooldownDto>,
}

#[derive(Debug, Serialize)]
struct KeyDto {
    id: String,
    masked: String,
    is_active: bool,
}

#[derive(Debug, Serialize)]
struct KeyCooldownDto {
    key_id: String,
    remaining_sec: u64,
}

#[derive(Debug, Serialize)]
struct ConfigDto {
    port: u16,
    host: String,
    auto_start: bool,
    vercel_relay_url: Option<String>,
    vercel_relay_token_set: bool,
    presets_sync_url: String,
    providers: Vec<ProviderDto>,
    problems: Vec<String>,
    data_dir: String,
    config_path: String,
}

fn provider_dto(state: &AppState, p: &ProviderConfig) -> ProviderDto {
    ProviderDto {
        id: p.id.clone(),
        name: p.name.clone(),
        base_url: p.base_url.clone(),
        is_enabled: p.is_enabled,
        use_relay: p.use_relay,
        keys: p
            .keys
            .iter()
            .map(|k| KeyDto { id: k.id.clone(), masked: mask_secret(&k.key), is_active: k.is_active })
            .collect(),
        models: p.models.iter().map(|m| m.id.clone()).collect(),
        models_count: p.models.len(),
        routable: p.is_routable(),
        cooldowns: p
            .keys
            .iter()
            .filter_map(|k| {
                state
                    .rotation()
                    .cooldown_remaining(&p.id, &k.id)
                    .map(|d| KeyCooldownDto {
                        key_id: k.id.clone(),
                        remaining_sec: d.as_secs(),
                    })
            })
            .filter(|c| c.remaining_sec > 0)
            .collect(),
    }
}

async fn get_config(State(state): State<AppState>) -> Response {
    let config = state.config();
    let dto = ConfigDto {
        port: config.port,
        host: config.host.clone(),
        auto_start: config.auto_start,
        vercel_relay_url: config.vercel_relay_url.clone(),
        vercel_relay_token_set: config
            .vercel_relay_token
            .as_deref()
            .map(|t| !t.trim().is_empty())
            .unwrap_or(false),
        presets_sync_url: config
            .presets_sync_url
            .clone()
            .unwrap_or_else(|| presets::DEFAULT_SYNC_URL.to_string()),
        providers: config.providers.iter().map(|p| provider_dto(&state, p)).collect(),
        problems: config.validate(),
        data_dir: state.data_dir().to_string_lossy().to_string(),
        config_path: core_store::config_path(state.data_dir()).to_string_lossy().to_string(),
    };
    Json(dto).into_response()
}

/// Изменяемые поля конфигурации. `None` = «не трогать».
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ConfigPatch {
    port: Option<u16>,
    host: Option<String>,
    auto_start: Option<bool>,
    vercel_relay_url: Option<String>,
    /// Секрет релея — ручной путь для тех, кто разворачивает воркер сам.
    ///
    /// В ответе НЕ возвращается: значение секрета отдаёт отдельный обработчик
    /// `GET /api/relay/token` и только по прямому запросу (см. модульный док).
    vercel_relay_token: Option<String>,
    presets_sync_url: Option<String>,
}

async fn put_config(State(state): State<AppState>, Json(patch): Json<ConfigPatch>) -> ApiResult<Json<ConfigDto>> {
    let port_changed = patch.port.is_some();
    state
        .update_config(|c| {
            if let Some(p) = patch.port {
                if p == 0 {
                    return Err("Порт должен быть больше нуля".to_string());
                }
                c.port = p;
            }
            if let Some(h) = patch.host.as_deref() {
                let h = h.trim();
                if h.is_empty() {
                    return Err("Адрес не может быть пустым".to_string());
                }
                // Привязка не к loopback сделает шлюз доступным из сети, где он
                // отдаёт ключи провайдеров без авторизации.
                if !matches!(h, "127.0.0.1" | "localhost" | "::1") {
                    return Err(format!(
                        "Адрес «{}» запрещён: шлюз хранит ключи провайдеров и не имеет авторизации, слушать можно только 127.0.0.1 / localhost / ::1",
                        h
                    ));
                }
                c.host = h.to_string();
            }
            if let Some(v) = patch.auto_start {
                c.auto_start = v;
            }
            if let Some(v) = patch.vercel_relay_url.as_deref() {
                let v = v.trim();
                c.vercel_relay_url = if v.is_empty() { None } else { Some(trim_base_url(v)) };
            }
            if let Some(v) = patch.vercel_relay_token.as_deref() {
                // Пустая строка — осознанный сброс. Но оставлять поле пустым
                // «на всякий случай» нельзя: воркер сравнивает заголовок с env
                // посимвольно, и пустая строка выглядела бы как заданный секрет,
                // который отвергает каждый запрос. Поэтому пустое = сброс в None.
                let v = v.trim();
                c.vercel_relay_token = if v.is_empty() { None } else { Some(v.to_string()) };
            }
            if let Some(v) = patch.presets_sync_url.as_deref() {
                let v = v.trim();
                if !v.is_empty() && !(v.starts_with("http://") || v.starts_with("https://")) {
                    return Err("URL каталога пресетов должен начинаться с http:// или https://".to_string());
                }
                c.presets_sync_url = if v.is_empty() { None } else { Some(v.to_string()) };
            }
            Ok(())
        })
        .map_err(|e| bad_request(e))?;

    if port_changed {
        state
            .events()
            .warn("изменён порт шлюза — нужен перезапуск процесса (кнопка «Перезапустить шлюз»)");
    }
    let config = state.config();
    Ok(Json(ConfigDto {
        port: config.port,
        host: config.host.clone(),
        auto_start: config.auto_start,
        vercel_relay_url: config.vercel_relay_url.clone(),
        vercel_relay_token_set: config.vercel_relay_token.as_deref().map(|t| !t.trim().is_empty()).unwrap_or(false),
        presets_sync_url: config.presets_sync_url.clone().unwrap_or_else(|| presets::DEFAULT_SYNC_URL.to_string()),
        providers: config.providers.iter().map(|p| provider_dto(&state, p)).collect(),
        problems: config.validate(),
        data_dir: state.data_dir().to_string_lossy().to_string(),
        config_path: core_store::config_path(state.data_dir()).to_string_lossy().to_string(),
    }))
}

async fn reload_config(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    state.reload().map_err(|e| internal(e))?;
    state.events().info("конфигурация перечитана с диска");
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ────────────────────────────── Провайдеры ──────────────────────────────

/// Тело добавления провайдера.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct NewProvider {
    /// Готовый id из каталога пресетов — тогда имя и base_url возьмутся оттуда.
    preset_id: Option<String>,
    /// Явные значения. Перекрывают пресет.
    id: Option<String>,
    name: Option<String>,
    base_url: Option<String>,
    /// API-ключи (можно несколько сразу).
    keys: Vec<String>,
}

async fn add_provider(State(state): State<AppState>, Json(body): Json<NewProvider>) -> ApiResult<Json<ProviderDto>> {
    let config = state.config();
    let (catalog, _) = presets::load(state.data_dir());

    let preset = match body.preset_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(pid) => Some(presets::find(&catalog, pid).cloned().ok_or_else(|| {
            not_found(format!(
                "Пресет «{}» не найден в каталоге (в нём {} провайдеров). Обновите каталог.",
                pid,
                catalog.providers.len()
            ))
        })?),
        None => None,
    };

    // Желаемый id: явный > пресет > имя. Затем slugify + проверка уникальности.
    let desired = body
        .id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| preset.as_ref().map(|p| p.id.clone()))
        .or_else(|| body.name.as_deref().map(|n| n.to_string()))
        .unwrap_or_else(|| "provider".to_string());
    let existing: Vec<String> = config.providers.iter().map(|p| p.id.clone()).collect();
    let id = presets::unique_provider_id(&existing, &desired);

    let base_url = body
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(trim_base_url)
        .or_else(|| preset.as_ref().map(|p| trim_base_url(&p.default_base_url)))
        .ok_or_else(|| {
            bad_request("Не указан base_url: выберите пресет в списке или впишите адрес вручную")
        })?;

    let name = body
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| preset.as_ref().map(|p| p.name.clone()))
        .unwrap_or_else(|| id.clone());

    let keys: Vec<AccountKey> = body
        .keys
        .iter()
        .map(|k| k.trim())
        .filter(|k| !k.is_empty())
        .enumerate()
        .map(|(i, k)| AccountKey { id: format!("k{}", i + 1), key: k.to_string(), is_active: true })
        .collect();

    // Подсказки из пресета НЕ попадают в список моделей: он означает «модели,
    // которые провайдер отдал на реальный /models». Пока этого не случилось,
    // провайдер не маршрутизируем. Пользователь жмёт «Обновить список моделей».
    let provider = ProviderConfig {
        id: id.clone(),
        name,
        base_url,
        is_enabled: true,
        use_relay: false,
        keys,
        models: Vec::new(),
    };

    let key_count = provider.keys.len();
    state
        .update_config(|c| {
            c.providers.push(provider);
            Ok(())
        })
        .map_err(internal)?;

    state.events().info(format!("добавлен провайдер «{}» ({} ключей)", id, key_count));
    notify_host(&state, "provider-added");
    Ok(Json(provider_dto(&state, &state.config().provider(&id).cloned().unwrap_or_default())))
}

/// Патч провайдера. `None` = не менять.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ProviderPatch {
    name: Option<String>,
    base_url: Option<String>,
    is_enabled: Option<bool>,
    use_relay: Option<bool>,
}

/// Достать провайдера или отдать `404`.
///
/// Существование проверяется **до** `update_config`: внутри `update_config` тип
/// ошибки — `String` (её возвращает запись на диск), а различать «нет такого
/// провайдера» (404) и «неверное значение» (400) нужно по статусу ответа.
fn require_provider(state: &AppState, id: &str) -> Result<ProviderConfig, Response> {
    state
        .config()
        .provider(id)
        .cloned()
        .ok_or_else(|| not_found(format!("Провайдер «{}» не найден", id)))
}

async fn patch_provider(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(patch): Json<ProviderPatch>,
) -> ApiResult<Json<ProviderDto>> {
    require_provider(&state, &id)?;
    let updated = state
        .update_config(|c| {
            let p = c
                .provider_mut(&id)
                .ok_or_else(|| format!("Провайдер «{}» исчез между проверкой и записью", id))?;
            if let Some(v) = patch.name.as_deref() {
                let v = v.trim();
                if !v.is_empty() {
                    p.name = v.to_string();
                }
            }
            if let Some(v) = patch.base_url.as_deref() {
                let v = v.trim();
                if v.is_empty() {
                    return Err("base_url не может быть пустым".to_string());
                }
                p.base_url = trim_base_url(v);
                // Смена адреса обесценивает список моделей: они опрашивались
                // по старому. Молча оставить их значило бы слать несуществующие
                // модели новому апстриму.
                p.models.clear();
            }
            if let Some(v) = patch.is_enabled {
                p.is_enabled = v;
            }
            if let Some(v) = patch.use_relay {
                p.use_relay = v;
            }
            Ok(p.clone())
        })
        .map_err(internal)?;

    state.events().info(format!("обновлён провайдер «{}»", id));
    notify_host(&state, "provider-updated");
    Ok(Json(provider_dto(&state, &updated)))
}

async fn delete_provider(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<serde_json::Value>> {
    require_provider(&state, &id)?;
    state
        .update_config(|c| {
            c.providers.retain(|p| p.id != id);
            Ok(())
        })
        .map_err(internal)?;
    state.events().warn(format!("удалён провайдер «{}» вместе с его ключами", id));
    notify_host(&state, "provider-deleted");
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ────────────────────────────── Ключи ──────────────────────────────

/// Тело добавления ключа: один или несколько сразу.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct NewKeys {
    keys: Vec<String>,
}

async fn add_keys(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<NewKeys>,
) -> ApiResult<Json<serde_json::Value>> {
    let incoming: Vec<String> = body
        .keys
        .iter()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .collect();
    if incoming.is_empty() {
        return Err(bad_request("Не передано ни одного ключа"));
    }
    require_provider(&state, &id)?;
    let added = state
        .update_config(|c| {
            let p = c
                .provider_mut(&id)
                .ok_or_else(|| format!("Провайдер «{}» исчез между проверкой и записью", id))?;
            let mut n = p.keys.len();
            let mut added = 0usize;
            for key in &incoming {
                // Повтор того же секрета не плодит мёртвую запись: два одинаковых
                // ключа в ротации бессмысленны и только раздувают список.
                if p.keys.iter().any(|k| k.key == *key) {
                    continue;
                }
                n += 1;
                p.keys.push(AccountKey { id: format!("k{n}"), key: key.clone(), is_active: true });
                added += 1;
            }
            Ok(added)
        })
        .map_err(internal)?;

    if added == 0 {
        return Err(bad_request(format!("Все переданные ключи уже добавлены к провайдеру «{}»", id)));
    }
    state.events().info(format!("к провайдеру «{}» добавлено {} ключей", id, added));
    notify_host(&state, "keys-changed");
    Ok(Json(serde_json::json!({ "added": added })))
}

async fn delete_key(
    State(state): State<AppState>,
    Path((id, key_id)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let provider = require_provider(&state, &id)?;
    if !provider.keys.iter().any(|k| k.id == key_id) {
        return Err(not_found(format!("Ключ «{}» у провайдера «{}» не найден", key_id, id)));
    }
    state
        .update_config(|c| {
            let p = c
                .provider_mut(&id)
                .ok_or_else(|| format!("Провайдер «{}» исчез между проверкой и записью", id))?;
            let before = p.keys.len();
            p.keys.retain(|k| k.id != key_id);
            if p.keys.len() == before {
                return Err(format!("Ключ «{}» у провайдера «{}» не найден", key_id, id));
            }
            Ok(())
        })
        .map_err(internal)?;
    state.rotation().clear_cooldown(&id, &key_id);
    state.events().warn(format!("удалён ключ {} провайдера «{}»", key_id, id));
    notify_host(&state, "keys-changed");
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Снять кулдаун и (при необходимости) снова включить ключ.
///
/// Отдельная кнопка нужна, потому что `401` выключает ключ **навсегда**: без
/// ручного действия пользователь не отличит «ключ списан» от «ключ отозван
/// и перевыпущен».
async fn reset_key(
    State(state): State<AppState>,
    Path((id, key_id)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let provider = require_provider(&state, &id)?;
    if !provider.keys.iter().any(|k| k.id == key_id) {
        return Err(not_found(format!("Ключ «{}» у провайдера «{}» не найден", key_id, id)));
    }
    state
        .update_config(|c| {
            let p = c
                .provider_mut(&id)
                .ok_or_else(|| format!("Провайдер «{}» исчез между проверкой и записью", id))?;
            let k = p
                .keys
                .iter_mut()
                .find(|k| k.id == key_id)
                .ok_or_else(|| format!("Ключ «{}» у провайдера «{}» не найден", key_id, id))?;
            k.is_active = true;
            Ok(())
        })
        .map_err(internal)?;
    state.rotation().clear_cooldown(&id, &key_id);
    state.events().info(format!("ключ {} провайдера «{}» снова включён", key_id, id));
    notify_host(&state, "keys-changed");
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ────────────────────────────── Модели ──────────────────────────────

#[derive(Debug, Serialize)]
struct ModelsRefreshDto {
    provider: String,
    count: usize,
    models: Vec<String>,
    latency_ms: u64,
    via_relay: bool,
}

/// Реальный `GET /models` провайдера и запись результата в конфиг.
async fn refresh_models(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<ModelsRefreshDto>> {
    let config = state.config();
    let provider = require_provider(&state, &id)?;

    let started = std::time::Instant::now();
    let models = proxy::fetch_provider_models(&state, &provider, &config)
        .await
        .map_err(bad_request)?;
    let latency_ms = started.elapsed().as_millis() as u64;
    let via_relay = provider.use_relay;

    if models.is_empty() {
        // Не пишем пустой список: он «съел» бы ранее обнаруженные модели, и
        // провайдер перестал бы быть маршрутизируемым из-за одной плохой
        // ответ��.
        return Err(bad_request(format!(
            "Провайдер «{}» вернул пустой список моделей. Прежние модели сохранены.",
            provider.name
        )));
    }

    let count = models.len();
    let names: Vec<String> = models.iter().map(|m| m.id.clone()).collect();
    state
        .update_config(|c| {
            if let Some(p) = c.provider_mut(&id) {
                p.models = models.clone();
            }
            Ok(())
        })
        .map_err(|e| internal(e))?;

    state.events().info(format!(
        "провайдер «{}»: обнаружено {} моделей за {} мс{}",
        provider.name,
        count,
        latency_ms,
        if via_relay { " (через релей)" } else { "" }
    ));
    notify_host(&state, "models-changed");
    Ok(Json(ModelsRefreshDto {
        provider: provider.name,
        count,
        models: names,
        latency_ms,
        via_relay,
    }))
}

#[derive(Debug, Serialize)]
struct TestDto {
    ok: bool,
    latency_ms: u64,
    models_count: usize,
    via_relay: bool,
    target: String,
}

async fn test_provider(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<TestDto>> {
    let config = state.config();
    let provider = require_provider(&state, &id)?;
    match proxy::ping_provider(&state, &provider, &config).await {
        Ok(r) => Ok(Json(TestDto {
            ok: true,
            latency_ms: r.latency_ms,
            models_count: r.models_count,
            via_relay: r.via_relay,
            target: r.target,
        })),
        Err(e) => {
            state.events().error(format!("проверка провайдера «{}» не удалась: {}", provider.name, e));
            Err(bad_request(e))
        }
    }
}

// ────────────────────────────── Каталог пресетов ──────────────────────────────

#[derive(Debug, Serialize)]
struct PresetsDto {
    source: &'static str,
    updated_at: String,
    sync_url: String,
    providers: Vec<presets::ProviderPreset>,
}

async fn get_presets(State(state): State<AppState>) -> Response {
    let (catalog, source) = presets::load(state.data_dir());
    let config = state.config();
    Json(PresetsDto {
        source: source.as_str(),
        updated_at: catalog.updated_at.clone(),
        sync_url: config
            .presets_sync_url
            .clone()
            .unwrap_or_else(|| presets::DEFAULT_SYNC_URL.to_string()),
        providers: catalog.providers.clone(),
    })
    .into_response()
}

#[derive(Debug, Serialize)]
struct PresetsSyncDto {
    source: &'static str,
    updated_at: String,
    count: usize,
    added: usize,
    removed: usize,
    from_url: String,
}

/// Сетевой sync каталога провайдеров.
///
/// Через релей — если он настроен: `raw.githubusercontent.com` в некоторых
/// сетях недоступен, и обычная проверка обновления каталога превратилась бы в
/// вечное «ошибка сети», хотя рядом стоит работающий релей.
async fn sync_presets(State(state): State<AppState>) -> ApiResult<Json<PresetsSyncDto>> {
    let config = state.config();
    let url = config
        .presets_sync_url
        .clone()
        .unwrap_or_else(|| presets::DEFAULT_SYNC_URL.to_string());

    // Каталог тянем через релей, если он настроен: `raw.githubusercontent.com`
    // в некоторых сетях недоступен, и обычная проверка обновления превратилась бы
    // в вечное «ошибка сети», хотя рядом стоит работающий релей.
    let relay_url = config
        .vercel_relay_url
        .as_deref()
        .map(trim_base_url)
        .filter(|s| !s.is_empty());
    let relay_token = config.vercel_relay_token.as_deref().filter(|t| !t.trim().is_empty());
    let via_relay = relay_url.is_some() && relay_token.is_some();

    let request = if via_relay {
        state
            .http()
            .get(format!(
                "{}{}{}",
                relay_url.expect("проверено выше"),
                crate::upstream::RELAY_PATH_PREFIX,
                url.trim_start_matches('/')
            ))
            .header(crate::upstream::HDR_TARGET_URL, url.as_str())
            .header(crate::upstream::HDR_RELAY_TOKEN, relay_token.expect("проверено выше"))
    } else {
        state.http().get(&url)
    }
    .timeout(std::time::Duration::from_secs(20));

    let response = request
        .send()
        .await
        .map_err(|e| bad_request(format!("Не удалось загрузить каталог пресетов: {}", e)))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(bad_request(format!(
            "Источник каталога ответил HTTP {}. Каталог не изменён — шлюз продолжает работать с текущим.",
            status.as_u16()
        )));
    }
    let catalog = presets::parse(&text).map_err(|e| {
        bad_request(format!("Источник вернул некорректный каталог ({}). Каталог не изменён.", e))
    })?;

    let (previous, _) = presets::load(state.data_dir());
    let before: Vec<&str> = previous.providers.iter().map(|p| p.id.as_str()).collect();
    let after: Vec<&str> = catalog.providers.iter().map(|p| p.id.as_str()).collect();
    let added = after.iter().filter(|id| !before.contains(id)).count();
    let removed = before.iter().filter(|id| !after.contains(id)).count();

    presets::save_cache(state.data_dir(), &catalog).map_err(|e| internal(e))?;
    state.events().info(format!(
        "каталог провайдеров обновлён: {} пресетов (+{} / -{}), источник {}",
        catalog.providers.len(),
        added,
        removed,
        url
    ));
    Ok(Json(PresetsSyncDto {
        source: presets::CatalogSource::Network.as_str(),
        updated_at: catalog.updated_at,
        count: catalog.providers.len(),
        added,
        removed,
        from_url: url,
    }))
}

// ────────────────────────────── Релей ──────────────────────────────

async fn relay_worker() -> Response {
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        relay::RELAY_WORKER,
    )
        .into_response()
}

/// Отдать секрет релея для ручного развёртывания.
///
/// Отдельный обработчик, а не поле в `ConfigDto`, — по уважению к правилу
/// «секреты маскируются» из заголовка модуля. В `PUT /api/config` секрет
/// возвращается только флагом «задан», поэтому даже случайный вызов конфига не
/// раздаёт его всему, кто открыл дашборд. Здесь же пользователь спросил
/// конкретно: он разворачивает воркер руками и обязан вписать `RELAY_TOKEN`
/// в переменные окружения Vercel.
async fn relay_token(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let token = relay::ensure_relay_token(&state).map_err(|e| internal(e))?;
    Ok(Json(serde_json::json!({ "token": token })))
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DeployBody {
    vercel_token: String,
    project: Option<String>,
}

async fn relay_deploy(State(state): State<AppState>, Json(body): Json<DeployBody>) -> ApiResult<Json<serde_json::Value>> {
    let result = relay::deploy_relay(&state, &body.vercel_token, body.project.as_deref())
        .await
        .map_err(|e| bad_request(e))?;
    // Развёрнутый URL сразу сохраняем: пользователь не должен копировать его
    // руками и не должен получить «получилось, но в поле пусто».
    let url = result.url.clone();
    state
        .update_config(|c| {
            c.vercel_relay_url = Some(url);
            Ok(())
        })
        .map_err(|e| internal(e))?;
    notify_host(&state, "relay-deployed");
    Ok(Json(serde_json::to_value(result).map_err(|e| internal(e.to_string()))?))
}

async fn relay_test(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let probe = relay::probe_relay(&state).await.map_err(|e| bad_request(e))?;
    Ok(Json(serde_json::to_value(probe).map_err(|e| internal(e.to_string()))?))
}

// ────────────────────────────── Журнал ──────────────────────────────

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct EventsQuery {
    /// Только события с номером больше `since`.
    since: Option<u64>,
    /// Сколько последних событий вернуть (без `since`).
    limit: Option<usize>,
}

async fn get_events(State(state): State<AppState>, axum::extract::Query(q): axum::extract::Query<EventsQuery>) -> Response {
    let (events, last_seq) = match q.since {
        Some(since) => (state.events().since(since), state.events().last_seq()),
        None => {
            let ev = state.events().tail(q.limit.unwrap_or(200));
            let last = state.events().last_seq();
            (ev, last)
        }
    };
    Json(serde_json::json!({ "events": events, "last_seq": last_seq })).into_response()
}

/// Сообщить хосту (панели настроек), что конфигурация изменилась.
///
/// Хост держит свой список комбо для выпадающих списков. Без уведомления он
/// обновил бы его только по своему 15-секундному тику — пользователь увидел бы
/// «модели не появились» при уже применённом изменении.
fn notify_host(state: &AppState, kind: &str) {
    let combos = cloud_routers_core::models::list_combos(&state.config().providers)
        .len();
    state.events().push("info", format!("конфигурация изменена ({}) — комбо доступно: {}", kind, combos));
}

// ────────────────────────────── Перезапуск ──────────────────────────────

/// Перезапуск процесса шлюза.
///
/// Нужен для смены порта и для применения изменений, которые читаются только
/// при старте. Ответ — `202 Accepted`: сервер может не успеть дописать тело,
/// процесс перезапустится и всё равно закроет соединение.
async fn restart(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    state.events().warn("запрошен перезапуск шлюза");
    let exe = std::env::current_exe()
        .map_err(|e| internal(format!("Не удалось определить путь к себе: {}", e)))?;
    let data_dir = state.data_dir().to_path_buf();
    let config = state.config();
    match crate::respawn::spawn_replacement(&exe, &data_dir, &config) {
        Ok(()) => {
            // Ответ отправляем до выхода процесса, иначе IDE/дашборд увидят
            // обрыв соединения вместо подтверждения.
            crate::respawn::wait_and_exit(&data_dir, config.port, config.port);
        }
        Err(e) => Err(internal(format!("Не удалось запустить новый экземпляр: {}", e))),
    }
}

/// Проверить, что конфигурация в порядке (используется дашбордом как «светофор»).
async fn health_check(State(state): State<AppState>) -> Response {
    let config = state.config();
    let problems = config.validate();
    Json(serde_json::json!({
        "status": if problems.is_empty() { "ok" } else { "degraded" },
        "version": cloud_routers_core::VERSION,
        "uptime_sec": state.uptime().as_secs(),
        "problems": problems,
    }))
    .into_response()
}

/// Собрать все маршруты управления (используется в [`crate::server`]).
pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/api/status", axum::routing::get(status))
        .route("/api/health", axum::routing::get(health_check))
        .route("/api/config", axum::routing::get(get_config).put(put_config))
        .route("/api/config/reload", axum::routing::post(reload_config))
        .route("/api/providers", axum::routing::post(add_provider))
        .route("/api/providers/{id}", axum::routing::patch(patch_provider).delete(delete_provider))
        .route("/api/providers/{id}/keys", axum::routing::post(add_keys))
        .route(
            "/api/providers/{id}/keys/{key_id}",
            axum::routing::delete(delete_key),
        )
        .route(
            "/api/providers/{id}/keys/{key_id}/reset",
            axum::routing::post(reset_key),
        )
        .route("/api/providers/{id}/models/refresh", axum::routing::post(refresh_models))
        .route("/api/providers/{id}/test", axum::routing::post(test_provider))
        .route("/api/presets", axum::routing::get(get_presets))
        .route("/api/presets/sync", axum::routing::post(sync_presets))
        .route("/api/relay/worker", axum::routing::get(relay_worker))
        .route("/api/relay/token", axum::routing::get(relay_token))
        .route("/api/relay/deploy", axum::routing::post(relay_deploy))
        .route("/api/relay/test", axum::routing::post(relay_test))
        .route("/api/events", axum::routing::get(get_events))
        .route("/api/restart", axum::routing::post(restart))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;
    use cloud_routers_core::config::GatewayConfig;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("cr_gw_admin_{tag}"));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[tokio::test]
    async fn get_config_masks_keys_and_exposes_paths() {
        let dir = tmp("mask");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let mut cfg = GatewayConfig::default();
        cfg.providers.push(ProviderConfig {
            id: "groq".into(),
            name: "Groq".into(),
            base_url: "https://api.groq.com/openai/v1".into(),
            keys: vec![AccountKey {
                id: "k1".into(),
                key: "gsk_supersecretvalue123".into(),
                is_active: true,
            }],
            models: vec![cloud_routers_core::DiscoveredModel { id: "m1".into(), ..Default::default() }],
            ..Default::default()
        });
        core_store::save(&dir, &cfg).expect("save");
        let state = AppState::new(&dir).expect("state");

        let r = get_config(State(state)).await;
        assert_eq!(r.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(r.into_response().into_body(), usize::MAX).await.expect("body");
        let text = String::from_utf8_lossy(&bytes);
        let secret = "gsk_supersecretvalue123";
        assert!(!text.contains(secret), "секрет утек в /api/config:\n{text}");
        // Маска обязана оставаться узнаваемой: без префикса пользователь не
        // отличит свои ключи друг от друга.
        assert!(
            text.contains(&format!("{}********{}", &secret[..6], secret.len())),
            "ожидалась маска с префиксом и длиной:\n{text}"
        );
        assert!(text.contains("config_path"), "путь к конфигу должен быть виден");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn put_config_rejects_non_loopback_host() {
        // Шлюз отдаёт ключи провайдеров без авторизации — слушать извне нельзя.
        let dir = tmp("host");
        let state = AppState::new(&dir).expect("state");
        let r = put_config(
            State(state),
            Json(ConfigPatch { host: Some("0.0.0.0".into()), ..Default::default() }),
        )
        .await;
        assert!(r.is_err(), "0.0.0.0 должен быть отвергнут");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn put_config_rejects_zero_port() {
        let dir = tmp("port0");
        let state = AppState::new(&dir).expect("state");
        let r = put_config(State(state), Json(ConfigPatch { port: Some(0), ..Default::default() })).await;
        assert!(r.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn put_config_rejects_bad_presets_url() {
        let dir = tmp("presetsurl");
        let state = AppState::new(&dir).expect("state");
        let r = put_config(
            State(state),
            Json(ConfigPatch { presets_sync_url: Some("ftp://x".into()), ..Default::default() }),
        )
        .await;
        assert!(r.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Ручной путь: пользователь разворачивает воркер сам и обязан вписать
    /// секрет в переменные окружения Vercel. Значит `ConfigPatch` обязан его
    /// принимать, а пустая строка — честно сбрасывать в `None` (воркер
    /// сравнивает заголовок с env посимвольно: «заданный» пустой секрет
    /// отвергал бы каждый запрос с 401, и UI показывал бы «задан»).
    #[tokio::test]
    async fn put_config_accepts_and_clears_relay_token() {
        let dir = tmp("relaytoken");
        let state = AppState::new(&dir).expect("state");

        put_config(
            State(state.clone()),
            Json(ConfigPatch { vercel_relay_token: Some("  my-secret  ".into()), ..Default::default() }),
        )
        .await
        .expect("принять секрет");
        assert_eq!(
            cloud_routers_core::load_config(&dir).vercel_relay_token.as_deref(),
            Some("my-secret"),
            "секрет должен сохраниться без краёв пробелов"
        );

        put_config(
            State(state.clone()),
            Json(ConfigPatch { vercel_relay_token: Some("   ".into()), ..Default::default() }),
        )
        .await
        .expect("сброс секрета");
        assert_eq!(
            cloud_routers_core::load_config(&dir).vercel_relay_token, None,
            "пустая строка — это сброс, а не «секрет из пробелов»"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Секрет релея НЕ должен утекать через `GET /api/config`: дашборд
    /// открыт по HTTP без авторизации, а правило модуля — маскировать секреты.
    /// Отдельный обработчик `GET /api/relay/token` существует именно поэтому.
    #[tokio::test]
    async fn get_config_never_exposes_relay_token_value() {
        let dir = tmp("relaymask");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let mut cfg = GatewayConfig::default();
        cfg.vercel_relay_token = Some("super_secret_relay_token_value".into());
        core_store::save(&dir, &cfg).expect("save");
        let state = AppState::new(&dir).expect("state");

        let r = get_config(State(state)).await;
        let bytes = axum::body::to_bytes(r.into_response().into_body(), usize::MAX).await.expect("body");
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains("super_secret_relay_token_value"), "секрет релея утек в /api/config:\n{text}");
        assert!(text.contains("\"vercel_relay_token_set\":true"), "пользователь должен видеть, что секрет задан");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Обратная сторона: разворачивая руками, пользователь обязан увидеть
    /// секрет — иначе воркер останется без `RELAY_TOKEN` и будет отдавать 503.
    /// Эндпоинт создаёт секрет при первом обращении и отдаёт его.
    #[tokio::test]
    async fn relay_token_endpoint_creates_and_returns_secret() {
        let dir = tmp("relaytokenep");
        let state = AppState::new(&dir).expect("state");
        assert!(state.config().vercel_relay_token.is_none());

        let r = relay_token(State(state)).await.expect("секрет выдаётся");
        let bytes = axum::body::to_bytes(r.into_response().into_body(), usize::MAX).await.expect("body");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        let token = v["token"].as_str().expect("token");
        assert_eq!(token.len(), 64, "ожидался 64 hex-символа: {}", token);
        assert_eq!(
            cloud_routers_core::load_config(&dir).vercel_relay_token.as_deref(),
            Some(token),
            "выданный секрет обязан сохраниться, а не жить только в ответе"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn add_provider_takes_name_and_base_url_from_preset() {
        let dir = tmp("addpreset");
        let state = AppState::new(&dir).expect("state");
        let r = add_provider(
            State(state),
            Json(NewProvider {
                preset_id: Some("groq".into()),
                keys: vec!["gsk-1".into()],
                ..Default::default()
            }),
        )
        .await
        .expect("add");
        let bytes = axum::body::to_bytes(r.into_response().into_body(), usize::MAX).await.expect("body");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(v["id"], "groq");
        assert_eq!(v["base_url"], "https://api.groq.com/openai/v1");
        assert_eq!(v["models_count"], 0, "модели появляются только после реального опроса");
        assert_eq!(v["keys"].as_array().expect("keys").len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn add_provider_does_not_seed_models_from_hint() {
        // Подсказки каталога не должны становиться «моделями провайдера»:
        // список моделей означает «апстрим это отдал».
        let dir = tmp("hint");
        let state = AppState::new(&dir).expect("state");
        let r = add_provider(
            State(state),
            Json(NewProvider { preset_id: Some("groq".into()), ..Default::default() }),
        )
        .await
        .expect("add");
        let bytes = axum::body::to_bytes(r.into_response().into_body(), usize::MAX).await.expect("body");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(v["models_count"], 0);
        assert_eq!(v["routable"], false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn add_provider_allocates_unique_id_on_collision() {
        // Второй провайдер того же сервиса получает суффикс, а не перезатирает
        // первый: два разных аккаунта у одного сервиса — штатная ситуация.
        let dir = tmp("collide");
        let first = add_provider(
            State(AppState::new(&dir).expect("state")),
            Json(NewProvider {
                preset_id: Some("groq".into()),
                keys: vec!["gsk-1".into()],
                ..Default::default()
            }),
        )
        .await
        .expect("first add");
        let b1 = axum::body::to_bytes(first.into_response().into_body(), usize::MAX)
            .await
            .expect("body");
        let v1: serde_json::Value = serde_json::from_slice(&b1).expect("json");
        assert_eq!(v1["id"], "groq");

        let second = add_provider(
            State(AppState::new(&dir).expect("state")),
            Json(NewProvider {
                preset_id: Some("groq".into()),
                keys: vec!["gsk-2".into()],
                ..Default::default()
            }),
        )
        .await
        .expect("second add");
        let b2 = axum::body::to_bytes(second.into_response().into_body(), usize::MAX)
            .await
            .expect("body");
        let v2: serde_json::Value = serde_json::from_slice(&b2).expect("json");
        assert_eq!(v2["id"], "groq-2", "уникальность id обязана соблюдаться");

        // Первый провайдер не пострадал.
        let config = core_store::load(&dir);
        assert_eq!(config.providers.len(), 2);
        assert_eq!(config.providers[0].keys[0].key, "gsk-1");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn add_provider_rejects_unknown_preset() {
        let dir = tmp("badpreset");
        let state = AppState::new(&dir).expect("state");
        let r = add_provider(
            State(state),
            Json(NewProvider { preset_id: Some("no-such-provider".into()), ..Default::default() }),
        )
        .await;
        assert!(r.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn patch_provider_clears_models_when_base_url_changes() {
        let dir = tmp("clear");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let mut cfg = GatewayConfig::default();
        cfg.providers.push(ProviderConfig {
            id: "groq".into(),
            name: "Groq".into(),
            base_url: "https://api.groq.com/openai/v1".into(),
            keys: vec![AccountKey { id: "k1".into(), key: "gsk-1".into(), is_active: true }],
            models: vec![cloud_routers_core::DiscoveredModel { id: "old-model".into(), ..Default::default() }],
            ..Default::default()
        });
        core_store::save(&dir, &cfg).expect("save");
        let state = AppState::new(&dir).expect("state");

        let r = patch_provider(
            State(state),
            Path("groq".to_string()),
            Json(ProviderPatch { base_url: Some("https://other.dev/v1".into()), ..Default::default() }),
        )
        .await
        .expect("patch");
        let bytes = axum::body::to_bytes(r.into_response().into_body(), usize::MAX).await.expect("body");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(v["models_count"], 0, "смена апстрима обесценивает список моделей");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn patch_provider_keeps_keys_untouched() {
        // Дашборд не знает секретов (видит маски) — сохранение без поля keys
        // обязано сохранить ключи, иначе любой save стирал бы авторизацию.
        let dir = tmp("keepkeys");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let mut cfg = GatewayConfig::default();
        cfg.providers.push(ProviderConfig {
            id: "groq".into(),
            name: "Groq".into(),
            base_url: "https://api.groq.com/openai/v1".into(),
            keys: vec![AccountKey { id: "k1".into(), key: "gsk-secret".into(), is_active: true }],
            ..Default::default()
        });
        core_store::save(&dir, &cfg).expect("save");
        let state = AppState::new(&dir).expect("state");

        let r = patch_provider(
            State(state),
            Path("groq".to_string()),
            Json(ProviderPatch { name: Some("Groq (мой)".into()), ..Default::default() }),
        )
        .await
        .expect("patch");
        let bytes = axum::body::to_bytes(r.into_response().into_body(), usize::MAX).await.expect("body");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(v["name"], "Groq (мой)");
        assert_eq!(v["keys"].as_array().expect("keys").len(), 1);
        assert_eq!(core_store::load(&dir).providers[0].keys[0].key, "gsk-secret");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn delete_key_reports_unknown_key() {
        let dir = tmp("delkey");
        let state = AppState::new(&dir).expect("state");
        let r = delete_key(State(state), Path(("nope".into(), "k1".into()))).await;
        assert!(r.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn delete_provider_reports_unknown_provider() {
        let dir = tmp("delp");
        let state = AppState::new(&dir).expect("state");
        let r = delete_provider(State(state), Path("nope".into())).await;
        assert!(r.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn add_keys_rejects_duplicate_secret() {
        let dir = tmp("dupkey");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let mut cfg = GatewayConfig::default();
        cfg.providers.push(ProviderConfig {
            id: "groq".into(),
            name: "Groq".into(),
            base_url: "https://api.groq.com/openai/v1".into(),
            keys: vec![AccountKey { id: "k1".into(), key: "gsk-same".into(), is_active: true }],
            ..Default::default()
        });
        core_store::save(&dir, &cfg).expect("save");
        let state = AppState::new(&dir).expect("state");

        let r = add_keys(
            State(state),
            Path("groq".into()),
            Json(NewKeys { keys: vec!["gsk-same".into()] }),
        )
        .await;
        assert!(r.is_err(), "дубль секрета должен отвергаться, а не плодить запись");
        assert_eq!(core_store::load(&dir).providers[0].keys.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn refresh_models_rejects_provider_without_keys() {
        let dir = tmp("nokeyrefresh");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let mut cfg = GatewayConfig::default();
        cfg.providers.push(ProviderConfig {
            id: "groq".into(),
            name: "Groq".into(),
            base_url: "https://api.groq.com/openai/v1".into(),
            ..Default::default()
        });
        core_store::save(&dir, &cfg).expect("save");
        let state = AppState::new(&dir).expect("state");
        let r = refresh_models(State(state), Path("groq".into())).await;
        assert!(r.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn sync_presets_surfaces_unreachable_source_without_losing_cache() {
        // Недоступный источник не должен оставлять пользователя без каталога.
        let dir = tmp("syncfail");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let mut cfg = GatewayConfig::default();
        cfg.presets_sync_url = Some("http://127.0.0.1:1/none.json".into());
        core_store::save(&dir, &cfg).expect("save");
        let state = AppState::new(&dir).expect("state");

        let r = sync_presets(State(state)).await;
        assert!(r.is_err());
        let (catalog, source) = presets::load(&dir);
        assert!(catalog.providers.len() >= 20);
        assert_eq!(source, presets::CatalogSource::Embedded);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn health_check_reports_degraded_for_invalid_config() {
        let dir = tmp("health");
        std::fs::create_dir_all(&dir).expect("mkdir");
        // Записываем заведомо плохой конфиг в обход save (он бы не дал).
        std::fs::write(
            core_store::config_path(&dir),
            r#"{"port":20131,"providers":[{"id":"bad/id","name":"x","base_url":"https://a.dev"}]}"#,
        )
        .expect("write");
        let state = AppState::new(&dir).expect("state");
        let r = health_check(State(state)).await;
        let bytes = axum::body::to_bytes(r.into_response().into_body(), usize::MAX).await.expect("body");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(v["status"], "degraded");
        assert!(!v["problems"].as_array().expect("problems").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn events_endpoint_supports_since_and_tail() {
        let dir = tmp("events");
        // Один и тот же экземпляр состояния: буфер событий живёт в памяти
        // процесса и НЕ перечитывается из файла. Отдельный AppState видел бы
        // пустой журнал — и это правильное поведение (журнал сессионный).
        let state = AppState::new(&dir).expect("state");
        state.events().info("первое");
        let mark = state.events().info("второе");
        state.events().warn("третье");

        let r = get_events(
            State(state.clone()),
            axum::extract::Query(EventsQuery { since: Some(mark), limit: None }),
        )
        .await;
        let bytes = axum::body::to_bytes(r.into_response().into_body(), usize::MAX).await.expect("body");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        let msgs: Vec<&str> = v["events"]
            .as_array()
            .expect("events")
            .iter()
            .map(|e| e["message"].as_str().expect("msg"))
            .collect();
        assert_eq!(msgs, vec!["третье"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}