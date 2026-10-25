//! Проверка API-ключей Gemini-провайдера через эндпоинт `:countTokens`.
//!
//! ## Почему именно `:countTokens`
//!
//! Лимиты Google AI Studio у пользователей тарифицируются по запросам к
//! генерации (`generateContent` и прочие). `:countTokens` — служебный эндпоинт,
//! который пользователь подтверждённым вручную рецептом (POST
//! `/v1beta/models/{model}:countTokens` с заголовком `x-goog-api-key`) проверил
//! на практике: аутентификация ключа происходит, а дневной лимит не тратится.
//! Кодовая гарантия против расхода лимита — тест
//! [`tests::target_url_is_count_tokens_only`]: по адресу, отличному от
//! `…:countTokens`, запрос построить нельзя.
//!
//! ## Модели проверки
//!
//! Смысл проверки — «сколько у ключа осталось рабочих лимитов» на конкретной
//! модели, поэтому модель выбирает пользователь в дашборде (запрос 25.10.2026).
//! Без явного списка берётся первый `gemini…` из моделей провайдера, а при
//! пустом списке — [`GEMINI_CHECK_FALLBACK_MODEL`] (страховка, не источник
//! правды: правильный список даёт кнопка «Обновить модели»).
//!
//! ## Правила
//!
//! - Провайдер считается Gemini по host'у `generativelanguage.googleapis.com`
//!   ([`is_gemini_provider`] — SSOT и для поля `is_gemini` в DTO дашборда).
//! - База провайдера — OpenAI-совместимая (`…/v1beta/openai`), нативный
//!   эндпоинт живёт без `/openai`: [`gemini_native_root`] отрезает суффикс
//!   (или дописывает `/v1beta`).
//! - Маршрут подчиняется флагу провайдера `use_relay` (та же правда, что у
//!   прокси чатов): релей берётся из [`crate::upstream::relay_route_base`],
//!   «релей настроен, но не задан / без секрета» — жёсткая ошибка, молчаливого
//!   прямого запроса нет.
//! - Проверка — диагностика, **не ротация**: конфиг не мутируется, кулдауны
//!   и `is_active` остаются как были.
//! - Запросы последовательные (ключ × модель за раз), таймаут 15 c на запрос,
//!   вердикты классифицирует чистая [`classify_key_check`].

use cloud_routers_core::config::{AccountKey, GatewayConfig, ProviderConfig};
use serde::Serialize;

use crate::app::AppState;
use crate::logbuf::mask_secret;
use crate::upstream::{HDR_RELAY_TOKEN, HDR_TARGET_URL, RELAY_PATH_PREFIX, relay_route_base};

/// Запасная модель для проверки, если выбранного/найденного списка нет.
///
/// Обновлена на `gemini-3.8-flash` (инцидент 25.10.2026): `gemini-2.0-flash`
/// отключён upstream — Google в теле 404 прямо рекомендует `gemini-3.8-flash`,
/// и все ключи получали «неизвестно» по вине этой константы, а не ключей.
pub const GEMINI_CHECK_FALLBACK_MODEL: &str = "gemini-3.8-flash";

/// Максимум моделей в одном прогоне проверки (защита от «проверить всё»).
pub const MAX_CHECK_MODELS: usize = 10;

/// Таймаут одного HTTP-запроса.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Хвост, до которого обрезается текст неудачного ответа в `detail`.
const DETAIL_MAX_CHARS: usize = 300;

/// Итог проверки одного ключа по одной модели.
#[derive(Debug, Clone, Serialize)]
pub struct KeyCheckResult {
    pub key_id: String,
    /// Имя, заданное пользователем (может отсутствовать — см. `AccountKey`).
    pub name: Option<String>,
    /// Маска секрета (`AQ.Ab8…`) — секрет целиком наружу не уходит.
    pub masked: String,
    /// Модель, по которой шёл запрос (`models/`-префикс снят).
    pub model: String,
    pub status: Option<u16>,
    pub verdict: CheckVerdict,
    /// Причина по-человечески (начало тела ответа либо ошибка сети).
    pub detail: String,
}

/// Вердикт по ключу.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckVerdict {
    /// Аутентификация прошла.
    Valid,
    /// Ключ отклонён провайдером (401/403 или явный `API key not valid`).
    Invalid,
    /// Аутентификация прошла, но сейчас сработал лимит (429).
    ValidLimited,
    /// Ответ, по которому вердикт дать нельзя.
    Unknown,
}

/// Свод проверки по провайдеру.
#[derive(Debug, Serialize)]
pub struct KeyCheckSummary {
    /// Модели, по которым шёл запрос (в порядке прогона).
    pub models: Vec<String>,
    /// Запросы шли через релей (`use_relay` провайдера).
    pub via_relay: bool,
    pub results: Vec<KeyCheckResult>,
}

/// Провайдер Gemini? SSOT: используется и хендлером проверки, и полем
/// `is_gemini` в `ProviderDto` (дашборд решает по полю DTO, не по своей копии).
pub fn is_gemini_provider(p: &ProviderConfig) -> bool {
    p.base_url.to_ascii_lowercase().contains("generativelanguage.googleapis.com")
}

/// Нативный root Gemini для base_url провайдера.
///
/// База провайдера — OpenAI-совместимая: `…/v1beta/openai` (из каталога
/// пресетов). Нативный countTokens-эндпоинт живёт без `/openai`, поэтому:
/// суффикс `/openai` отрезается, а если после этого `/v1beta` нет —
/// дописывается. Завершающие `/` снимает `trim_base_url`.
pub fn gemini_native_root(base_url: &str) -> String {
    let trimmed = cloud_routers_core::config::trim_base_url(base_url);
    let no_openai = trimmed.strip_suffix("/openai").unwrap_or(&trimmed);
    if no_openai.ends_with("/v1beta") {
        no_openai.to_string()
    } else {
        format!("{no_openai}/v1beta")
    }
}

/// Кандидат модели по умолчанию: первый `gemini…` из списка провайдера,
/// префикс `models/` отрезается; списка нет — `GEMINI_CHECK_FALLBACK_MODEL`.
pub fn model_candidate(p: &ProviderConfig) -> String {
    p.models
        .iter()
        .map(|m| m.id.trim())
        // Литеральная запись апстрима вида «models/gemini-2.5-pro» встречается
        // у каталогов: проверим «gemini» уже после отрезания префикса.
        .map(|id| id.strip_prefix("models/").unwrap_or(id))
        .find(|id| id.starts_with("gemini"))
        .map(str::to_string)
        .unwrap_or_else(|| GEMINI_CHECK_FALLBACK_MODEL.to_string())
}

/// Санитайзинг пользовательского списка моделей: трим, снятие префикса
/// `models/`, пустые отбрасываются, дубли схлопываются, кап
/// [`MAX_CHECK_MODELS`] (лишнее молча не теряем — тест закрепляет порядок).
pub fn sanitize_check_models(raw: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(raw.len());
    for m in raw {
        let trimmed = m.trim();
        let id = trimmed.strip_prefix("models/").unwrap_or(trimmed);
        if id.is_empty() || out.iter().any(|x| x == id) {
            continue;
        }
        out.push(id.to_string());
        if out.len() >= MAX_CHECK_MODELS {
            break;
        }
    }
    out
}

/// Чистый классификатор ответа → вердикт.
///
/// Правило не осуждать ключ за «unknown»: 400 без валидного текста ошибки
/// означает нечитаемый запрос, а его обязан исправлять код, а не ключ.
pub fn classify_key_check(status: Option<u16>, body: &str) -> CheckVerdict {
    let low = body.to_ascii_lowercase();
    match status {
        Some(200) => CheckVerdict::Valid,
        Some(429) => CheckVerdict::ValidLimited,
        Some(401 | 403) => CheckVerdict::Invalid,
        Some(400) => {
            if low.contains("api key not valid") || low.contains("api_key_invalid") {
                CheckVerdict::Invalid
            } else {
                CheckVerdict::Unknown
            }
        }
        _ => CheckVerdict::Unknown,
    }
}

/// Маршрут и цели проверки — всё, что вычисляется ДО сети.
#[derive(Debug, Clone)]
pub struct CheckPlan {
    pub models: Vec<String>,
    pub via_relay: bool,
    /// Абсолютные URL целей: `{root}/models/{model}:countTokens` — по одному
    /// на модель, в том же порядке, что и `models`.
    pub target_urls: Vec<String>,
}

/// URL цели проверки для одной модели (модульный тест закрепляет его форму).
fn check_target_url(base: &str, model: &str) -> Result<String, String> {
    let base = base.trim();
    if base.is_empty() {
        return Err("у провайдера нет адреса — проверять нечего".to_string());
    }
    Ok(format!("{}/models/{model}:countTokens", gemini_native_root(base)))
}

/// Проверка маршрута и целей ДО сети. Все причины отказа — понятные строки,
/// которые попадают в тост дашборда без доработок.
pub fn build_check_plan(
    provider: &ProviderConfig,
    config: &GatewayConfig,
    requested: Option<&[String]>,
) -> Result<CheckPlan, String> {
    if !is_gemini_provider(provider) {
        return Err(format!(
            "«{}» — не Gemini-провайдер (апстрим не generativelanguage.googleapis.com): проверка через countTokens не подходит",
            provider.name
        ));
    }
    // Список моделей: явный выбор пользователя; пустой после санитайзинга или
    // не заданный — дефолтный кандидат провайдера.
    let models = match requested.map(sanitize_check_models) {
        Some(list) if !list.is_empty() => list,
        _ => vec![model_candidate(provider)],
    };
    let target_urls = models
        .iter()
        .map(|model| check_target_url(&provider.base_url, model))
        .collect::<Result<Vec<_>, _>>()?;
    let via_relay = provider.use_relay;
    if via_relay {
        // Общий helper релея: «настроен, но не задан / задан без схемы» —
        // жёсткая ошибка, молчаливого прямого запроса нет.
        relay_route_base(provider, config)?;
        let token_owned = config.relay_token();
        let token = token_owned.as_deref().map(str::trim).filter(|t| !t.is_empty());
        if token.is_none() {
            return Err(
                "релей настроен, но секрет релея не задан — впишите его в воркер (кнопка \
                 «Показать секрет RELAY_TOKEN» на вкладке «Релей») либо снимите флаг «через релей»"
                    .to_string(),
            );
        }
    }
    Ok(CheckPlan { models, via_relay, target_urls })
}

/// Выполнить готовый [`CheckPlan`] против активных ключей провайдера.
///
/// Поднят в отдельную функцию ради интеграционных тестов: mock-апстрим живёт
/// на 127.0.0.1, а значит host-проверку [`is_gemini_provider`] он не проходит;
/// тест свободен собрать план на мок-URL и прогнать реальную сеть.
pub async fn run_plan(
    state: &AppState,
    provider: &ProviderConfig,
    plan: CheckPlan,
) -> Result<KeyCheckSummary, String> {
    let keys: Vec<AccountKey> = provider
        .keys
        .iter()
        .filter(|k| k.is_active && !k.key.trim().is_empty())
        .cloned()
        .collect();
    if keys.is_empty() {
        return Err(format!("у провайдера «{}» нет активных ключей для проверки", provider.name));
    }

    let config = state.config();
    let relay_url = if plan.via_relay {
        let relay = relay_route_base(provider, &config)?;
        Some(format!("{relay}{RELAY_PATH_PREFIX}"))
    } else {
        None
    };
    let relay_token = config.relay_token().filter(|t| !t.trim().is_empty());

    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| format!("не удалось собрать HTTP-клиент: {e}"))?;

    let mut results = Vec::with_capacity(keys.len() * plan.models.len());
    for item in &keys {
        for (model, target) in plan.models.iter().zip(&plan.target_urls) {
            // Последовательно по одному запросу: спешки нет, а «выстрелы
            // пачкой» для релея и лимитов лишние.
            let request_url = relay_url.clone().unwrap_or_else(|| target.clone());
            let (status, body) =
                send_one(&client, &request_url, plan.via_relay, target, relay_token.as_deref(), &item.key)
                    .await;
            let verdict = classify_key_check(status, &body);
            let masked = mask_secret(&item.key);
            let detail = truncate_detail(&body);
            // Тело ошибки апстрима (например, 404 «model … is no longer
            // available») обязано доживать в журнале — только по вердиктам,
            // где оно объясняет причину (запрос 25.10.2026).
            let _ = match verdict {
                CheckVerdict::Valid => state.events().info(format!(
                    "ключ {} {}: модель {model}: валиден (HTTP {})",
                    provider.id,
                    &masked,
                    status.unwrap_or(0)
                )),
                CheckVerdict::ValidLimited => state.events().info(format!(
                    "ключ {} {}: модель {model}: валиден, сейчас в лимите (HTTP 429)",
                    provider.id, &masked
                )),
                CheckVerdict::Invalid => state.events().warn(format!(
                    "ключ {} {}: модель {model}: отвергнут (HTTP {}) — {detail}",
                    provider.id,
                    &masked,
                    status.unwrap_or(0)
                )),
                CheckVerdict::Unknown => state.events().warn(format!(
                    "ключ {} {}: модель {model}: непонятный ответ (HTTP {}) — ключ не осуждён — {detail}",
                    provider.id,
                    &masked,
                    status.unwrap_or(0)
                )),
            };
            results.push(KeyCheckResult {
                key_id: item.id.clone(),
                name: item.name.clone(),
                masked,
                model: model.clone(),
                status,
                verdict,
                detail,
            });
        }
    }

    let (valid, limited, invalid, unknown) = counts(&results);
    state.events().info(format!(
        "проверка ключей «{}» (моделей {}) через {}: валидных {valid}, в лимите {limited}, невалидных {invalid}, неизвестно {unknown}",
        provider.name,
        plan.models.len(),
        if plan.via_relay { "релей" } else { "напрямую" }
    ));

    Ok(KeyCheckSummary { models: plan.models, via_relay: plan.via_relay, results })
}

/// Полная проверка провайдера: план → запуск. Её вызывает хендлер админки.
pub async fn check_provider_keys(
    state: &AppState,
    provider: &ProviderConfig,
    requested: Option<&[String]>,
) -> Result<KeyCheckSummary, String> {
    let plan = build_check_plan(provider, &state.config(), requested)?;
    run_plan(state, provider, plan).await
}

/// Один HTTP-запрос проверки. Ответ — `(статус, тело)`; сеть упала —
/// `(None, причина)`, вердикт такой же вариант, как у нечитаемого ответа.
async fn send_one(
    client: &reqwest::Client,
    request_url: &str,
    via_relay: bool,
    target_url: &str,
    relay_token: Option<&str>,
    secret: &str,
) -> (Option<u16>, String) {
    let mut req = client.post(request_url);
    if via_relay {
        // Релей шлёт fetch по абсолютному target URL — путь самой отправки
        // важности не имеет, важен заголовок цели.
        req = req
            .header(HDR_TARGET_URL, target_url)
            .header(HDR_RELAY_TOKEN, relay_token.unwrap_or_default());
    }
    // Ключ передаётся заголовком x-goog-api-key (тот же рецепт, что юзер
    // проверял вручную); Authorization не используется.
    let req = req
        .header("x-goog-api-key", secret)
        .header("content-type", "application/json")
        .body(r#"{"contents":[{"parts":[{"text":"check"}]}]}"#);

    match req.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            (Some(status), body)
        }
        Err(e) => (None, format!("сеть: {e}")),
    }
}

fn truncate_detail(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.chars().count() <= DETAIL_MAX_CHARS {
        trimmed.to_string()
    } else {
        let head: String = trimmed.chars().take(DETAIL_MAX_CHARS).collect();
        format!("{head}…")
    }
}

type CountsTuple = (usize, usize, usize, usize);

fn counts(results: &[KeyCheckResult]) -> CountsTuple {
    let mut counts = (0usize, 0usize, 0usize, 0usize);
    for r in results {
        match r.verdict {
            CheckVerdict::Valid => counts.0 += 1,
            CheckVerdict::ValidLimited => counts.1 += 1,
            CheckVerdict::Invalid => counts.2 += 1,
            CheckVerdict::Unknown => counts.3 += 1,
        }
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloud_routers_core::config::DiscoveredModel;

    fn provider(base: &str, models: &[&str]) -> ProviderConfig {
        ProviderConfig {
            id: "google".into(),
            name: "Google".into(),
            base_url: base.into(),
            is_enabled: true,
            use_relay: false,
            keys: vec![AccountKey {
                id: "k1".into(),
                key: "AQ.Ab8RN6-test".into(),
                is_active: true,
                name: None,
            }],
            models: models
                .iter()
                .map(|m| DiscoveredModel { id: m.to_string(), ..Default::default() })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn is_gemini_recognized_by_host_variants() {
        assert!(is_gemini_provider(&provider(
            "https://generativelanguage.googleapis.com/v1beta/openai",
            &[]
        )));
        assert!(!is_gemini_provider(&provider("https://api.groq.com/openai/v1", &[])));
        assert!(!is_gemini_provider(&provider("", &[])), "пустая база не Gemini");
    }

    /// Регресс базы: три реальных вида base_url Gemini-провайдера — голый хост,
    /// задокументированный каталогом `…/v1beta`, OpenAI-совместимый
    /// `…/v1beta/openai`. Все три обязаны дать один нативный root.
    #[test]
    fn gemini_native_root_variants() {
        assert_eq!(
            gemini_native_root("https://generativelanguage.googleapis.com"),
            "https://generativelanguage.googleapis.com/v1beta"
        );
        assert_eq!(
            gemini_native_root("https://generativelanguage.googleapis.com/v1beta/"),
            "https://generativelanguage.googleapis.com/v1beta"
        );
        assert_eq!(
            gemini_native_root("https://generativelanguage.googleapis.com/v1beta/openai"),
            "https://generativelanguage.googleapis.com/v1beta"
        );
    }

    #[test]
    fn model_candidate_prefers_provider_models_and_strips_model_prefix() {
        let mut p = provider("https://generativelanguage.googleapis.com", &[]);
        assert_eq!(model_candidate(&p), GEMINI_CHECK_FALLBACK_MODEL, "списка нет — fallback");

        p.models = vec![
            DiscoveredModel { id: "chat-iron".into(), ..Default::default() },
            DiscoveredModel { id: "models/gemini-3.8-flash".into(), ..Default::default() },
            DiscoveredModel { id: "gemini-2.5-pro".into(), ..Default::default() },
        ];
        // Первый gemini в списке побеждает («chat-iron» ничем не помогает),
        // литеральный префикс models/ снимается.
        assert_eq!(model_candidate(&p), "gemini-3.8-flash");
    }

    /// Санитайзинг пользовательского списка: трим, снятие `models/`, пустые
    /// и дубли отбрасываются, порядок сохраняется, кап — MAX_CHECK_MODELS.
    #[test]
    fn sanitize_check_models_trims_strips_dedups_caps() {
        let raw: Vec<String> = vec![
            "  gemini-3.8-flash  ".into(),
            "models/gemini-2.5-pro".into(),
            "".into(),
            "   ".into(),
            "gemini-3.8-flash".into(), // дубль после снятия префикса/трима
        ];
        assert_eq!(sanitize_check_models(&raw), vec!["gemini-3.8-flash", "gemini-2.5-pro"]);

        let many: Vec<String> = (0..25).map(|i| format!("gemini-m{i}")).collect();
        let got = sanitize_check_models(&many);
        assert_eq!(got.len(), MAX_CHECK_MODELS, "кап обязателен");
        assert_eq!(got.first().map(String::as_str), Some("gemini-m0"), "порядок сохранён");
    }

    /// Кодовая гарантия нулевого расхода лимита: построить цель, отличную
    /// от `…:countTokens`, нельзя — ни `generateContent`, ни `embedContent`.
    #[test]
    fn target_url_is_count_tokens_only() {
        let base = "https://generativelanguage.googleapis.com/v1beta/openai";
        for model in ["gemini-2.5-flash", "gemini-3.8-flash"] {
            let target = check_target_url(base, model).expect("plan");
            assert!(
                target.ends_with(":countTokens"),
                "цель обязана быть ровно countTokens: {target}"
            );
            assert!(target.contains("generativelanguage.googleapis.com"), "{target}");
            assert!(!target.contains("generateContent"), "{target}");
            assert!(!target.contains("embedContent"), "{target}");
            assert!(!target.contains("openai"), "нативный путь без /openai: {target}");
        }
    }

    /// Явный список пользователя попадает в план по одному URL на модель;
    /// без списка работает прежний кандидат провайдера.
    #[test]
    fn build_check_plan_uses_requested_models_and_defaults_to_candidate() {
        let config = GatewayConfig::default();
        let p = provider(
            "https://generativelanguage.googleapis.com/v1beta/openai",
            &["models/gemini-9.9-pro", "gemini-3.8-flash"],
        );

        let requested = vec!["models/gemini-3.8-flash".to_string(), " gemini-2.5-pro ".to_string()];
        let plan = build_check_plan(&p, &config, Some(&requested)).expect("plan");
        assert_eq!(plan.models, vec!["gemini-3.8-flash", "gemini-2.5-pro"]);
        assert_eq!(plan.target_urls.len(), plan.models.len());
        for (url, model) in plan.target_urls.iter().zip(&plan.models) {
            assert!(url.ends_with(&format!("/models/{model}:countTokens")), "{url}");
        }

        // Санитайзинг съел всё явное (пустые строки) — дефолтный кандидат.
        let empty: Vec<String> = vec!["   ".into(), String::new()];
        let plan = build_check_plan(&p, &config, Some(&empty)).expect("plan");
        assert_eq!(plan.models, vec!["gemini-9.9-pro"], "первый gemini из списка провайдера");

        // Тела нет вовсе — прежнее поведение.
        let plan = build_check_plan(&p, &config, None).expect("plan");
        assert_eq!(plan.models, vec!["gemini-9.9-pro"]);
    }

    #[test]
    fn classify_key_check_verdicts() {
        assert_eq!(classify_key_check(Some(200), "{}"), CheckVerdict::Valid);
        assert_eq!(
            classify_key_check(
                Some(400),
                r#"{"error":{"message":"API key not valid. Please pass a valid API key.","status":"INVALID_ARGUMENT"}}"#
            ),
            CheckVerdict::Invalid,
            "текст ошибки Google распознаётся без регистра"
        );
        assert_eq!(
            classify_key_check(Some(400), r#"{"error":"bad request shape"}"#),
            CheckVerdict::Unknown,
            "прочий 400 — вина запроса, ключ не осуждаем"
        );
        assert_eq!(classify_key_check(Some(401), "unauthorized"), CheckVerdict::Invalid);
        assert_eq!(classify_key_check(Some(403), "forbidden"), CheckVerdict::Invalid);
        assert_eq!(classify_key_check(Some(429), "rate limited"), CheckVerdict::ValidLimited);
        assert_eq!(classify_key_check(Some(500), "boom"), CheckVerdict::Unknown);
        assert_eq!(classify_key_check(None, "сеть: timeout"), CheckVerdict::Unknown);
    }

    #[test]
    fn build_check_plan_rejects_nongemini_and_broken_relay() {
        let config = GatewayConfig::default();
        // Не-Gemini апстрим — отказ до сети.
        let wrong = provider("https://api.groq.com/openai/v1", &[]);
        assert!(build_check_plan(&wrong, &config, None).is_err());

        // Gemini без ключей: план строится (ключевые условия проверяет
        // run_plan перед сетью), отказ планирования — только про маршрут.
        let mut no_keys = provider("https://generativelanguage.googleapis.com/v1beta/openai", &[]);
        no_keys.keys[0].is_active = false;
        assert!(build_check_plan(&no_keys, &config, None).is_ok());

        // Релей установлен, но URL релея не настроен — жёсткая ошибка.
        let mut relayed = provider("https://generativelanguage.googleapis.com/v1beta/openai", &[]);
        relayed.use_relay = true;
        let err = build_check_plan(&relayed, &config, None).expect_err("релей без URL обязан не пустить");
        assert!(err.contains("релей"), "{err}");
    }

    #[test]
    fn detail_is_truncated() {
        let long = "x".repeat(1000);
        let cut = truncate_detail(&long);
        assert!(cut.chars().count() <= DETAIL_MAX_CHARS + 1, "{}", cut.chars().count());
        assert!(truncate_detail("  ok  ") == "ok", "края тела не сохраняются");
    }
}
