//! Детектор и проверка ключей OpenRouter.
//!
//! SSOT для всего, что специфично для OpenRouter (счётчик `:free`-запросов
//! в [`crate::free_usage`], проверка ключей): по образцу
//! `is_gemini_provider` ([`crate::key_check`]) — host/base_url, а не
//! protocol или id: пользователь переименовывает и пересоздаёт провайдеров.
//!
//! Пресет OpenRouter уже в каталоге (`assets/provider-presets.json`,
//! base_url `https://openrouter.ai/api/v1`).
//!
//! ## Схема эндпоинта ключей
//!
//! `GET {base_url}/key` (Bearer-ключ) возвращает
//! `{ "data": { label, usage, usage_daily, usage_weekly, usage_monthly,
//! limit, limit_remaining, limit_reset, is_free_tier, … } }`. API НЕ отдаёт
//! счётчик бесплатных запросов (аккаунтный дневной лимит — в деньгах), это
//! чинит локальный счётчик [`crate::free_usage`]; предполагаемый кап
//! выводится из тиров ключей ([`free_usage_cap`]).

use cloud_routers_core::config::{GatewayConfig, ProviderConfig};
use serde::{Deserialize, Serialize};

use crate::key_check::{CheckPlan, CheckVerdict, KeyCheckKind};

/// Провайдер — OpenRouter? (base_url содержит `openrouter.ai`).
pub fn is_openrouter_provider(p: &ProviderConfig) -> bool {
    p.base_url.contains("openrouter.ai")
}

/// Цель проверки ключей: `GET {base_url}/key` (эндпоинт ключевой, не модельный).
pub fn key_info_target_url(base_url: &str) -> Result<String, String> {
    let base = base_url.trim();
    if base.is_empty() {
        return Err("у провайдера нет адреса — проверять нечего".to_string());
    }
    Ok(format!("{base}/key"))
}

/// Порядок платформы по дневному числу бесплатных запросов: ключи free-тира
/// — 50, платные — 1000. Числа только для отображения капа (жёсткого гейта
/// нет), API самого числа не отдаёт.
pub const FREE_DAILY_CAP_FREE_KEY: u64 = 50;
pub const FREE_DAILY_CAP_PAID_KEY: u64 = 1000;

/// Данные ключа из `data` ответа `/api/v1/key`. Отсутствующие/null поля —
/// честный `None`: рисовать «—» честнее, чем выдумывать 0.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OpenRouterKeyInfo {
    /// Тир ключа платформы: `false` — платный, `true` — бесплатный.
    pub is_free_tier: Option<bool>,
    /// Потрачено всего, $.
    pub usage: Option<f64>,
    /// Потрачено за сегодня, $.
    pub usage_daily: Option<f64>,
    /// Аккаунтный лимит расхода, $.
    pub limit: Option<f64>,
    /// Остаток лимита расхода, $.
    pub limit_remaining: Option<f64>,
    /// Как часто обнуляется лимит (`daily`/`weekly`/`monthly`/…).
    /// Незнакомое значение отображается как есть — не паникуем.
    pub limit_reset: Option<String>,
}

/// Конверт ответа `/api/v1/key`: данные лежат под ключом `data`.
#[derive(Debug, Deserialize)]
struct KeyEnvelope {
    data: Option<OpenRouterKeyInfo>,
}

/// Распарсить тело ответа `/api/v1/key`. `None` — тело не JSON, `data`
/// отсутствует или null: ответ не о ключе (мусор релея, HTML-страница и т.п.).
pub fn parse_key_info(body: &str) -> Option<OpenRouterKeyInfo> {
    serde_json::from_str::<KeyEnvelope>(body).ok()?.data
}

/// Классификация ответа OpenRouter: 200 «валиден» только с распарсиваемой
/// `data` — мусорный 200 (HTML-страница релея и т.п.) ключ не осуждает.
pub fn classify_openrouter(status: Option<u16>, body: &str) -> CheckVerdict {
    match status {
        Some(200) if parse_key_info(body).is_some() => CheckVerdict::Valid,
        Some(401 | 403) => CheckVerdict::Invalid,
        Some(429) => CheckVerdict::ValidLimited,
        _ => CheckVerdict::Unknown,
    }
}

/// Кап дневной квоты бесплатных запросов по тирам проверенных ключей.
///
/// Кап определяется аккаунтом: платный ключ (`is_free_tier == false`)
/// поднимает кап до [`FREE_DAILY_CAP_PAID_KEY`]; все проверенные ключи
/// free-тира — [`FREE_DAILY_CAP_FREE_KEY`]. Тир неизвестен хотя бы у одного
/// ключа (401, мусорный ответ, поле отсутствует) — кап выставлять наугад
/// нельзя: `None` (в UI — «лимит неизвестен»). Одна функция-вывод: кап не
/// хардкодится в двух местах.
pub fn free_usage_cap(infos: &[Option<OpenRouterKeyInfo>]) -> Option<u64> {
    let tiers: Vec<bool> = infos.iter().flatten().filter_map(|i| i.is_free_tier).collect();
    if tiers.iter().any(|&free| !free) {
        return Some(FREE_DAILY_CAP_PAID_KEY);
    }
    // Все ключи прошли проверку (каждый дал info) и у всех известный free-тир.
    if infos.len() == tiers.len() && !tiers.is_empty() {
        return Some(FREE_DAILY_CAP_FREE_KEY);
    }
    None
}

/// План проверки ключей OpenRouter: один `GET {base}/key` на ключ.
///
/// `models = vec!["key"]` — цикл zip в [`crate::key_check::run_plan`] требует
/// пары (модель, URL); «key» здесь маркер цели, а не модель — эндпоинт
/// ключевой, не модельный.
pub fn build_openrouter_plan(
    provider: &ProviderConfig,
    config: &GatewayConfig,
) -> Result<CheckPlan, String> {
    let target = key_info_target_url(&provider.base_url)?;
    let via_relay = provider.relay_id.is_some();
    if via_relay {
        // Общий релей-гейт обеих проверок (SSOT причины отказа).
        crate::key_check::ensure_relay_route(provider, config)?;
    }
    Ok(CheckPlan {
        kind: KeyCheckKind::OpenRouterKeyInfo,
        models: vec!["key".into()],
        via_relay,
        target_urls: vec![target],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(base: &str) -> ProviderConfig {
        ProviderConfig {
            id: "x".into(),
            name: "X".into(),
            base_url: base.into(),
            ..Default::default()
        }
    }

    #[test]
    fn detector_matches_openrouter_base_urls() {
        assert!(is_openrouter_provider(&provider("https://openrouter.ai/api/v1")));
        assert!(is_openrouter_provider(&provider("https://openrouter.ai/api/v1/")));
        assert!(!is_openrouter_provider(&provider("https://api.groq.com/openai/v1")));
        assert!(!is_openrouter_provider(&provider("")));
    }

    #[test]
    fn key_target_url_is_get_key_endpoint() {
        let t = key_info_target_url("https://openrouter.ai/api/v1").expect("url");
        assert_eq!(t, "https://openrouter.ai/api/v1/key");
        assert!(t.ends_with("/key"), "эндпоинт ключевой: {t}");
        assert!(!t.contains("countTokens"), "не модельный: {t}");
        assert!(!t.contains("chat/completions"), "лимит генерации не расходуется: {t}");
        assert!(key_info_target_url("   ").is_err(), "пустая база — отказ");
    }

    #[test]
    fn parse_key_info_reads_data_and_honest_none() {
        // Реальная схема ответа (сверена по фикстурам CodexBar / issue #6842).
        let body = r#"{"data":{"label":"my-key","is_free_tier":true,"usage":0.42,"usage_daily":0.03,
            "usage_weekly":0.2,"usage_monthly":0.4,"limit":10.0,"limit_remaining":9.97,
            "limit_reset":"daily","include_byok_in_limit":true}}"#;
        let info = parse_key_info(body).expect("parsed");
        assert_eq!(info.is_free_tier, Some(true));
        assert_eq!(info.usage, Some(0.42));
        assert_eq!(info.usage_daily, Some(0.03));
        assert_eq!(info.limit, Some(10.0));
        assert_eq!(info.limit_remaining, Some(9.97));
        assert_eq!(info.limit_reset.as_deref(), Some("daily"));

        // Отсутствующие поля → None; незнакомый limit_reset доходит как есть.
        let partial = parse_key_info(r#"{"data":{"limit_reset":"weekly","extra_field":123}}"#)
            .expect("parsed");
        assert_eq!(partial.is_free_tier, None);
        assert_eq!(partial.usage, None);
        assert_eq!(partial.limit_reset.as_deref(), Some("weekly"));

        // data: null / без data / не JSON — ответ не о ключе.
        assert_eq!(parse_key_info(r#"{"data":null}"#), None);
        assert_eq!(parse_key_info(r#"{"foo":1}"#), None);
        assert_eq!(parse_key_info("<html>page</html>"), None);
    }

    #[test]
    fn classify_openrouter_verdicts() {
        let good = r#"{"data":{"is_free_tier":true}}"#;
        assert_eq!(classify_openrouter(Some(200), good), CheckVerdict::Valid);
        // Мусорный 200 — не валидность (ответ пришёл, но не о ключе).
        assert_eq!(classify_openrouter(Some(200), "<html>page</html>"), CheckVerdict::Unknown);
        assert_eq!(classify_openrouter(Some(200), r#"{"data":null}"#), CheckVerdict::Unknown);
        assert_eq!(classify_openrouter(Some(200), r#"{"foo":1}"#), CheckVerdict::Unknown);
        assert_eq!(
            classify_openrouter(Some(401), r#"{"error":{"message":"Invalid key"}}"#),
            CheckVerdict::Invalid
        );
        assert_eq!(classify_openrouter(Some(403), "forbidden"), CheckVerdict::Invalid);
        assert_eq!(classify_openrouter(Some(429), "rate limited"), CheckVerdict::ValidLimited);
        assert_eq!(classify_openrouter(Some(500), "boom"), CheckVerdict::Unknown);
        assert_eq!(classify_openrouter(None, "сеть: timeout"), CheckVerdict::Unknown);
    }

    #[test]
    fn free_usage_cap_from_tiers() {
        let free = Some(OpenRouterKeyInfo { is_free_tier: Some(true), ..Default::default() });
        let paid = Some(OpenRouterKeyInfo { is_free_tier: Some(false), ..Default::default() });
        let unknown_tier = Some(OpenRouterKeyInfo { is_free_tier: None, ..Default::default() });
        let no_info: Option<OpenRouterKeyInfo> = None;

        assert_eq!(free_usage_cap(&[free.clone()]), Some(FREE_DAILY_CAP_FREE_KEY), "free — 50");
        assert_eq!(free_usage_cap(&[free.clone(), free.clone()]), Some(FREE_DAILY_CAP_FREE_KEY));
        assert_eq!(
            free_usage_cap(&[paid.clone(), free.clone()]),
            Some(FREE_DAILY_CAP_PAID_KEY),
            "платный ключ поднимает кап до 1000"
        );
        assert_eq!(free_usage_cap(&[paid.clone()]), Some(FREE_DAILY_CAP_PAID_KEY));
        assert_eq!(free_usage_cap(&[free.clone(), no_info.clone()]), None, "401 — тир неизвестен");
        assert_eq!(free_usage_cap(&[free, unknown_tier]), None, "поле absent — тир неизвестен");
        assert_eq!(free_usage_cap(&[no_info.clone(), no_info]), None);
        assert_eq!(free_usage_cap(&[]), None, "пусто — неизвестно");
    }

    #[test]
    fn build_openrouter_plan_rejects_dangling_relay() {
        let config = GatewayConfig::default();
        let mut p = provider("https://openrouter.ai/api/v1");
        p.relay_id = Some("ghost".into());
        let err = build_openrouter_plan(&p, &config).expect_err("релей без слота обязан не пустить");
        assert!(err.contains("ghost"), "{err}");
    }
}
