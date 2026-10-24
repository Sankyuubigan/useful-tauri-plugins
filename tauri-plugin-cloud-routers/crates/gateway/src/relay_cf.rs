//! Развёртывание релея на Cloudflare Workers по REST API.
//!
//! Второй вид релея рядом с Vercel ([`crate::relay`]): протокол релея тот же
//! (заголовки `x-relay-token`/`x-target-url`, fail-closed без секрета), а
//! [`RELAY_WORKER_CF`] — адаптация того же исходника `relay-worker.js` под
//! платформу: `env` вместо `process.env`, модульный синтаксис Workers.
//!
//! Почему REST, а не `wrangler`: деплой делается кнопкой из панели настроек;
//! у пользователя нет ни node, ни wrangler, а у шлюза нет права запускать
//! внешние процессы сборки. Тот же выбор платформа-ориентированного REST,
//! что и у Vercel-пути.
//!
//! ## Порядок вызовов (каждый шаг — документированный эндпоинт CF REST v4)
//!
//! 1. `GET /accounts` — резолв аккаунта. Токен с доступом к нескольким
//!    аккаунтам без явно указанного ID даёт честный отказ, а не «случайный
//!    первый аккаунт»: молча разместить воркер в неправильном аккаунте хуже,
//!    чем попросить аккаунт-строку.
//! 2. `GET /accounts/{id}/workers/subdomain` — публичный субдомен аккаунта
//!    (`*.workers.dev`). Пусто → честный отказ: воркер без субдомена
//!    недоступен снаружи, а автосоздание субдомена здесь не эмулируется.
//! 3. `PUT /accounts/{id}/workers/scripts/{name}` — multipart:
//!    part `metadata` (`{"main_module":"relay.js","compatibility_date":"…"}`
//!    — обязателен для групповой загрузки модулей) + part `relay.js`
//!    (`application/javascript+module`).
//! 4. `PUT /accounts/{id}/workers/scripts/{name}/secrets` — секрет `RELAY_TOKEN`
//!    через документированный эндпоинт: по документации Cloudflare он
//!    **создаёт новую версию воркера** с секретом, поэтому второго прохода
//!    деплоя, как у Vercel, НЕ нужно.
//! 5. `POST /accounts/{id}/workers/scripts/{name}/subdomain` —
//!    `{"enabled": true}`: включить воркер на публичном субдомене (иначе
//!    URL молча отдавал бы «no worker»).
//!
//! Общий секрет создаётся ДО сетевых вызовов ([`ensure_relay_token`]): общая
//! защита прокси важнее успеха деплоя.
//!
//! ## Честно о статусе этого пути
//!
//! Последовательность собрана по документированной схеме REST Cloudflare и
//! **не проверена на живом API** (как и Vercel-путь при его добавлении).
//! Поэтому:
//!
//! - при любой ошибке наружу идёт HTTP-статус и текст ответа CF — не «что-то
//!   пошло не так»;
//! - частичный успех описывается явно (воркер загружен / секрет поставлен) —
//!   повторный деплой поправим;
//! - ручной путь остаётся доступен теми же эндпоинтами, что и у Vercel:
//!   секрет — `/api/relay/token`.

use cloud_routers_core::config::{RelaySlot, RELAY_KIND_CLOUDFLARE};
use serde::Deserialize;

use crate::app::AppState;
use crate::relay::{ensure_relay_token, DeployResult};

/// Исходник воркера для Cloudflare. Отдаётся шлюзом по деплою; тот же
/// контракт заголовков, что у Vercel-воркера.
pub const RELAY_WORKER_CF: &str = include_str!("../assets/relay-worker-cf.js");

/// Имя воркера по умолчанию: URL вида `https://<имя>.<субдомен>.workers.dev`.
pub const DEFAULT_CF_SCRIPT: &str = "cloud-routers-relay";

/// Идентификатор слота Cloudflare в конфиге. Один CF-релей на MVP: слот
/// находится по `kind`, а не по id — переименование воркера не плодит слоты.
const SLOT_ID_CF: &str = "cloudflare";

/// База Cloudflare REST API.
const CF_API_BASE: &str = "https://api.cloudflare.com/client/v4";

/// Дата совместимости воркера. Не привязана к релизу: компиляция старого
/// синтаксиса не нужна, воркер держится на стандартных API платформы.
const COMPAT_DATE: &str = "2024-01-01";

/// Имя модуля воркера в загрузке. Обязательно совпадает с `main_module`
/// в metadata и с именем файла part'а.
const MAIN_MODULE: &str = "relay.js";

/// Имя секрета, который читает воркер. Должно совпадать с константой в
/// `relay-worker-cf.js` и с Vercel-воркером (`relay.rs`), чтобы оба вида
/// релея жили на одном общем секрете.
const RELAY_TOKEN_ENV: &str = "RELAY_TOKEN";

const HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Огибающий (`envelope`) любого ответа Cloudflare API.
#[derive(Debug, Deserialize)]
struct CfEnvelope {
    success: bool,
    #[serde(default)]
    result: serde_json::Value,
    #[serde(default)]
    errors: Vec<CfApiError>,
}

/// Ошибка внутри envelope: у CF статус HTTP часто 200 даже при `success: false`.
#[derive(Debug, Deserialize)]
struct CfApiError {
    code: i64,
    message: String,
}

/// Текст ошибок envelope для сообщения пользователю.
fn errors_text(errors: &[CfApiError]) -> String {
    if errors.is_empty() {
        "ответ не является корректным envelope Cloudflare".to_string()
    } else {
        errors
            .iter()
            .map(|e| format!("код {}: {}", e.code, e.message))
            .collect::<Vec<_>>()
            .join("; ")
            .chars()
            .take(300)
            .collect()
    }
}

/// Один JSON-вызов CF API. Возвращает `result` из envelope; ошибка наружу —
/// всегда HTTP-код + текст ошибок CF, никогда «что-то пошло не так».
async fn cf_json(
    state: &AppState,
    method: reqwest::Method,
    path: &str,
    api_token: &str,
    body: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let url = format!("{CF_API_BASE}{path}");
    let mut builder = state.http().request(method, url).bearer_auth(api_token);
    if let Some(payload) = body {
        builder = builder.json(&payload);
    }
    let response = builder
        .timeout(HTTP_TIMEOUT)
        .send()
        .await
        .map_err(|e| format!("Не удалось обратиться к Cloudflare API: {}", e))?;
    parse_envelope(response).await
}

/// Преобразовать ответ CF в `result` или в ошибку с текстом API.
async fn parse_envelope(response: reqwest::Response) -> Result<serde_json::Value, String> {
    let status = response.status();
    let raw = response.text().await.unwrap_or_default();
    let envelope: CfEnvelope = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => {
            return Err(format!(
                "Cloudflare API: HTTP {} — {}",
                status.as_u16(),
                raw.chars().take(300).collect::<String>()
            ))
        }
    };
    if !status.is_success() || !envelope.success {
        return Err(format!(
            "Cloudflare API: HTTP {} — {}",
            status.as_u16(),
            errors_text(&envelope.errors)
        ));
    }
    Ok(envelope.result)
}

/// Резолв аккаунта под токеном.
///
/// Явный ID бесспорен. Иначе: 0 аккаунтов → проверь права токена; 1 — берём;
/// несколько — отказ с подсказкой, где взять ID (панель Cloudflare →
/// «Workers & Pages» → правый столбец «Account Overview» → «Account ID»).
async fn select_account(
    state: &AppState,
    api_token: &str,
    explicit: Option<&str>,
) -> Result<String, String> {
    if let Some(id) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(id.to_string());
    }
    let body = cf_json(state, reqwest::Method::GET, "/accounts?per_page=5", api_token, None).await?;
    let accounts = body.as_array().ok_or_else(|| {
        "Cloudflare API: /accounts вернул не список — проверьте права токена (Workers Scripts: Edit)".to_string()
    })?;
    match accounts.len() {
        0 => Err(
            "Cloudflare API: токен валиден, но аккаунтов не видно — проверьте права токена (Workers Scripts: Edit)"
                .to_string(),
        ),
        1 => accounts[0]["id"].as_str().map(|s| s.to_string()).ok_or_else(|| {
            "Cloudflare API: у единственного аккаунта нет идентификатора".to_string()
        }),
        _ => Err(
            "Cloudflare API: токен видит несколько аккаунтов — укажите Account ID (панель Cloudflare → Workers & Pages → Account ID)"
                .to_string(),
        ),
    }
}

/// Субдомен workers.dev аккаунта. Пустой субдомен — честный отказ.
async fn resolve_subdomain(state: &AppState, api_token: &str, account: &str) -> Result<String, String> {
    let body = cf_json(
        state,
        reqwest::Method::GET,
        &format!("/accounts/{account}/workers/subdomain"),
        api_token,
        None,
    )
    .await?;
    let subdomain = body["subdomain"].as_str().unwrap_or_default();
    if subdomain.is_empty() {
        return Err(
            "Cloudflare API: у аккаунта не задан субдомен workers.dev — создайте его в панели Cloudflare → Workers & Pages → Change Subdomain"
                .to_string(),
        );
    }
    Ok(subdomain.to_string())
}

/// Загрузить воркер: `PUT .../scripts/{name}` multipart (metadata + модуль).
async fn upload_worker(state: &AppState, api_token: &str, account: &str, script: &str) -> Result<(), String> {
    let metadata = serde_json::json!({ "main_module": MAIN_MODULE, "compatibility_date": COMPAT_DATE });
    let meta_bytes =
        serde_json::to_vec(&metadata).map_err(|e| format!("Не удалось собрать metadata воркера: {}", e))?;
    let module_name = MAIN_MODULE.to_string();
    let form = reqwest::multipart::Form::new()
        .part(
            "metadata",
            reqwest::multipart::Part::bytes(meta_bytes)
                .mime_str("application/json")
                .map_err(|e| format!("Не удалось собрать multipart: {}", e))?,
        )
        .part(
            module_name.clone(),
            reqwest::multipart::Part::bytes(RELAY_WORKER_CF.as_bytes().to_vec())
                .file_name(module_name)
                .mime_str("application/javascript+module")
                .map_err(|e| format!("Не удалось собрать multipart: {}", e))?,
        );
    let response = state
        .http()
        .put(format!("{CF_API_BASE}/accounts/{account}/workers/scripts/{script}"))
        .bearer_auth(api_token)
        .multipart(form)
        .timeout(HTTP_TIMEOUT)
        .send()
        .await
        .map_err(|e| format!("Не удалось загрузить воркер в Cloudflare: {}", e))?;
    parse_envelope(response).await?;
    Ok(())
}

/// Секрет воркера через `PUT .../scripts/{name}/secrets`.
async fn put_secret(
    state: &AppState,
    api_token: &str,
    account: &str,
    script: &str,
    relay_secret: &str,
) -> Result<(), String> {
    let payload = serde_json::json!({
        "name": RELAY_TOKEN_ENV,
        "text": relay_secret,
        "type": "secret_text",
    });
    cf_json(
        state,
        reqwest::Method::PUT,
        &format!("/accounts/{account}/workers/scripts/{script}/secrets"),
        api_token,
        Some(payload),
    )
    .await?;
    Ok(())
}

/// Включить воркер на публичном субдомене аккаунта.
async fn enable_worker_subdomain(
    state: &AppState,
    api_token: &str,
    account: &str,
    script: &str,
) -> Result<(), String> {
    cf_json(
        state,
        reqwest::Method::POST,
        &format!("/accounts/{account}/workers/scripts/{script}/subdomain"),
        api_token,
        Some(serde_json::json!({ "enabled": true })),
    )
    .await?;
    Ok(())
}

/// Занести заполненный слот: URL, секрет, привязка к аккаунту.
fn fill_cf_slot(slot: &mut RelaySlot, url: &str, secret: &str, account: &str, script: &str) {
    slot.url = Some(url.to_string());
    slot.relay_secret = Some(secret.to_string());
    slot.cf_account_id = Some(account.to_string());
    slot.cf_script_name = Some(script.to_string());
}

/// Записать слот Cloudflare в конфиг и сделать его активным.
fn write_cf_slot(
    state: &AppState,
    url: &str,
    secret: &str,
    account: &str,
    script: &str,
) -> Result<(), String> {
    state.update_config(move |c| {
        // Legacy → слот, если ещё не был перенесён: после первого
        // CF-деплоя активным становится не-верцеловский слот, и legacy
        // чтение через `relay_url()`-фолбэк больше ничего не решает.
        c.migrate_relay_slots();
        let slot_id = match c.relays.iter().position(|s| s.kind == RELAY_KIND_CLOUDFLARE) {
            Some(idx) => {
                let slot = c.relays.get_mut(idx).expect("индекс из того же вектора");
                fill_cf_slot(slot, url, secret, account, script);
                slot.id.clone()
            }
            None => {
                let mut slot = RelaySlot {
                    id: SLOT_ID_CF.to_string(),
                    kind: RELAY_KIND_CLOUDFLARE.to_string(),
                    ..Default::default()
                };
                fill_cf_slot(&mut slot, url, secret, account, script);
                let id = slot.id.clone();
                c.relays.push(slot);
                id
            }
        };
        c.active_relay = Some(slot_id);
        Ok(())
    })
}

/// Развернуть релей на Cloudflare Workers.
///
/// Возвращает URL воркера (`https://{script}.{subdomain}.workers.dev`);
/// `deployment_id` у CF-пути нет (модульная загрузка не даёт id деплоя).
pub async fn deploy_relay_cf(
    state: &AppState,
    api_token: &str,
    account_id: Option<&str>,
    script_arg: Option<&str>,
) -> Result<DeployResult, String> {
    let token = api_token.trim();
    if token.is_empty() {
        return Err("Токен Cloudflare не указан".to_string());
    }
    let script = script_arg
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_CF_SCRIPT);
    // Ограничения имени воркера CF: латиница/цифры/дефис/подчёркивание, ≤64.
    // Проверка ДО сети и ДО создания секрета — без сетевых побочных эффектов.
    let name_ok = script.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !name_ok || script.len() > 64 {
        return Err(format!(
            "Имя воркера «{script}» не подходит: латиница, цифры, дефис или подчёркивание, до 64 символов"
        ));
    }

    // Общий секрет создаётся ДО обращения к CF: без него развёрнутый воркер
    // был бы публичным прокси (см. `ensure_relay_token` из relay.rs).
    let relay_secret = ensure_relay_token(state)?;

    let account = select_account(state, token, account_id).await?;
    let subdomain = resolve_subdomain(state, token, &account).await?;

    if let Err(e) = upload_worker(state, token, &account, &script).await {
        return Err(e);
    }
    if let Err(e) = put_secret(state, token, &account, &script, &relay_secret).await {
        return Err(format!(
            "{e}. Воркер «{script}» уже загружен и отвечает 503 (fail-closed, без секрета ничего не проксирует) \
             — нажмите «Развернуть» ещё раз: секрет проставляется отдельным запросом."
        ));
    }
    if let Err(e) = enable_worker_subdomain(state, token, &account, &script).await {
        return Err(format!(
            "{e}. Воркер и секрет уже сохранены — повторный деплой включит воркер на субдомене."
        ));
    }

    let url = format!("https://{script}.{subdomain}.workers.dev");
    write_cf_slot(state, &url, &relay_secret, &account, &script)?;
    state.events().info(format!("релей Cloudflare развёрнут на {}", url));
    Ok(DeployResult {
        url,
        deployment_id: None,
        raw: format!("Cloudflare: воркер «{script}» (аккаунт {account}) включён на {subdomain}.workers.dev"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cf_worker_source_has_no_process_env() {
        // CF Workers не имеют `process` — воркер обязан читать переменные из
        // параметра `env` обработчика. Иначе развёрнутый воркер упадёт
        // на первом запросе с ReferenceError.
        assert!(
            !RELAY_WORKER_CF.contains("process.env"),
            "CF-воркер не должен читать process.env — это API Vercel"
        );
        assert!(
            RELAY_WORKER_CF.contains("env.RELAY_TOKEN"),
            "CF-воркер обязан читать секрет из env-параметра обработчика"
        );
    }

    #[test]
    fn cf_worker_source_demands_shared_secret() {
        // Тот же контракт защиты, что у Vercel: без секрета — fail-closed 503.
        assert!(
            RELAY_WORKER_CF.contains("x-relay-token"),
            "CF-воркер не проверяет общий секрет"
        );
        assert!(
            RELAY_WORKER_CF.contains("if (!expected)") && RELAY_WORKER_CF.contains("503"),
            "CF-воркер обязан fail-closed (503) без RELAY_TOKEN"
        );
    }

    #[test]
    fn cf_worker_source_is_workers_module() {
        assert!(
            !RELAY_WORKER_CF.contains("export const config"),
            "export const config — Vercel-специфика, CF такую конструкцию не понимает"
        );
        assert!(
            RELAY_WORKER_CF.contains("export default"),
            "CF-воркер обязан быть ES-модулем с default-экспортом"
        );
    }

    #[test]
    fn cf_worker_source_matches_gateway_probe() {
        // probe_relay отличает «наш воркер» от чужого ответа по такому тексту.
        assert!(RELAY_WORKER_CF.contains("x-target-url"));
    }

    #[test]
    fn cf_worker_names_relay_token_env() {
        assert_eq!(RELAY_TOKEN_ENV, "RELAY_TOKEN");
        assert!(RELAY_WORKER_CF.contains(RELAY_TOKEN_ENV));
    }

    #[tokio::test]
    async fn cf_deploy_requires_token() {
        let dir = std::env::temp_dir().join("cr_gw_relay_cf_notoken");
        let _ = std::fs::remove_dir_all(&dir);
        let state = AppState::new(&dir).expect("state");
        let err = deploy_relay_cf(&state, "   ", None, None).await.expect_err("must fail");
        assert!(err.contains("Токен Cloudflare"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn cf_deploy_rejects_bad_script_name_before_network() {
        let dir = std::env::temp_dir().join("cr_gw_relay_cf_badname");
        let _ = std::fs::remove_dir_all(&dir);
        let state = AppState::new(&dir).expect("state");
        let err = deploy_relay_cf(&state, "tok", None, Some("bad name!"))
            .await
            .expect_err("must fail");
        assert!(err.contains("Имя воркера"), "{err}");
        // Валидация имени идёт ДО создания секрета: никаких побочных эффектов.
        assert!(
            state.config().relay_token().is_none(),
            "проверка имени не должна создавать секрет релея"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
