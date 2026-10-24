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
//! - Запросы последовательные (по ключу за раз), таймаут 15 c на ключ,
//!   вердикты классифицирует чистая [`classify_key_check`].

use cloud_routers_core::config::{AccountKey, GatewayConfig, ProviderConfig};
use serde::Serialize;

use crate::app::AppState;
use crate::logbuf::mask_secret;
use crate::upstream::{HDR_RELAY_TOKEN, HDR_TARGET_URL, RELAY_PATH_PREFIX, relay_route_base};

/// Модель по умолчанию для проверки, если у провайдера список моделей пуст.
///
/// Это страховка, а не источник правды: правильную модель провайдеру даёт
/// кнопка «Обновить модели», а именно её результат затем выбирается.
pub const GEMINI_CHECK_FALLBACK_MODEL: &str = "gemini-2.0-flash";

/// Таймаут одного HTTP-запроса.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Хвост, до которого обрезается текст неудачного ответа в `detail`.
const DETAIL_MAX_CHARS: usize = 300;

/// Итог проверки одного ключа.
#[derive(Debug, Clone, Serialize)]
pub struct KeyCheckResult {
    pub key_id: String,
    /// Имя, заданное пользователем (может отсутствовать — см. `AccountKey`).
    pub name: Option<String>,
    /// Маска секрета (`AQ.Ab8…`) — секрет целиком наружу не уходит.
    pub masked: String,
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
    /// Модель, по которой шёл запрос (лог).
    pub model: String,
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

/// Кандидат модели для проверки: первый `gemini…` из списка провайдера,
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

/// Маршрут и цель проверки — всё, что вычисляется ДО сети.
#[derive(Debug, Clone)]
pub struct CheckPlan {
    pub model: String,
    pub via_relay: bool,
    /// Абсолютный URL цели: `{root}/models/{model}:countTokens`.
    pub target_url: String,
}

/// URL цели проверки для провайдера (модульный тест закрепляет его форму).
fn check_target_url(provider: &ProviderConfig) -> Result<(String, String), String> {
    let base = provider.base_url.trim();
    if base.is_empty() {
        return Err(format!(
            "у провайдера «{}» нет адреса — проверять нечего",
            provider.name
        ));
    }
    let model = model_candidate(provider);
    Ok((
        format!("{}/models/{model}:countTokens", gemini_native_root(base)),
        model,
    ))
}

/// Проверка маршрута и цели ДО сети. Все причины отказа — понятные строки,
/// которые попадают в тост дашборда без доработок.
pub fn build_check_plan(provider: &ProviderConfig, config: &GatewayConfig) -> Result<CheckPlan, String> {
    if !is_gemini_provider(provider) {
        return Err(format!(
            "«{}» — не Gemini-провайдер (апстрим не generativelanguage.googleapis.com): проверка через countTokens не подходит",
            provider.name
        ));
    }
    let (target, model) = check_target_url(provider)?;
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
    Ok(CheckPlan { model, via_relay, target_url: target })
}

/// Выполнить готовый [`CheckPlan`] против активных ключей провайдера.
///
/// Поднят в отдельную функцию ради интеграционных тестов: mock-апстрим живёт
/// на 127.0.0.1, а значит host-проверку [`is_gemini_provider`] он не проходит;
/// тест свободен собрать план на мок-URL и прогнать реальную сеть.
pub async fn run_plan(state: &AppState, provider: &ProviderConfig, plan: CheckPlan) -> Result<KeyCheckSummary, String> {
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
    let request_url = if plan.via_relay {
        let relay = relay_route_base(provider, &config)?;
        format!("{relay}{RELAY_PATH_PREFIX}")
    } else {
        plan.target_url.clone()
    };
    let relay_token = config.relay_token().filter(|t| !t.trim().is_empty());

    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| format!("не удалось собрать HTTP-клиент: {e}"))?;

    let mut results = Vec::with_capacity(keys.len());
    for item in &keys {
        // Последовательно по одному ключу: спешки нет, а «выстрелы пачкой» для релея и лимитов лишние.
        let (status, body) = send_one(&client, &request_url, plan.via_relay, &plan.target_url, relay_token.as_deref(), &item.key).await;
        let verdict = classify_key_check(status, &body);
        let masked = mask_secret(&item.key);
        // Логгер возвращает порядковый номер сообщения — значение не используется.
        let _ = match verdict {
            CheckVerdict::Valid => state.events().info(format!(
                "ключ {} {}: валиден (HTTP {})",
                provider.id,
                &masked,
                status.unwrap_or(0)
            )),
            CheckVerdict::ValidLimited => state.events().info(format!(
                "ключ {} {}: валиден, сейчас в лимите (HTTP 429)",
                provider.id, &masked
            )),
            CheckVerdict::Invalid => state.events().warn(format!(
                "ключ {} {}: отвергнут (HTTP {})",
                provider.id, &masked,
                status.unwrap_or(0)
            )),
            CheckVerdict::Unknown => state.events().warn(format!(
                "ключ {} {}: непонятный ответ (HTTP {}) — ключ не осуждён",
                provider.id,
                &masked,
                status.unwrap_or(0)
            )),
        };
        results.push(KeyCheckResult {
            key_id: item.id.clone(),
            name: item.name.clone(),
            masked,
            status,
            verdict,
            detail: truncate_detail(&body),
        });
    }

    let (valid, limited, invalid, unknown) = counts(&results);
    state.events().info(format!(
        "проверка ключей «{}» через {}: валидных {valid}, в лимите {limited}, невалидных {invalid}, неизвестно {unknown}",
        provider.name,
        if plan.via_relay { "релей" } else { "напрямую" }
    ));

    Ok(KeyCheckSummary { model: plan.model, via_relay: plan.via_relay, results })
}

/// Полная проверка провайдера: план → запуск. Её вызывает хендлер админки.
pub async fn check_provider_keys(
    state: &AppState,
    provider: &ProviderConfig,
) -> Result<KeyCheckSummary, String> {
    let plan = build_check_plan(provider, &state.config())?;
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

    /// Кодовая гарантия нулевого расхода лимита: построить цель, отличную
    /// от `…:countTokens`, нельзя — ни `generateContent`, ни `embedContent`.
    #[test]
    fn target_url_is_count_tokens_only() {
        let base = "https://generativelanguage.googleapis.com/v1beta/openai";
        let (target, _) = check_target_url(&provider(base, &["gemini-2.5-flash"])).expect("plan");
        assert!(
            target.ends_with(":countTokens"),
            "цель обязана быть ровно countTokens: {target}"
        );
        assert!(target.contains("generativelanguage.googleapis.com"), "{target}");
        assert!(!target.contains("generateContent"), "{target}");
        assert!(!target.contains("embedContent"), "{target}");
        assert!(!target.contains("openai"), "нативный путь без /openai: {target}");
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
        assert!(build_check_plan(&wrong, &config).is_err());

        // Gemini без ключей: план строится (ключевые условия проверяет
        // run_plan перед сетью), отказ планирования — только про маршрут.
        let mut no_keys = provider("https://generativelanguage.googleapis.com/v1beta/openai", &[]);
        no_keys.keys[0].is_active = false;
        assert!(build_check_plan(&no_keys, &config).is_ok());

        // Релей установлен, но URL релея не настроен — жёсткая ошибка.
        let mut relayed = provider("https://generativelanguage.googleapis.com/v1beta/openai", &[]);
        relayed.use_relay = true;
        let err = build_check_plan(&relayed, &config).expect_err("релей без URL обязан не пустить");
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
