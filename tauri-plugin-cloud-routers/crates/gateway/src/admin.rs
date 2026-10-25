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

use cloud_routers_core::config::{
    AccountKey, ProviderConfig, RELAY_KIND_CLOUDFLARE, RELAY_KIND_VERCEL, trim_base_url,
};
use cloud_routers_core::presets;
use cloud_routers_core::store as core_store;

use crate::app::AppState;
use crate::key_check;
use crate::logbuf::mask_secret;
use crate::proxy;
use crate::relay;
use crate::relay_cf;

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
    presets_sources: Vec<String>,
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
        relay_url: config.relay_url(),
        relay_token_set: config
            .relay_token()
            .map(|t| !t.trim().is_empty())
            .unwrap_or(false),
        auto_start: config.auto_start,
        presets_sources: config.effective_presets_sources(),
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
    /// Протокол апстрима (`openai` | `kiro`).
    protocol: cloud_routers_core::protocol::VendorProtocol,
    /// Способ авторизации: `api_keys` либо маскированный OAuth-статус.
    /// Секретов здесь нет — только факт привязки и метаданные.
    auth: ProviderAuthDto,
    is_enabled: bool,
    use_relay: bool,
    /// Ключи с маской вместо секрета.
    keys: Vec<KeyDto>,
    models: Vec<String>,
    models_count: usize,
    /// Готов к маршрутизации (есть ключ и модели).
    routable: bool,
    /// Апстрим — Gemini (generativelanguage.googleapis.com): только такому
    /// провайдеру дашборд показывает кнопку «Проверить ключи».
    is_gemini: bool,
    /// Остаток кулдауна по каждому ключу, секунды.
    cooldowns: Vec<KeyCooldownDto>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ProviderAuthDto {
    ApiKeys,
    OAuth {
        session_id: String,
        /// Сессия найдена на диске.
        session_present: bool,
        method: Option<cloud_routers_core::protocol::OAuthMethod>,
        email: Option<String>,
        expires_at_unix: u64,
        refreshable: bool,
    },
}

#[derive(Debug, Serialize)]
struct KeyDto {
    id: String,
    /// Имя, заданное пользователем при добавлении (`mihael2`), если было.
    name: Option<String>,
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
    presets_sources: Vec<String>,
    providers: Vec<ProviderDto>,
    problems: Vec<String>,
    data_dir: String,
    config_path: String,
}

fn provider_dto(state: &AppState, p: &ProviderConfig) -> ProviderDto {
    let auth = match p.auth.oauth_session_id() {
        None => ProviderAuthDto::ApiKeys,
        Some(session_id) => {
            let sessions = cloud_routers_core::store::load_oauth_sessions(state.data_dir());
            match sessions.get(session_id) {
                Some(s) => {
                    let dto = cloud_routers_core::protocol::OAuthSessionDto::mask(
                        session_id,
                        s,
                    );
                    ProviderAuthDto::OAuth {
                        session_id: session_id.to_string(),
                        session_present: true,
                        method: Some(dto.method),
                        email: dto.email,
                        expires_at_unix: dto.expires_at_unix,
                        refreshable: dto.refreshable,
                    }
                }
                None => ProviderAuthDto::OAuth {
                    session_id: session_id.to_string(),
                    session_present: false,
                    method: None,
                    email: None,
                    expires_at_unix: 0,
                    refreshable: false,
                },
            }
        }
    };
    ProviderDto {
        id: p.id.clone(),
        name: p.name.clone(),
        base_url: p.base_url.clone(),
        protocol: p.protocol,
        auth,
        is_enabled: p.is_enabled,
        use_relay: p.use_relay,
        keys: p
            .keys
            .iter()
            .map(|k| KeyDto {
                id: k.id.clone(),
                name: k.name.clone(),
                masked: mask_secret(&k.key),
                is_active: k.is_active,
            })
            .collect(),
        models: p.models.iter().map(|m| m.id.clone()).collect(),
        models_count: p.models.len(),
        routable: p.is_routable(),
        is_gemini: key_check::is_gemini_provider(p),
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
        vercel_relay_url: config.relay_url(),
        vercel_relay_token_set: config
            .relay_token()
            .map(|t| !t.trim().is_empty())
            .unwrap_or(false),
        presets_sources: config.effective_presets_sources(),
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
    /// Новый список источников каталога. Пустой список = сброс к дефолтам.
    ///
    /// `None` = «не трогать». Отдельного «удалить один» нет: список обычно из
    /// 2–4 элементов, целиком его прислать проще, чем изобретать операции.
    presets_sources: Option<Vec<String>>,
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
                // Схемная проверка здесь же, а не в момент запроса: релей
                // без схемы (реальный кейс 22.10.2026 — «my-relay.vercel.app»)
                // ломает КАЖДЫЙ запрос через него бессмысленным
                // «builder error». Пусть это будет ошибка сохранения с
                // понятным текстом, а не поломка каталога через час.
                let base = if v.is_empty() {
                    None
                } else {
                    let base = cloud_routers_core::config::relay_base_url(v);
                    if base.is_none() {
                        return Err(format!(
                            "URL релея «{v}» должен начинаться с http:// или https://"
                        ));
                    }
                    base
                };
                // Запись идёт в Vercel-слот, если он есть (миграция 23.10.2026);
                // в конфигах без слотов — в legacy-поле (совместимая запись).
                if let Some(slot) = c.vercel_slot_mut() {
                    slot.url = base;
                } else {
                    c.vercel_relay_url = base;
                }
            }
            if let Some(v) = patch.vercel_relay_token.as_deref() {
                // Пустая строка — осознанный сброс. Но оставлять поле пустым
                // «на всякий случай» нельзя: воркер сравнивает заголовок с env
                // посимвольно, и пустая строка выглядела бы как заданный секрет,
                // который отвергает каждый запрос. Поэтому пустое = сброс в None.
                let v = v.trim();
                let secret = if v.is_empty() { None } else { Some(v.to_string()) };
                if let Some(slot) = c.vercel_slot_mut() {
                    slot.relay_secret = secret;
                } else {
                    c.vercel_relay_token = secret;
                }
            }
            if let Some(list) = patch.presets_sources.as_deref() {
                let mut cleaned: Vec<String> = Vec::with_capacity(list.len());
                for raw in list {
                    let v = raw.trim();
                    if v.is_empty() {
                        continue;
                    }
                    if !(v.starts_with("http://") || v.starts_with("https://")) {
                        return Err(format!(
                            "URL источника каталога «{v}» должен начинаться с http:// или https://"
                        ));
                    }
                    if !cleaned.iter().any(|x| x == v) {
                        cleaned.push(v.to_string());
                    }
                }
                c.presets_sources = cleaned;
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
        vercel_relay_url: config.relay_url(),
        vercel_relay_token_set: config.relay_token().map(|t| !t.trim().is_empty()).unwrap_or(false),
        presets_sources: config.effective_presets_sources(),
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

/// Один элемент списка ключей во входе дашборда.
///
/// `#[serde(untagged)]` принимает ОБА формата:
/// - объект `{"name": "mihael2", "secret": "AQ.Ab8…"}` — новый (имя задаётся
///   при добавлении);
/// - плоскую строку `"AQ.Ab8…"` — старый (совместимость: сохранить ввод без
///   имени в старом формате не должен ломаться).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum KeySpec {
    Named { name: String, secret: String },
    Plain(String),
}

/// SSOT-конверсия `KeySpec → AccountKey`: трим, пропуск пустых, дедуп по
/// секрету, сквозная нумерация `k{n}` от `next_index`.
///
/// Единственная точка сборки ключей. Раньше модалка провайдера и «Добавить
/// ключи» делали это дублирующим кодом — при любом новом правиле (нормализация,
/// дедуп) пришлось бы править два места; правило живёт здесь одно.
fn keyspecs_into_account_keys(
    specs: &[KeySpec],
    existing: &mut Vec<AccountKey>,
    mut next_index: usize,
) -> usize {
    for spec in specs {
        let (secret, name) = match spec {
            KeySpec::Named { name, secret } => (secret.as_str(), Some(name.clone())),
            KeySpec::Plain(secret) => (secret.as_str(), None),
        };
        let secret = secret.trim();
        if secret.is_empty() {
            continue;
        }
        // Повтор того же секрета не плодит мёртвую запись: два одинаковых
        // ключа в ротации бессмысленны и только раздувают список.
        if existing.iter().any(|k| k.key == secret) {
            continue;
        }
        next_index += 1;
        existing.push(cloud_routers_core::config::account_key(
            format!("k{next_index}"),
            secret.to_string(),
            name,
        ));
    }
    next_index
}

/// Список ключей для события: `<имя> (<маска>)`. Секреты никогда не попадают
/// в журнал целиком — только через `mask_secret`.
fn key_brief_list(keys: &[AccountKey]) -> String {
    keys.iter()
        .map(|k| match k.name.as_deref() {
            Some(n) => format!("{n} ({})", mask_secret(&k.key)),
            None => mask_secret(&k.key),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

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
    /// API-ключи (можно несколько сразу), каждым — с опциональным именем.
    keys: Vec<KeySpec>,
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

    // Ключи собирает SSOT-хелпер: нумерация k1…, трим, пустые отброшены,
    // дубликаты схлопнуты. Имя ключа (если задано) живёт в AccountKey.name.
    let mut keys: Vec<AccountKey> = Vec::new();
    keyspecs_into_account_keys(&body.keys, &mut keys, 0);

    // Подсказки из пресета НЕ попадают в список моделей: он означает «модели,
    // которые провайдер отдал на реальный /models». Пока этого не случилось,
    // провайдер не маршрутизируем. Пользователь жмёт «Обновить список моделей».
    // Протокол — из пресета каталога: ручной выбор протокола при добавлении
    // появится в дашборде шагом 5. Пока пресет определяет язык провайдера.
    let protocol = preset
        .map(|p| p.protocol)
        .unwrap_or(cloud_routers_core::protocol::VendorProtocol::Openai);
    let provider = ProviderConfig {
        id: id.clone(),
        name,
        base_url,
        protocol,
        auth: cloud_routers_core::protocol::ProviderAuth::ApiKeys,
        is_enabled: true,
        use_relay: false,
        keys,
        models: Vec::new(),
    };

    let key_count = provider.keys.len();
    let key_brief = key_brief_list(&provider.keys);
    state
        .update_config(|c| {
            c.providers.push(provider);
            Ok(())
        })
        .map_err(internal)?;

    state.events().info(format!(
        "добавлен провайдер «{}» ({} ключей: {})",
        id, key_count, key_brief
    ));
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

/// Тело добавления ключей: один или несколько сразу. Каждый элемент — либо
/// плоская строка-секрет (старый формат), либо `{"name": "…", "secret": "…"}`
/// (новый, с именем). Разбор выступает `KeySpec` — см. его док.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct NewKeys {
    keys: Vec<KeySpec>,
}

async fn add_keys(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<NewKeys>,
) -> ApiResult<Json<serde_json::Value>> {
    // Хотя бы один непустой секрет должен прийти: обработать «пустые строки»
    // как «передано ничего» — та же причина, что и раньше.
    let has_input = body.keys.iter().any(|s| match s {
        KeySpec::Named { secret, .. } => !secret.trim().is_empty(),
        KeySpec::Plain(s) => !s.trim().is_empty(),
    });
    if !has_input {
        return Err(bad_request("Не передано ни одного ключа"));
    }
    require_provider(&state, &id)?;
    let (added, added_keys) = state
        .update_config(|c| {
            let p = c
                .provider_mut(&id)
                .ok_or_else(|| format!("Провайдер «{}» исчез между проверкой и записью", id))?;
            let start = p.keys.len();
            let next = keyspecs_into_account_keys(&body.keys, &mut p.keys, start);
            Ok((next - start, p.keys[start..].to_vec()))
        })
        .map_err(internal)?;

    if added == 0 {
        return Err(bad_request(format!("Все переданные ключи уже добавлены к провайдеру «{}»", id)));
    }
    state.events().info(format!(
        "к провайдеру «{}» добавлено {} ключ(ей): {}",
        id,
        added,
        key_brief_list(&added_keys)
    ));
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

// ────────────────────────────── OAuth (Kiro) ──────────────────────────────

/// Начало Device Flow: дашборд показывает user_code + ссылку.
///
/// Провайдер обязан быть Kiro: для OpenAI OAuth не существует, и молчаливый
/// старт потока ввёл бы пользователя в заблуждение.
async fn oauth_start(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> ApiResult<Json<crate::kiro_oauth::DeviceFlowPrompt>> {
    let provider = require_provider(&state, &id)?;
    if provider.protocol != cloud_routers_core::protocol::VendorProtocol::Kiro {
        return Err(bad_request(format!(
            "OAuth есть только у Kiro: провайдер «{}» — {:?}",
            provider.id, provider.protocol
        )));
    }
    let region = body
        .get("region")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("us-east-1")
        .to_string();
    if !crate::protocol_kiro::valid_region(&region) {
        return Err(bad_request(format!("некорректный AWS-регион «{region}»")));
    }
    crate::kiro_oauth::start_device_flow(&state, &id, &region).await.map(Json).map_err(bad_request)
}

/// Одна итерация поллинга: дашборд зовёт раз в несколько секунд.
///
/// `done` без profileArn — валидно для API-ключа? Нет: этот путь только для
/// Device Flow, профиль здесь обязателен в строгом смысле? Тоже нет: профиль
/// может отсутствовать у свежих аккаунтов — тогда чат идёт без profileArn
/// (токен default). Честно отдаём то, что есть.
async fn oauth_poll(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> ApiResult<Json<serde_json::Value>> {
    let _provider = require_provider(&state, &id)?;
    let session_id = body
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| bad_request("нет session_id device flow"))?;
    match crate::kiro_oauth::poll_once(&state, session_id).await.map_err(bad_request)? {
        crate::kiro_oauth::PollOutcome::Pending => {
            Ok(Json(serde_json::json!({ "status": "pending" })))
        }
        crate::kiro_oauth::PollOutcome::Done { profile_arn } => {
            // Привязываем провайдера к сессии: без этого чат не найдёт токен.
            let session_id = session_id.to_string();
            state
                .update_config(|c| {
                    let Some(p) = c.provider_mut(&id) else {
                        return Err(format!("провайдер «{id}» исчез во время входа"));
                    };
                    p.auth = cloud_routers_core::protocol::ProviderAuth::OAuth { session_id };
                    p.models.clear();
                    Ok(())
                })
                .map_err(internal)?;
            state.events().info(format!("«{id}»: привязана OAuth-сессия, обновите модели"));
            Ok(Json(serde_json::json!({
                "status": "done",
                "profile_arn": profile_arn,
            })))
        }
    }
}

/// Статус OAuth-привязки провайдера (маскированный — без секретов).
async fn oauth_status(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<serde_json::Value>> {
    let provider = require_provider(&state, &id)?;
    let Some(session_id) = provider.auth.oauth_session_id() else {
        return Ok(Json(serde_json::json!({ "bound": false })));
    };
    let sessions = cloud_routers_core::store::load_oauth_sessions(state.data_dir());
    match sessions.get(session_id) {
        Some(s) => Ok(Json(serde_json::json!({
            "bound": true,
            "session": cloud_routers_core::protocol::OAuthSessionDto::mask(session_id, s),
        }))),
        None => Ok(Json(serde_json::json!({
            "bound": true,
            "missing": true,
            "hint": "сессия удалена: пройдите вход заново",
        }))),
    }
}

/// Отвязка: удалить сессию и перевести провайдера на ключи.
async fn oauth_disconnect(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    require_provider(&state, &id)?;
    let removed = crate::kiro_oauth::disconnect(&state, &id).map_err(internal)?;
    Ok(Json(serde_json::json!({ "ok": true, "session_removed": removed })))
}

// ────────────────────────────── Каталог пресетов ──────────────────────────────

#[derive(Debug, Serialize)]
struct PresetsDto {
    source: &'static str,
    updated_at: String,
    sync_url: String,
    sync_urls: Vec<String>,
    providers: Vec<presets::ProviderPreset>,
}

async fn get_presets(State(state): State<AppState>) -> Response {
    let (catalog, source) = presets::load(state.data_dir());
    let config = state.config();
    let urls = config.effective_presets_sources();
    Json(PresetsDto {
        source: source.as_str(),
        updated_at: catalog.updated_at.clone(),
        sync_url: urls.first().cloned().unwrap_or_default(),
        sync_urls: urls,
        providers: catalog.providers.clone(),
    })
    .into_response()
}

/// Одна попытка источника при sync: что пробовали и чем кончилось.
///
/// Поле существует ради диагностики: раньше неуспех любого источника давал
/// одинаковое «Не удалось загрузить каталог пресетов», и понять, кто виноват —
/// сеть, 404, чужой формат — было не по чему (см. историю с «builder error»).
#[derive(Debug, Serialize)]
struct SourceAttempt {
    url: String,
    ok: bool,
    /// Что дал источник: число провайдеров, число подсказок или текст ошибки.
    detail: String,
}

#[derive(Debug, Serialize)]
struct PresetsSyncDto {
    source: &'static str,
    updated_at: String,
    count: usize,
    added: usize,
    removed: usize,
    from_url: String,
    attempts: Vec<SourceAttempt>,
}

/// Сетевой sync каталога провайдеров.
///
/// Обход ВСЕГО списка: провайдеров берёт первый пригодный источник (раньше был
/// один URL, и его 404 означал мёртвый каталог навсегда), а модели остальных
/// источников идут в подсказки (`default_models_hint`). Подсказки litellm —
/// best-effort: его недоступность sync не роняет.
///
/// Через релей — если он настроен: часть источников в некоторых сетях
/// недоступна напрямую, и обычная проверка обновления превратилась бы в
/// вечное «ошибка сети», хотя рядом стоит работающий релей.
async fn sync_presets(State(state): State<AppState>) -> ApiResult<Json<PresetsSyncDto>> {
    use presets::adapters::litellm::parse_litellm_hints;
    use presets::adapters::models_dev::ExternalProvider;
    use presets::adapters::models_json::parse_models_json_hints;
    use presets::adapters::{classify_source_body, parse_models_dev_body, SourceBodyKind};

    let config = state.config();
    let urls = config.effective_presets_sources();
    if urls.is_empty() {
        return Err(bad_request(
            "Список источников каталога пуст. Добавьте хотя бы один URL или сбросьте список к значениям по умолчанию.",
        ));
    }

    let mut attempts: Vec<SourceAttempt> = Vec::with_capacity(urls.len());
    let mut external: Option<(String, Vec<ExternalProvider>)> = None;
    let mut extra_hints: Vec<(String, Vec<String>)> = Vec::new();

    // Каждая неудачная попытка обязана попасть в журнал ОДНОЙ точкой: до
    // камеры 23.10.2026 «неизвестный формат тела» не логировался никуда,
    // и пользователь не видел в журнале причин провала sync.
    let push_fail = |attempts: &mut Vec<SourceAttempt>, url: &str, detail: String| {
        log::error!("cloud-routers: sync каталога: {url}: {detail}");
        state.events().warn(format!("sync каталога: {url}: {detail}"));
        attempts.push(SourceAttempt { url: url.to_string(), ok: false, detail });
    };

    for url in &urls {
        let (fetched, note) = fetch_source_text_with_fallback(&state, &config, url).await;
        match fetched {
            Err(e) => push_fail(&mut attempts, url, e),
            Ok(text) => {
                // Формат тела определяет классификатор, а не перебор парсеров:
                // перебор «успешно» разбирал чужие тела как мусор (поймано E2E:
                // litellm и models.json давали тысячи «провайдеров» с пустыми
                // моделями, и подсказки терялись).
                let mut push_ok = |url: &str, detail: String| {
                    attempts.push(SourceAttempt { url: url.to_string(), ok: true, detail });
                };
                match classify_source_body(&text) {
                    SourceBodyKind::ModelsDev => match parse_models_dev_body(&text) {
                        Ok(list) => {
                            if external.is_none() {
                                push_ok(url, format!("провайдеров: {}", list.len()));
                                external = Some((url.clone(), list));
                            } else {
                                let hint_count: usize =
                                    list.iter().map(|p| p.models.len()).sum();
                                push_ok(url, format!("модели в подсказки: {hint_count}"));
                                accumulate_models_as_hints(&mut extra_hints, &list);
                            }
                        }
                        Err(e) => push_fail(
                            &mut attempts,
                            url,
                            format!("models.dev-тело не разобралось: {e}"),
                        ),
                    },
                    SourceBodyKind::Litellm => match parse_litellm_hints(&text) {
                        Ok(hints) => {
                            let count: usize = hints.iter().map(|(_, m)| m.len()).sum();
                            push_ok(url, format!("подсказок моделей: {count}"));
                            merge_hints(&mut extra_hints, hints);
                        }
                        Err(e) => push_fail(
                            &mut attempts,
                            url,
                            format!("litellm-тело не разобралось: {e}"),
                        ),
                    },
                    SourceBodyKind::ModelsJson => match parse_models_json_hints(&text) {
                        Ok(hints) => {
                            let count: usize = hints.iter().map(|(_, m)| m.len()).sum();
                            push_ok(url, format!("подсказок моделей: {count}"));
                            merge_hints(&mut extra_hints, hints);
                        }
                        Err(e) => push_fail(
                            &mut attempts,
                            url,
                            format!("models.json-тело не разобралось: {e}"),
                        ),
                    },
                    SourceBodyKind::Unknown => push_fail(&mut attempts, url, unknown_body_reason(&text)),
                }
                // Успешная после фоллбэка попытка несёт в detail своё
                // пояснение: откуда тело, если релей провалился.
                if let Some(note) = note {
                    if let Some(last) = attempts.last_mut() {
                        if last.ok {
                            last.detail = format!("{} ({})", last.detail, note);
                        }
                    }
                }
            }
        }
    }

    let Some((from_url, list)) = external else {
        let summary = attempts
            .iter()
            .map(|a| format!("{}: {}", a.url, a.detail))
            .collect::<Vec<_>>()
            .join("; ");
        log::error!("cloud-routers: sync каталога не удался: {summary}");
        return Err(bad_request(format!(
            "Ни один источник каталога не подошёл ({summary}). Каталог не изменён — шлюз продолжает работать с текущим."
        )));
    };

    let catalog = presets::sources::merge_catalogs(list, &extra_hints, &today_ymd()).map_err(|e| {
        bad_request(format!("Источники дали несобираемый каталог ({e}). Каталог не изменён."))
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
        from_url
    ));
    Ok(Json(PresetsSyncDto {
        source: presets::CatalogSource::Network.as_str(),
        updated_at: catalog.updated_at,
        count: catalog.providers.len(),
        added,
        removed,
        from_url,
        attempts,
    }))
}

/// Модели проигравшего models.dev-источника — в подсказки.
///
/// Второй models.dev-файл даёт тех же провайдеров, и как каталог он не нужен.
/// Но его список моделей идёт в `extra_hints`: лишним не бывает.
fn accumulate_models_as_hints(
    into: &mut Vec<(String, Vec<String>)>,
    list: &[presets::adapters::models_dev::ExternalProvider],
) {
    let hints: Vec<(String, Vec<String>)> = list
        .iter()
        .map(|p| (p.id.clone(), p.models.clone()))
        .filter(|(_, m)| !m.is_empty())
        .collect();
    merge_hints(into, hints);
}

/// Слить подсказки litellm в общий список: дедупликация по id модели.
fn merge_hints(into: &mut Vec<(String, Vec<String>)>, hints: Vec<(String, Vec<String>)>) {
    for (id, models) in hints {
        match into.iter_mut().find(|(eid, _)| eid == &id) {
            Some((_, existing)) => {
                for m in models {
                    if existing.len() >= presets::sources::MODELS_HINT_LIMIT {
                        break;
                    }
                    if !existing.iter().any(|x| x == &m) {
                        existing.push(m);
                    }
                }
            }
            None => into.push((id, models)),
        }
    }
}

/// Сегодняшняя дата `ГГГГ-ММ-ДД` для `updated_at` каталога.
///
/// Без `chrono`: в шлюзе календарных часов нет, а тащить зависимость ради
/// одной строки запрещено минимализмом. Считаем дни от эпохи Unix.
fn today_ymd() -> String {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0);
    // Алгоритм «civil_from_days» Говарда Хиннанта: дни → (год, месяц, день).
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{:02}-{:02}", if m <= 2 { y + 1 } else { y }, m, d)
}

/// Короткий вид ошибки `reqwest`: что именно сломалось.
///
/// Раньше текст ошибки уходил пользователю как есть (`{e}`), и понять, сеть
/// это, TLS, неверный URL или чужой формат, было не по чему — так родилась
/// недиагностируемая «builder error».
fn reqwest_kind(e: &reqwest::Error) -> &'static str {
    if e.is_builder() {
        "неверный URL запроса (builder)"
    } else if e.is_connect() {
        "нет соединения"
    } else if e.is_timeout() {
        "таймаут"
    } else if e.is_request() {
        "ошибка отправки запроса"
    } else if e.is_body() || e.is_decode() {
        "ошибка чтения тела ответа"
    } else if e.is_status() {
        "HTTP-статус ошибки"
    } else {
        "ошибка сети"
    }
}

/// Честная причина «неизвестный формат тела».
///
/// Голый «неизвестный формат» нодекабжу», так был закрыт целый раунд работы
/// (кейс 23.10.2026). Релей, чье URL ведёт не на воркер, отдаёт HTML-страницу
/// SSR приложения — это распознаётся и прямо называется.
fn unknown_body_reason(text: &str) -> String {
    let head = text.trim_start();
    let lower = head.to_ascii_lowercase();
    if lower.starts_with("<!doctype html") || lower.starts_with("<html") {
        "тело — HTML-страница, а не JSON: похоже, URL релея ведёт не на воркер релея (отдаётся фронт приложения). Проверьте/разверните релей".to_string()
    } else {
        let sample: String = text.chars().take(80).collect();
        format!("неизвестный формат тела (начало: «{sample}»)")
    }
}

/// Путь источника каталога для суффикса релей-запроса.
///
/// Источники — полные URL («https://models.dev/api.json»); для обращения к
/// воркеру релея в URL запроса годится только path (возможно с query) —
/// хост и схема уходят в заголовок `x-target-url`. Источник без схемы
/// понимается как уже путь и возвращается как есть.
fn source_path_tail(url: &str) -> String {
    let s = url.trim();
    for scheme in ["https://", "http://"] {
        if let Some(rest) = s.strip_prefix(scheme) {
            return match rest.find('/') {
                Some(idx) => rest[idx..].to_string(),
                None => "/catalog".to_string(),
            };
        }
    }
    s.to_string()
}

/// Скачать тело одного источника каталога.
///
/// Ошибки возвращаются текстом с видом отказа и URL: вызывающий пишет их в
/// `attempts`, а не теряет. Успешный не-2xx — тоже ошибка с кодом статуса.
///
/// `allow_relay = false` — прямая попытка: используется фоллбэком
/// ([`fetch_source_text_with_fallback`]) после неудачи через релей.
async fn fetch_source_text(
    state: &AppState,
    config: &cloud_routers_core::config::GatewayConfig,
    url: &str,
    allow_relay: bool,
) -> Result<String, String> {
    // Каталог тянем через релей, если он настроен: часть источников в
    // некоторых сетях недоступна напрямую.
    //
    // Базу берёт ЕДИНАЯ `relay_base_url`, а не «настроен = иди»: релей без
    // схемы (реальный кейс 22.10.2026, «my-relay.vercel.app» в конфиге) давал
    // scheme-less склейку и reqwest 0.12 `Kind::Builder` («builder error») на
    // КАЖДОМ источнике. Теперь негодная база даёт явную причину с текстом
    // «исправьте поле», а не четыре нулевразумительных «builder error».
    let relay_ready = allow_relay
        && config.relay_url().is_some()
        && config.relay_token().map(|t| !t.trim().is_empty()).unwrap_or(false);
    if relay_ready {
        let relay_url_owned = config.relay_url();
        let relay_raw = relay_url_owned
            .as_deref()
            .filter(|s| !trim_base_url(s).is_empty());
        let relay_url = relay_raw.and_then(cloud_routers_core::config::relay_base_url);
        let relay_url = match relay_url {
            Some(base) => base,
            None => {
                let raw = relay_raw.unwrap_or_default();
                let msg = format!(
                    "{url}: настроенный URL релея «{raw}» не начинается с http:// или https:// — исправьте поле «URL релея» в разделе «Релей»"
                );
                log::error!("cloud-routers: sync каталога: {msg}");
                return Err(msg);
            }
        };
        let relay_token = config
            .relay_token()
            .filter(|t| !t.trim().is_empty())
            .expect("проверено выше");

        // Воркер релея делает ровно один fetch по `x-target-url` и путь запроса
        // НЕ читает (assets/relay-worker.js: линия target → fetch). Прибавлять
        // к префиксу источник ЦЕЛИКОМ нельзя: источники каталога — полные URL
        // («https://models.dev/api.json»), и склейка «gateway» + «https://...»
        // даёт URL-мусор «.../api/gatewayhttps://models.dev/api.json» — боевой
        // кейс 23.10.2026 («нет соединения» на каждом источнике, пользователь
        // поставил схему, а sync всё ещё падал). Как суффикс идёт только
        // ПУТЬ источника — он уникален, читаем и формирует корректный URL
        // endpoint'а воркера.
        let request = state
            .http()
            .get(format!(
                "{}{}{}",
                relay_url,
                crate::upstream::RELAY_PATH_PREFIX,
                source_path_tail(url)
            ))
            .header(crate::upstream::HDR_TARGET_URL, url)
            .header(crate::upstream::HDR_RELAY_TOKEN, relay_token)
            .timeout(std::time::Duration::from_secs(presets::sources::SOURCE_TIMEOUT_SECS));

        let response = request.send().await.map_err(|e| {
            let msg = format!("{}: запрос к релею: {} ({})", url, reqwest_kind(&e), e);
            log::error!("cloud-routers: sync каталога: {msg}");
            msg
        })?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if !status.is_success() {
            let msg = format!("{url}: релей ответил HTTP {}", status.as_u16());
            log::error!("cloud-routers: sync каталога: {msg}");
            return Err(msg);
        }
        // HTTP 200 не обязан значить «источник скачан»: релей, чей URL ведёт не
        // на воркер, сам отвечает фронтовой HTML-страницей SSR (боевой кейс
        // 23.10.2026: 200, text/html, при том что источник напрямую жив). HTML
        // классификатор отбросил бы как Unknown И ПОСЛЕ него фоллбэк напрямую
        // уже не начался бы — поэтому провал релея объявляется именно здесь,
        // до возврата Ok.
        let reason = unknown_body_reason(&text);
        if reason.contains("HTML-страница") {
            let msg = format!("{url}: {reason}");
            log::error!("cloud-routers: sync каталога: {msg}");
            return Err(msg);
        }
        return Ok(text);
    }

    // Прямая попытка (родной путь sync, когда релей не настроен или фоллбэк).
    let response = state
        .http()
        .get(url)
        .timeout(std::time::Duration::from_secs(presets::sources::SOURCE_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| {
            let msg = format!("{}: {} ({})", url, reqwest_kind(&e), e);
            log::error!("cloud-routers: sync каталога: {msg}");
            msg
        })?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        let msg = format!("{url}: источник ответил HTTP {}", status.as_u16());
        log::error!("cloud-routers: sync каталога: {msg}");
        return Err(msg);
    }
    Ok(text)
}

/// Тело источника: попытка через релей (если настроен), затем напрямую.
///
/// Результат — `Ok(тело)` + `Option<пояснение>` успех-сценария (заполняется,
/// когда через релей не вышло, а напрямую сработало) или `Err(полная причина)`
/// с обеими попытками. Пояснение попадает в `attempts` панели, причина —
/// дополнительно в журнал — silent-fail каталога был поводом целого раунда
/// работы (кейс 23.10.2026), у любого отказа记账 обязано быть лицо.
async fn fetch_source_text_with_fallback(
    state: &AppState,
    config: &cloud_routers_core::config::GatewayConfig,
    url: &str,
) -> (Result<String, String>, Option<String>) {
    let relay_ready = config.relay_url().is_some()
        && config.relay_token().map(|t| !t.trim().is_empty()).unwrap_or(false);

    if !relay_ready {
        return (fetch_source_text(state, config, url, false).await, None);
    }

    match fetch_source_text(state, config, url, true).await {
        Ok(text) => (Ok(text), None),
        Err(relay_err) => match fetch_source_text(state, config, url, false).await {
            Ok(text) => {
                let note = format!("через релей не вышло ({relay_err}); взято напрямую");
                log::warn!("cloud-routers: sync каталога: {url}: {note}");
                (Ok(text), Some(note))
            }
            Err(direct_err) => {
                let cause = format!("{relay_err}; попытка напрямую тоже не вышла: {direct_err}");
                log::error!("cloud-routers: sync каталога: {url}: {cause}");
                (Err(cause), None)
            }
        },
    }
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
    /// Токен платформы: Vercel или Cloudflare — что именно, задаёт `kind`.
    /// (Имя поля сохранено `vercel_token` ради совместимости панели.)
    vercel_token: String,
    /// Vercel: имя проекта; в Cloudflare-пути не используется.
    project: Option<String>,
    /// Cloudflare: Account ID, если токен видит несколько аккаунтов.
    account_id: Option<String>,
    /// Cloudflare: имя воркера (`https://{script}.{sub}.workers.dev`).
    script: Option<String>,
    /// "vercel" (по умолчанию, совместимая панель) | "cloudflare".
    kind: Option<String>,
}

async fn relay_deploy(State(state): State<AppState>, Json(body): Json<DeployBody>) -> ApiResult<Json<serde_json::Value>> {
    let result: relay::DeployResult = match body.kind.as_deref().unwrap_or(RELAY_KIND_VERCEL) {
        RELAY_KIND_CLOUDFLARE => {
            // CF-путь сам пишет свой слот и делает его активным (relay_cf.rs).
            relay_cf::deploy_relay_cf(&state, &body.vercel_token, body.account_id.as_deref(), body.script.as_deref())
                .await
                .map_err(|e| bad_request(e))?
        }
        _ => {
            let result = relay::deploy_relay(&state, &body.vercel_token, body.project.as_deref())
                .await
                .map_err(|e| bad_request(e))?;
            // Развёрнутый URL сразу сохраняем: пользователь не должен копировать
            // его руками и не должен получить «получилось, но в поле пусто».
            // Развернули — и СДЕЛАЛИ АКТИВНЫМ: панель не заставляет пользователя
            // после деплоя искать ещё одну кнопку.
            let url = result.url.clone();
            state
                .update_config(|c| {
                    c.migrate_relay_slots();
                    let activated = c.vercel_slot_mut().map(|slot| {
                        slot.url = Some(url.clone());
                        slot.id.clone()
                    });
                    match activated {
                        Some(id) => c.active_relay = Some(id),
                        None => c.vercel_relay_url = Some(url),
                    }
                    Ok(())
                })
                .map_err(|e| internal(e))?;
            result
        }
    };
    notify_host(&state, "relay-deployed");
    Ok(Json(serde_json::to_value(result).map_err(|e| internal(e.to_string()))?))
}

async fn relay_test(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let probe = relay::probe_relay(&state).await.map_err(|e| bad_request(e))?;
    Ok(Json(serde_json::to_value(probe).map_err(|e| internal(e.to_string()))?))
}

// ────────────────────────────── Слоты релея ──────────────────────────────

/// Слот релея для панели.
///
/// `url` — не секрет: это публичный адрес воркера. Секрет отдаётся только
/// флагом «задан» — по тому же правилу «секреты маскируются», что и в
/// `ConfigDto`.
#[derive(Debug, Serialize)]
struct RelaySlotDto {
    id: String,
    kind: String,
    url: Option<String>,
    relay_secret_set: bool,
    cf_account_id: Option<String>,
    cf_script_name: Option<String>,
    active: bool,
}

/// Список слотов релея + активный путь доставки.
async fn relay_slots_list(State(state): State<AppState>) -> Json<serde_json::Value> {
    let config = state.config();
    let slots: Vec<RelaySlotDto> = config
        .relays
        .iter()
        .map(|s| RelaySlotDto {
            id: s.id.clone(),
            kind: s.kind.clone(),
            url: s.url.clone(),
            // «Задан» = непустая непробельная строка: воркер сравнивает
            // заголовок посимвольно, пробельный секрет отклонял бы всё 401.
            relay_secret_set: s
                .relay_secret
                .as_deref()
                .map(|t| !t.trim().is_empty())
                .unwrap_or(false),
            cf_account_id: s.cf_account_id.clone(),
            cf_script_name: s.cf_script_name.clone(),
            active: config.active_relay.as_deref() == Some(s.id.as_str()),
        })
        .collect();
    Json(serde_json::json!({ "slots": slots, "active": config.active_relay }))
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RelaySlotIdBody {
    id: String,
}

/// Переключить активный путь доставки на слот.
async fn relay_slot_activate(
    State(state): State<AppState>,
    Json(body): Json<RelaySlotIdBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let id = body.id.clone();
    let exists = state.config().relays.iter().any(|s| s.id == id);
    if !exists {
        return Err(not_found(format!("Слот релея «{}» не найден", id)));
    }
    state
        .update_config(move |c| {
            if c.relays.iter().any(|s| s.id == id) {
                c.active_relay = Some(id.clone());
            }
            Ok(())
        })
        .map_err(|e| internal(e))?;
    notify_host(&state, "relay-slot-activated");
    Ok(Json(serde_json::json!({ "ok": true, "active": body.id })))
}

/// Удалить слот.
///
/// Удаление АКТИВНОГО слота снимает его с активного пути (`active_relay: None`):
/// доставка честно перестаёт работать, а не продолжает указывать на
/// несуществующий релей.
async fn relay_slot_delete(
    State(state): State<AppState>,
    Json(body): Json<RelaySlotIdBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let id = body.id.clone();
    let exists = state.config().relays.iter().any(|s| s.id == id);
    if !exists {
        return Err(not_found(format!("Слот релея «{}» не найден", id)));
    }
    state
        .update_config(move |c| {
            c.relays.retain(|s| s.id != id);
            if c.active_relay.as_deref() == Some(id.as_str()) {
                c.active_relay = None;
            }
            Ok(())
        })
        .map_err(|e| internal(e))?;
    notify_host(&state, "relay-slot-deleted");
    Ok(Json(serde_json::json!({ "ok": true })))
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

/// Тело `POST /api/ui-error`: отчёт дашборда об ошибке JS/сети.
#[derive(Debug, Deserialize)]
struct UiErrorBody {
    /// Где случилось (имя функции или `window.onerror`). Ключевое слово Rust,
    /// поэтому поле переименовано через serde.
    #[serde(default, rename = "where")]
    where_text: String,
    #[serde(default)]
    message: String,
}

/// Отчёт об ошибке JS дашборда → RAM-журнал шлюза.
///
/// Сетевой отказ fetch не порождает HTTP-ответа, серверная middleware его не
/// видит — до 25.10.2026 такой отказ дашборда («отказано в подключении») не
/// попадал ни в журнал, ни в логи хоста. Дашборд репортит сюда сам
/// (`utils.js reportUiError`), записи уходят зеркалом (`gateway_events.rs`)
/// в лог приложения с префиксом `gateway:`.
///
/// Кривое/пустое тело — 400 по контракту `check_keys_gemini`: молчаливый
/// откат к дефолту маскировал бы сломанного репортёра.
async fn report_ui_error(State(state): State<AppState>, body: axum::body::Bytes) -> Response {
    if body.is_empty() {
        return bad_request("тело запроса — ожидается JSON {\"where\": \"…\", \"message\": \"…\"}");
    }
    let parsed: UiErrorBody = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return bad_request(format!(
                "тело запроса — ожидается JSON {{\"where\": \"…\", \"message\": \"…\"}}: {e}"
            ));
        }
    };
    let where_text = parsed.where_text.trim();
    let message = parsed.message.trim();
    if message.is_empty() {
        return bad_request("поле message обязательно");
    }
    // Хвост как у journal-строк проверки ключей (≤300): stack-trace целиком
    // журналу не нужен, полезные события он вытеснять не должен.
    state.events().push(
        "warn",
        format!("ui: {}: {}", where_text, tail_chars(message, 300)),
    );
    StatusCode::OK.into_response()
}

/// Хвост строки длиной ≤ `max` символов (граница символов, не байт —
/// сообщения дашборда содержат кириллицу).
fn tail_chars(s: &str, max: usize) -> &str {
    let total = s.chars().count();
    if total <= max {
        return s;
    }
    let skip = total - max;
    s.char_indices()
        .nth(skip)
        .map(|(i, _)| &s[i..])
        .unwrap_or("")
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

/// Остановить шлюз — процесс завершает **сам себя**.
///
/// Задача «убить чужой процесс» на Windows упирается в политику
/// TerminateProcess: процесс, созданный повышенным (админским) хостом, нельзя
/// остановить из не-повышенного (реальный кейс 22.10.26: taskkill по пиду
/// старой копии — «Отказано в доступе» ×3). Задача «процесс сам себя закрывает»
/// этих прав не требует вовсе, поэтому остановка хостом идёт через этот
/// эндпойнт как первый шаг, а taskkill остаётся запасом на зависший процесс.
///
/// Ответ отдаётся сразу; выход — через 300 мс, чтобы TCP-ответ и строка
/// event-лога успели уйти. `clear_server_record` перед `exit(0)` обязателен:
/// exit не крутит деструкторы, и `server.json` иначе остался бы сиротой.
async fn shutdown(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    state.events().warn("запрошена остановка шлюза по HTTP");
    let data_dir = state.data_dir().to_path_buf();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        core_store::clear_server_record(&data_dir);
        log::info!("шлюз завершает себя по запросу /api/shutdown");
        std::process::exit(0);
    });
    Ok(Json(serde_json::json!({ "status": "shutting_down" })))
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

/// Тело запроса проверки ключей. Оба поля опциональны: дашборд шлёт тело
/// только когда пользователь выбрал модели.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct CheckKeysBody {
    /// Модели, по которым проверить ключи (санитайзит `key_check`).
    models: Vec<String>,
}

/// Проверить все активные Gemini-ключи провайдера через `:countTokens`
/// (лимит генерации не расходуется). Логику исполняет [`crate::key_check`],
/// обработчик — только разбор зависимостей и код ответа: диагностическая
/// причина («не Gemini», «релей без URL») приходит в тост дашборда как 400.
/// Тело опционально: `{"models": [...]}` — проверка по выбранным моделям,
/// пустое тело — по дефолтному кандидату провайдера.
async fn check_keys_gemini(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: axum::body::Bytes,
) -> ApiResult<Json<serde_json::Value>> {
    // Bytes не отклоняет запрос без тела (в отличие от Json) — пусто значит
    // «без явного списка»; кривой JSON — 400 с читаемой причиной, а не
    // молчаливый откат к дефолту.
    let requested = if body.is_empty() {
        None
    } else {
        let parsed: CheckKeysBody = serde_json::from_slice(&body).map_err(|e| {
            bad_request(format!(
                "тело запроса — ожидается JSON {{\"models\": [\"…\"]}}: {e}"
            ))
        })?;
        Some(parsed.models)
    };
    let Some(provider) = state.config().provider(&id).cloned() else {
        return Err(not_found(format!("Провайдер «{}» не найден", id)));
    };
    let summary = key_check::check_provider_keys(&state, &provider, requested.as_deref())
        .await
        .map_err(bad_request)?;
    Ok(Json(serde_json::to_value(&summary).expect("сводка проверки сериализуется")))
}
/// Собрать все маршруты управления (используется в [`crate::server`]).
///
/// Единый механизм «админ-отказ в журнале» (23.10.2026): middleware ниже ловит
/// ответ 4xx/5xx любого маршрута и пишет причину в журнал событий (и дальше —
/// в `gateway.log` через `EventLog::push`). Причина формируется в хендлерах
/// (`err/bad_request/not_found/internal`), дублировать `log::!` в каждом из них
/// не нужно.
#[allow(clippy::needless_pass_by_value)]
pub fn router(state: AppState) -> axum::Router<AppState> {
    // Синтаксис параметров — axum 0.7 (`:id`): с Cargo.lock 0.7.9 формы
    // `{id}` не захватываются (matchit 0.7 трактует их как обычные литеральные
    // сегменты), маршруты не совпадают и дашборд получает пустую 404 на всякое
    // действие над конкретным провайдером. Регресс: tests::provider_param_routes_use_axum07_syntax.
    axum::Router::new()
        .route("/api/status", axum::routing::get(status))
        .route("/api/health", axum::routing::get(health_check))
        .route("/api/config", axum::routing::get(get_config).put(put_config))
        .route("/api/config/reload", axum::routing::post(reload_config))
        .route("/api/providers", axum::routing::post(add_provider))
        .route("/api/providers/:id", axum::routing::patch(patch_provider).delete(delete_provider))
        .route("/api/providers/:id/keys", axum::routing::post(add_keys))
        .route(
            "/api/providers/:id/keys/:key_id",
            axum::routing::delete(delete_key),
        )
        .route(
            "/api/providers/:id/keys/:key_id/reset",
            axum::routing::post(reset_key),
        )
        .route(
            "/api/providers/:id/keys/check",
            axum::routing::post(check_keys_gemini),
        )
        .route("/api/providers/:id/models/refresh", axum::routing::post(refresh_models))
        .route("/api/providers/:id/test", axum::routing::post(test_provider))
        .route("/api/providers/:id/oauth/start", axum::routing::post(oauth_start))
        .route("/api/providers/:id/oauth/poll", axum::routing::post(oauth_poll))
        .route("/api/providers/:id/oauth", axum::routing::get(oauth_status))
        .route("/api/providers/:id/oauth/disconnect", axum::routing::post(oauth_disconnect))
        .route("/api/presets", axum::routing::get(get_presets))
        .route("/api/presets/sync", axum::routing::post(sync_presets))
        .route("/api/relay/worker", axum::routing::get(relay_worker))
        .route("/api/relay/token", axum::routing::get(relay_token))
        .route("/api/relay/deploy", axum::routing::post(relay_deploy))
        .route("/api/relay/test", axum::routing::post(relay_test))
        .route("/api/relay/slots", axum::routing::get(relay_slots_list))
        .route("/api/relay/slots/activate", axum::routing::post(relay_slot_activate))
        .route("/api/relay/slots/delete", axum::routing::post(relay_slot_delete))
        .route("/api/events", axum::routing::get(get_events))
        .route("/api/ui-error", axum::routing::post(report_ui_error))
        .route("/api/restart", axum::routing::post(restart))
        .route("/api/shutdown", axum::routing::post(shutdown))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            log_admin_errors,
        ))
}

/// Миддлварь журнала отказов admin-API.
///
/// Без неё причины ошибок (`{ «error»: «…» }`) умирали бы только в HTTP-ответе:
/// дашборд показал тост и всё, журналы (`gateway.log`, вкладка событий) молчали.
/// `/api/events` исключён — это сам журнал: писать в журнал о чтении журнала —
/// бесконечный feedback-цикл.
async fn log_admin_errors(
    State(state): State<AppState>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let path = req.uri().path().to_owned();
    let method = req.method().to_string();
    let response = next.run(req).await;
    let status = response.status().as_u16();
    if status < 400 || path == "/api/events" {
        return response;
    }
    let (parts, body) = response.into_parts();
    // Тело ошибки маскируется в точке формирования (см. доку модуля); здесь
    // только ограничение размера: наш JSON ошибок короткий.
    let bytes = match axum::body::to_bytes(body, 4096).await {
        Ok(b) => b,
        Err(_) => axum::body::Bytes::new(),
    };
    let reason: String = String::from_utf8_lossy(&bytes).trim().chars().take(300).collect();
    state
        .events()
        .error(format!("{} {} → {}: {}", method, path, status, reason));
    // Отдаём клиенту ОРИГИНАЛЬНОЕ тело (не обрезанное): обрезка — только для журнала.
    Response::from_parts(parts, axum::body::Body::from(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use axum::response::IntoResponse;
    use cloud_routers_core::config::GatewayConfig;
    use tower::ServiceExt;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("cr_gw_admin_{tag}"));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    /// Регрессия 23.10.2026: параметризованные маршруты регистрировались в
    /// синтаксисе axum 0.8 (`/api/providers/{id}`) при закреплённой axum 0.7
    /// (Cargo.lock 0.7.9), где захват пишется как `:id`. В 0.7 `{id}` — обычный
    /// литеральный сегмент, маршруты не совпадали, и весь блок «провайдер +
    /// вложенные действия» дашборда (PATCH провайдера, добавление/удаление
    /// ключей, refresh моделей, проверка связи, OAuth) отдавал дефолтный 404
    /// с ПУСТЫМ телом — юзер видел «Не удалось сохранить: HTTP 404» на чекбоксе
    /// «через релей». Тест прогоняет PATCH и POST-ключи через полный роутер:
    /// если кто-то вернёт `{id}`, оба запроса упрутся в 404 и тест упадёт сразу.
    #[tokio::test]
    async fn provider_param_routes_use_axum07_syntax() {
        let dir = tmp("axum07params");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let mut cfg = GatewayConfig::default();
        cfg.providers.push(ProviderConfig {
            id: "mock".into(),
            name: "Mock".into(),
            base_url: "https://upstream.example/v1".into(),
            keys: vec![AccountKey {
                id: "k1".into(),
                key: "sk-1".into(),
                is_active: true,
                name: None,
            }],
            ..Default::default()
        });
        core_store::save(&dir, &cfg).expect("save");
        let state = AppState::new(&dir).expect("state");
        let app = crate::server::build(state);

        // PATCH провайдера — тот самый чекбокс «через релей» (use_relay).
        let r = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::PATCH)
                    .uri("/api/providers/mock")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"use_relay":true}"#))
                    .expect("req"),
            )
            .await
            .expect("response");
        assert_eq!(
            r.status(),
            StatusCode::OK,
            "PATCH /api/providers/:id обязан проходить (см. комментарий к тесту)"
        );
        let saved = cloud_routers_core::load_config(&dir);
        assert!(
            saved.providers[0].use_relay,
            "флаг «через релей» обязан примениться в конфиге"
        );

        // POST ключей — второй параметризованный маршрут того же блока.
        let r2 = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/api/providers/mock/keys")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"keys":["sk-new-1"]}"#))
                    .expect("req"),
            )
            .await
            .expect("response");
        assert_eq!(
            r2.status(),
            StatusCode::OK,
            "POST /api/providers/:id/keys обязан проходить"
        );
        let saved2 = cloud_routers_core::load_config(&dir);
        assert!(
            saved2.providers[0].keys.iter().any(|k| k.key == "sk-new-1"),
            "ключ обязан добавиться к провайдеру"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 23.10.26 — ключи с именами: новый формат `{name, secret}`, старый
    /// плоский список продолжает работать, нумерация продолжается от
    /// существующих ключей, дубликат секрета (с именем или без) не плодит
    /// запись и не перезаписывает имя существующего ключа.
    #[tokio::test]
    async fn add_keys_accepts_named_entries_and_legacy_plain_list() {
        let dir = tmp("keynames");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let mut cfg = GatewayConfig::default();
        cfg.providers.push(ProviderConfig {
            id: "mock".into(),
            name: "Mock".into(),
            base_url: "https://upstream.example/v1".into(),
            keys: vec![AccountKey { id: "k1".into(), key: "sk-existing".into(), is_active: true, name: None }],
            ..Default::default()
        });
        core_store::save(&dir, &cfg).expect("save");
        let state = AppState::new(&dir).expect("state");
        let app = crate::server::build(state);

        // Новый и старый элементы вперемешку: объекты с именами, плоская
        // строка, объект с именем из одних пробелов.
        let body = "{\"keys\":[{\"name\":\"mihael2\",\"secret\":\"AQ.Ab8-1\"},\
                    {\"name\":\"  maredon3  \",\"secret\":\"AQ.Ab8-2\"},\
                    \"AQ.Ab8-3\",{\"name\":\"   \",\"secret\":\"AQ.Ab8-4\"}]}";
        let r = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/api/providers/mock/keys")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .expect("req"),
            )
            .await
            .expect("response");
        assert_eq!(
            r.status(),
            StatusCode::OK,
            "именованные и плоские ключи обязаны приниматься"
        );
        let saved = cloud_routers_core::load_config(&dir);
        let keys = &saved.providers[0].keys;
        assert_eq!(keys.len(), 5, "существующий + 4 добавленных");
        let by_secret = |s: &str| keys.iter().find(|k| k.key == s).expect("key");
        assert_eq!(by_secret("AQ.Ab8-1").name.as_deref(), Some("mihael2"));
        assert_eq!(by_secret("AQ.Ab8-2").name.as_deref(), Some("maredon3"), "имя обрезается");
        assert_eq!(by_secret("AQ.Ab8-3").name, None, "плоская строка — без имени");
        assert_eq!(by_secret("AQ.Ab8-4").name, None, "пустое имя схлопывается в None");
        // Нумерация продолжается от существующих ключей.
        assert_eq!(by_secret("AQ.Ab8-1").id, "k2");
        assert_eq!(by_secret("AQ.Ab8-4").id, "k5");

        // Дубликат секрета с новым именем не плодит запись и не меняет имя.
        let dup = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/api/providers/mock/keys")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"keys":[{"name":"again","secret":"AQ.Ab8-1"}]}"#))
                    .expect("req"),
            )
            .await
            .expect("response");
        assert_eq!(dup.status(), StatusCode::BAD_REQUEST, "все переданные уже добавлены");
        let saved2 = cloud_routers_core::load_config(&dir);
        assert_eq!(saved2.providers[0].keys.len(), 5, "дубликат не добавлен");
        assert_eq!(
            saved2.providers[0]
                .keys
                .iter()
                .find(|k| k.key == "AQ.Ab8-1")
                .and_then(|k| k.name.as_deref()),
            Some("mihael2"),
            "имя существующего ключа не перезаписывается дубликатом"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 23.10.26 — имена ключей доходят до дашборда через /api/config, а секреты
    /// по-прежнему только в масках.
    #[tokio::test]
    async fn provider_config_dto_exposes_key_names_without_secrets() {
        let dir = tmp("keydto");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let mut cfg = GatewayConfig::default();
        cfg.providers.push(ProviderConfig {
            id: "google".into(),
            name: "Google".into(),
            base_url: "https://generativelanguage.googleapis.com/v1beta/openai".into(),
            keys: vec![AccountKey {
                id: "k1".into(),
                key: "AQ.Ab8RN6-secret-value".into(),
                is_active: true,
                name: Some("mihael2".into()),
            }],
            ..Default::default()
        });
        core_store::save(&dir, &cfg).expect("save");
        let state = AppState::new(&dir).expect("state");
        let resp = {
            let app = crate::server::build(state);
            app.oneshot(Request::builder().uri("/api/config").body(Body::empty()).expect("req"))
                .await
                .expect("response")
        };
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(
            v["providers"][0]["keys"][0]["name"],
            "mihael2",
            "имя ключа обязано дойти до дашборда: {v}"
        );
        assert!(v["providers"][0]["keys"][0]["masked"].is_string(), "маска обязана быть");
        let text = serde_json::to_string(&v).expect("text");
        assert!(
            !text.contains("AQ.Ab8RN6-secret-value"),
            "живой секрет не имеет права утекать в /api/config:\n{text}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn provider_auth_dto_serializes_oauth_kind_for_dashboard() {
        let dto = ProviderAuthDto::OAuth {
            session_id: "s1".into(),
            session_present: true,
            method: None,
            email: None,
            expires_at_unix: 0,
            refreshable: false,
        };
        let v = serde_json::to_value(&dto).expect("json");
        // Дашборд проверяет `p.auth.kind === 'o_auth'`: строка обязана совпадать.
        assert_eq!(v["kind"], "o_auth", "{v}");
        let dto = ProviderAuthDto::ApiKeys;
        assert_eq!(serde_json::to_value(&dto).expect("json")["kind"], "api_keys");
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
                name: None,
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
            Json(ConfigPatch {
                presets_sources: Some(vec!["ftp://x".into()]),
                ..Default::default()
            }),
        )
        .await;
        assert!(r.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn put_config_rejects_relay_url_without_scheme() {
        // Кейс 22.10.2026: релей «my-relay.vercel.app» сохранялся как настроенный
        // и ломал каждый запрос каталога «builder error». Сохранение обязано
        // отказать с понятной причиной — негодный релей не должен попасть
        // в конфиг и, значит, не может повредить sync/маршрутизацию.
        let dir = tmp("relayurl");
        let state = AppState::new(&dir).expect("state");
        let r = put_config(
            State(state.clone()),
            Json(ConfigPatch {
                vercel_relay_url: Some("my-relay.vercel.app".into()),
                ..Default::default()
            }),
        )
        .await;
        assert!(r.is_err(), "без http(s):// сохранение обязано упасть");
        // Значение не должно просочиться в конфиг после отказа.
        assert_eq!(state.config().vercel_relay_url, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn put_config_accepts_sources_list_and_resets_on_empty() {
        let dir = tmp("presetssrc");
        let state = AppState::new(&dir).expect("state");

        let patch = Json(ConfigPatch {
            presets_sources: Some(vec![
                "https://example.com/a.json ".into(),
                "https://example.com/a.json".into(),
            ]),
            ..Default::default()
        });
        let _ = put_config(State(state.clone()), patch).await.expect("принять список");
        assert_eq!(
            cloud_routers_core::load_config(&dir).presets_sources,
            vec!["https://example.com/a.json".to_string()],
            "пробелы обрезаются, дубликаты схлопываются"
        );

        let patch =
            Json(ConfigPatch { presets_sources: Some(vec![]), ..Default::default() });
        let _ = put_config(State(state.clone()), patch).await.expect("сброс к дефолтам");
        let cfg = cloud_routers_core::load_config(&dir);
        assert!(cfg.presets_sources.is_empty());
        assert_eq!(
            cfg.effective_presets_sources()[0],
            cloud_routers_core::presets::sources::MODELS_DEV_API
        );
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

        let _ = put_config(
            State(state.clone()),
            Json(ConfigPatch { vercel_relay_token: Some("  my-secret  ".into()), ..Default::default() }),
        )
        .await
        .expect("принять секрет");
        assert_eq!(
            cloud_routers_core::load_config(&dir).relay_token().as_deref(),
            Some("my-secret"),
            "секрет должен сохраниться без краёв пробелов"
        );

        let _ = put_config(
            State(state.clone()),
            Json(ConfigPatch { vercel_relay_token: Some("   ".into()), ..Default::default() }),
        )
        .await
        .expect("сброс секрета");
        assert_eq!(
            cloud_routers_core::load_config(&dir).relay_token(),
            None,
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
        assert!(state.config().relay_token().is_none());

        let r = relay_token(State(state)).await.expect("секрет выдаётся");
        let bytes = axum::body::to_bytes(r.into_response().into_body(), usize::MAX).await.expect("body");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        let token = v["token"].as_str().expect("token");
        assert_eq!(token.len(), 64, "ожидался 64 hex-символа: {}", token);
        assert_eq!(
            cloud_routers_core::load_config(&dir).relay_token().as_deref(),
            Some(token),
            "выданный секрет обязан сохраниться, а не жить только в ответе"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Активация слота обязана переключить путь доставки: геттер
    /// `relay_url()` читает АКТИВНЫЙ слот, а не legacy/первый попавшийся.
    /// Это контракта панели: кнопка «Сделать активным» реально переключает,
    /// а не только красит карточку.
    #[tokio::test]
    async fn relay_slot_activate_switches_active_relay() {
        let dir = tmp("slots_activate");
        let state = AppState::new(&dir).expect("state");
        state
            .update_config(|c| {
                c.relays.push(cloud_routers_core::config::RelaySlot {
                    id: "vercel".into(),
                    kind: "vercel".into(),
                    url: Some("https://vercel-relay.vercel.app".into()),
                    ..Default::default()
                });
                c.relays.push(cloud_routers_core::config::RelaySlot {
                    id: "cloudflare".into(),
                    kind: "cloudflare".into(),
                    url: Some("https://cf-relay.workers.dev".into()),
                    ..Default::default()
                });
                c.active_relay = Some("vercel".into());
                Ok(())
            })
            .expect("seed");

        let r = relay_slot_activate(
            State(state.clone()),
            Json(RelaySlotIdBody { id: "cloudflare".into() }),
        )
        .await
        .expect("activate ok");
        let bytes = axum::body::to_bytes(r.into_response().into_body(), usize::MAX).await.expect("body");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(v["active"], "cloudflare");

        assert_eq!(
            state.config().relay_url().as_deref(),
            Some("https://cf-relay.workers.dev"),
            "после активации доставка идёт через активированный слот"
        );
        assert_eq!(
            cloud_routers_core::load_config(&dir).active_relay.as_deref(),
            Some("cloudflare"),
            "активация обязана пережить перезапуск шлюза"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Активация неизвестного слота — 404 (и middleware это событие
    /// свалится в журнал отказов), конфиг не трогается.
    #[tokio::test]
    async fn relay_slot_activate_unknown_id_is_404() {
        let dir = tmp("slots_unknown");
        let state = AppState::new(&dir).expect("state");
        let r = relay_slot_activate(
            State(state.clone()),
            Json(RelaySlotIdBody { id: "nope".into() }),
        )
        .await
        .expect_err("must be 404");
        assert_eq!(r.into_response().status(), StatusCode::NOT_FOUND);
        assert!(state.config().active_relay.is_none(), "неудачная активация не должна менять конфиг");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Удаление активного слота снимает его с активного пути: доставка
    /// честно пуста, а не указывает на несуществующий релей.
    #[tokio::test]
    async fn relay_slot_delete_removes_and_deactivates() {
        let dir = tmp("slots_delete");
        let state = AppState::new(&dir).expect("state");
        state
            .update_config(|c| {
                c.relays.push(cloud_routers_core::config::RelaySlot {
                    id: "cloudflare".into(),
                    kind: "cloudflare".into(),
                    url: Some("https://cf-relay.workers.dev".into()),
                    ..Default::default()
                });
                c.active_relay = Some("cloudflare".into());
                Ok(())
            })
            .expect("seed");

        let _ = relay_slot_delete(
            State(state.clone()),
            Json(RelaySlotIdBody { id: "cloudflare".into() }),
        )
        .await
        .expect("delete ok");

        let cfg = state.config();
        assert!(cfg.relays.is_empty(), "слот должен быть удалён");
        assert!(cfg.active_relay.is_none(), "активный указатель обязан был сняться");
        assert_eq!(
            cfg.relay_url(),
            None,
            "удалённый слот больше не обслуживает доставку (а legacy без URL — тем более)"
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
                keys: vec![KeySpec::Plain("gsk-1".into())],
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
                keys: vec![KeySpec::Plain("gsk-1".into())],
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
                keys: vec![KeySpec::Plain("gsk-2".into())],
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
            keys: vec![AccountKey { id: "k1".into(), key: "gsk-1".into(), is_active: true, name: None }],
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
            keys: vec![AccountKey { id: "k1".into(), key: "gsk-secret".into(), is_active: true, name: None }],
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
            keys: vec![AccountKey { id: "k1".into(), key: "gsk-same".into(), is_active: true, name: None }],
            ..Default::default()
        });
        core_store::save(&dir, &cfg).expect("save");
        let state = AppState::new(&dir).expect("state");

        let r = add_keys(
            State(state),
            Path("groq".into()),
            Json(NewKeys { keys: vec![KeySpec::Plain("gsk-same".into())] }),
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
        cfg.presets_sources = vec!["http://127.0.0.1:1/none.json".into()];
        core_store::save(&dir, &cfg).expect("save");
        let state = AppState::new(&dir).expect("state");

        let r = sync_presets(State(state)).await;
        let err_resp = r.expect_err("sync обязан упасть").into_response();
        let bytes =
            axum::body::to_bytes(err_resp.into_body(), usize::MAX).await.expect("body");
        let err_text = String::from_utf8_lossy(&bytes);
        assert!(err_text.contains("127.0.0.1:1"), "ошибка обязана назвать виновный URL: {err_text}");
        assert!(
            err_text.contains("нет соединения") || err_text.contains("таймаут"),
            "ошибка обязана назвать вид отказа, а не «builder error»: {err_text}"
        );
        let (catalog, source) = presets::load(&dir);
        assert!(catalog.providers.len() >= 20);
        assert_eq!(source, presets::CatalogSource::Embedded);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn source_path_tail_strips_scheme_and_host_keeps_path_and_query() {
        // Боевой кейс 23.10.2026: склейка gateway + ПОЛНЫЙ URL источника
        // давала «.../api/gatewayhttps://models.dev/api.json» (нет соединения).
        assert_eq!(source_path_tail("https://models.dev/api.json"), "/api.json");
        assert_eq!(
            source_path_tail("https://raw.githubusercontent.com/BerriAI/litellm/main/prices.json?v=2"),
            "/BerriAI/litellm/main/prices.json?v=2"
        );
        assert_eq!(source_path_tail("http://127.0.0.1:8808/x.json"), "/x.json");
        // Уже путь — вернуться без изменений; адрес без пути — заглушка.
        assert_eq!(source_path_tail("/local.json"), "/local.json");
        assert_eq!(source_path_tail("https://models.dev"), "/catalog");
    }

    #[tokio::test]
    async fn sync_with_schemeless_relay_names_the_field_not_builder() {
        // Кейс 22.10.2026: релей настроен («my-relay.vercel.app», token есть),
        // и каждый источник отвечал «неверный URL запроса (builder) (builder
        // error)» — ноль диагностики. Негодный релей обязан дать отказ до
        // сети с текстом, где чинить.
        let dir = tmp("syncrelay");
        std::fs::create_dir_all(&dir).expect("mkdir");
        // Пишем сырой JSON в обход save: save теперь сам валидирует
        // (invalid config больше невозможен через штатный путь), а кейс
        // воспроизводит именно СТАРЫЙ конфиг на диске, подпорченный раньше.
        std::fs::write(
            core_store::config_path(&dir),
            r#"{"port":20131,"providers":[],"vercel_relay_url":"my-relay.vercel.app","vercel_relay_token":"tok","presets_sources":["http://127.0.0.1:1/none.json"]}"#,
        )
        .expect("write");
        let state = AppState::new(&dir).expect("state");

        let r = sync_presets(State(state)).await;
        let err_resp = r.expect_err("sync обязан упасть").into_response();
        let bytes =
            axum::body::to_bytes(err_resp.into_body(), usize::MAX).await.expect("body");
        let err_text = String::from_utf8_lossy(&bytes);
        assert!(
            err_text.contains("my-relay.vercel.app") && err_text.contains("http:// или https://"),
            "ошибка обязана называть поле релея и нужную схему: {err_text}"
        );
        assert!(!err_text.contains("builder"), "{err_text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Заглушка релея, у которого URL указывает не на воркер: ВСЕ пути
    /// отвечают 200 text/html страницей SSR приложения (как реальный кейс
    /// 23.10.2026: `data-dpl-id` Vercel-страницы на под-пути воркера).
    async fn fake_relay_fallback() -> &'static str {
        "<!DOCTYPE html><html data-dpl-id=\"dpl_test\"><body>next.js page</body></html>"
    }

    #[tokio::test]
    async fn relay_html_falls_back_to_direct_and_syncs() {
        // Кейс 23.10.2026: релей пользователя отдал 200 с HTML-страницей (URL
        // ведёт не на воркер релея), при этом источник каталога доступен
        // напрямую. Sync обязан опознать HTML как провал релей-попытки, взять
        // тело напрямую и отчитаться в attempts ОБЕИМИ ветками, а не
        // упасть с «неизвестный формат тела».
        let (relay_base, source_base) = {
            let html_router = axum::Router::new().fallback(fake_relay_fallback);
            let rl = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind relay");
            let relay_port = rl.local_addr().expect("relay addr").port();
            tokio::spawn(async move { let _ = axum::serve(rl, html_router).await; });

            let src_router = axum::Router::new().route(
                "/api.json",
                axum::routing::get(|| async {
                    r#"{"groq":{"name":"Groq","api":"https://api.groq.example/openai/v1","models":{"llama-3":{}}}}"#
                }),
            );
            let sl = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind source");
            let src_port = sl.local_addr().expect("source addr").port();
            tokio::spawn(async move { let _ = axum::serve(sl, src_router).await; });

            (format!("http://127.0.0.1:{relay_port}"), format!("http://127.0.0.1:{src_port}"))
        };

        let dir = tmp("relayfb");
        std::fs::create_dir_all(&dir).expect("mkdir");
        // Валидный конфиг (релей СО схемой) — кейс: релей «работает» HTTP-wise,
        // но отдаёт не то, ради чего он был заведён.
        std::fs::write(
            core_store::config_path(&dir),
            format!(
                r#"{{"port":20131,"providers":[],"vercel_relay_url":"{relay_base}","vercel_relay_token":"tok","presets_sources":["{source_base}/api.json"]}}"#,
            ),
        )
        .expect("write");
        let state = AppState::new(&dir).expect("state");

        let r = sync_presets(State(state)).await.expect("sync обязан выжить через фоллбэк");
        assert_eq!(r.attempts.len(), 1, "одна попытка — один источник");
        let a = &r.attempts[0];
        assert!(a.ok, "попытка обязана быть успешной (фоллбэк): {:?}", a);
        assert!(
            a.detail.contains("провайдеров: 1"),
            "успешный разбор обязателен: {:?}",
            a
        );
        assert!(
            a.detail.contains("через релей не вышло") && a.detail.contains("HTML-страница"),
            "detail обязан объяснить фоллбэк и причину провала релея: {:?}",
            a
        );
        // Итоговый каталог = встроенный seed + внешний слой, поэтому
        // absolute-count здесь = invented-number. Проверяем ФАКТ применения
        // мок-источника: провайдер groq обязан появиться в сохранённом кэше.
        let (catalog, _) = presets::load(&dir);
        assert!(
            catalog.providers.iter().any(|p| p.id == "groq"),
            "groq из мок-источника обязан попасть в каталог: {:?}",
            catalog.providers.iter().map(|p| p.id.as_str()).collect::<Vec<_>>()
        );
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

    #[tokio::test]
    async fn failing_admin_request_is_logged_to_event_log() {
        // PRP 23.10.2026, шаг 2: отказ admin-API обязан попадать в журнал
        // событий (и через `EventLog::push` в `gateway.log`). Кейс юзера:
        // «Проверить ключи» упало тостом — а журнал молчал.
        let dir = tmp("errors_logged");
        std::fs::create_dir_all(&dir).expect("mkdir");
        core_store::save(&dir, &GatewayConfig::default()).expect("save");
        let state = AppState::new(&dir).expect("state");
        let app = crate::server::build(state.clone());

        let r = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/api/providers/unknown/keys/check")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("response");
        assert_eq!(r.status(), StatusCode::NOT_FOUND);

        let events = state.events().tail(10);
        let hit = events.iter().find(|e| {
            e.level == "error"
                && e.message.contains("/api/providers/unknown/keys/check")
                && e.message.contains("404")
        });
        assert!(
            hit.is_some(),
            "404-отказ обязан быть в журнале; журнал: {:?}",
            events
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `POST /api/ui-error` (запрос 25.10.2026, E2): отчёт дашборда об ошибке
    /// JS/сети попадает в RAM-журнал с префиксом `ui:` (дальше его унесёт
    /// зеркало в лог хоста). Пустое/кривое тело — 400 с читаемой причиной:
    /// молчаливый откат маскировал бы сломанного репортёра.
    #[tokio::test]
    async fn ui_error_report_lands_in_event_log() {
        let dir = tmp("ui_error");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let state = AppState::new(&dir).expect("state");

        // Валидный отчёт.
        let r = report_ui_error(
            State(state.clone()),
            axum::body::Bytes::from(
                r#"{"where":"loadEvents","message":"шлюз недоступен: TypeError: Failed to fetch"}"#,
            ),
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK);
        let events = state.events().tail(10);
        let hit = events
            .iter()
            .find(|e| e.level == "warn" && e.message.contains("ui: loadEvents:"));
        assert!(hit.is_some(), "ui-отчёт обязан быть в журнале: {:?}", events);
        assert!(
            hit.unwrap().message.contains("Failed to fetch"),
            "текст ошибки обязан попасть в журнал: {}",
            hit.unwrap().message
        );

        // Пустое тело — 400.
        let r = report_ui_error(State(state.clone()), axum::body::Bytes::new()).await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        // Кривой JSON — 400.
        let r = report_ui_error(State(state.clone()), axum::body::Bytes::from("не-json")).await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        // Пустой message — 400.
        let r = report_ui_error(
            State(state.clone()),
            axum::body::Bytes::from(r#"{"where":"x","message":"  "}"#),
        )
        .await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Хвост строки — на границе символов, не байт: сообщения дашборда
    /// содержат кириллицу, байтовый срез дал бы панику или мусор.
    #[test]
    fn tail_chars_slices_on_char_boundary() {
        assert_eq!(tail_chars("короткое", 300), "короткое");
        assert_eq!(tail_chars("абвгд", 3), "вгд");
        assert_eq!(tail_chars("абв", 0), "");
        // Многосимвольная юникод-строка: 🚀 — 4 байта, байтовый срез паниковал бы.
        assert_eq!(tail_chars("🚀🚀🚀🚀", 2), "🚀🚀");
    }

    /// Тело `{"models": [...]}` (запрос 25.10.2026): кривой JSON — 400 с
    /// читаемой причиной (не молчаливый откат к дефолту), валидный JSON
    /// доходит до планировщика — отказ «не Gemini» доказывает, что список
    /// распарсился и был передан в key_check.
    #[tokio::test]
    async fn keys_check_accepts_models_body_or_rejects_bad_json() {
        let dir = tmp("keys_check_body");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let mut cfg = GatewayConfig::default();
        // Не-Gemini провайдер: планировщик обязан отказать ДО сети.
        cfg.providers.push(ProviderConfig {
            id: "mock".into(),
            name: "Mock".into(),
            base_url: "https://upstream.example/v1".into(),
            keys: vec![AccountKey { id: "k1".into(), key: "sk-1".into(), is_active: true, name: None }],
            ..Default::default()
        });
        core_store::save(&dir, &cfg).expect("save");
        let state = AppState::new(&dir).expect("state");
        let app = crate::server::build(state);

        let r = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/api/providers/mock/keys/check")
                    .header("content-type", "application/json")
                    .body(Body::from("не json вовсе"))
                    .expect("req"),
            )
            .await
            .expect("response");
        assert_eq!(r.status(), StatusCode::BAD_REQUEST, "кривой JSON — 400");
        let bytes = axum::body::to_bytes(r.into_body(), usize::MAX).await.expect("body");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("тело запроса"), "причина называет проблему тела: {text}");

        let state2 = AppState::new(&dir).expect("state");
        let app2 = crate::server::build(state2);
        let r = app2
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/api/providers/mock/keys/check")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"models":["gemini-3.8-flash"]}"#))
                    .expect("req"),
            )
            .await
            .expect("response");
        assert_eq!(r.status(), StatusCode::BAD_REQUEST, "не-Gemini апстрим отклоняется планом");
        let bytes = axum::body::to_bytes(r.into_body(), usize::MAX).await.expect("body");
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            text.contains("не Gemini"),
            "валидное тело дошло до планировщика (отказ про апстрим, не про JSON): {text}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}