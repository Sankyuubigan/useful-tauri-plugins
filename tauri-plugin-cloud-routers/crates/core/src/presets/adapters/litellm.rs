//! Адаптер LiteLLM → подсказки моделей для наших провайдеров.
//!
//! Источник: `model_prices_and_context_window.json`. Формат: плоская карта
//! `"запись модели" → { "litellm_provider", "mode", "max_input_tokens", ... }`.
//!
//! Проверенные факты (2026-10-16), закодированные ниже как правила:
//! - **ни одна** из ~4100 записей не содержит `api_base` → как каталог
//!   провайдеров (кому какой base_url) этот файл **не годится**;
//! - ключ карты — не всегда чистый id модели: у части провайдеров это
//!   `"<префикс-провайдера>/<модель>"` (`nebius/deepseek-ai/DeepSeek-R1`,
//!   `vertex_ai/gemini-3-pro-preview`, `gemini/gemini-embedding-001`), у других
//!   (`openai`, `anthropic`) — чистый id (`claude-haiku-4-5`, `ft:gpt-3.5-turbo`);
//! - `mode` бывает не-чатовым (`image_generation`, `embedding`, `moderation`,
//!   `rerank`, `guardrail`, ...) → в подсказки провайдера берём только чатовые
//!   режимы (`chat`, `completion`, `responses`), иначе пользователю предложат
//!   эмбеддинг как чат-модель;
//! - нужен обратный маппинг `litellm_provider → наш id провайдера`: таблица
//!   [`LITELLM_PROVIDER_TO_PRESET`] ниже.
use super::super::sources::MODELS_HINT_LIMIT;

/// Обратный маппинг: значение `litellm_provider` → id провайдера в нашем каталоге.
///
/// Таблица проверена по реальным ключам файла (2026-10-16). Покрывает
/// провайдеров, у которых формат ключей отличается от нашего id.
const LITELLM_PROVIDER_TO_PRESET: &[(&str, &str)] = &[
    ("gemini", "google"),
    ("vertex_ai", "google"),
    ("vertex_ai-language-models", "google"),
    ("together_ai", "together"),
    ("fireworks_ai", "fireworks"),
    ("deepinfra", "deepinfra"),
    ("azure_ai", "azure"),
    ("novita", "novita"),
    ("nebius", "nebius"),
    ("perplexity", "perplexity"),
    ("dashscope", "dashscope"),
    ("cohere", "cohere"),
    ("xai", "xai"),
    ("mistral", "mistral"),
    ("openai", "openai"),
    ("anthropic", "anthropic"),
    ("groq", "groq"),
    ("cerebras", "cerebras"),
];

/// Разобрать файл LiteLLM в карту `наш id провайдера → id моделей`.
///
/// Пропускает записи без `litellm_provider`, записи вне таблицы маппинга и
/// не-чатовые `mode`. Пустой результат — ошибка, а не пустая карта: иначе шлюз
/// записал бы кэш «ноль подсказок» поверх рабочего.
pub fn parse_litellm_hints(raw: &str) -> Result<Vec<(String, Vec<String>)>, String> {
    let root: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("litellm: не JSON: {}", e))?;
    let map = root.as_object().ok_or_else(|| "litellm: нет карты моделей".to_string())?;
    if map.is_empty() {
        return Err("litellm: карта моделей пуста".to_string());
    }

    let mut by_preset: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for (key, entry) in map {
        let Some(obj) = entry.as_object() else { continue };
        let Some(provider) = obj.get("litellm_provider").and_then(|v| v.as_str()) else {
            continue
        };
        let Some(preset_id) = provider_to_preset(provider) else { continue };
        if !is_chat_mode(obj.get("mode").and_then(|v| v.as_str())) {
            continue;
        }
        let model_id = strip_provider_prefix(provider, key);
        let model_id = model_id.trim();
        if model_id.is_empty() {
            continue;
        }
        let models = by_preset.entry(preset_id.to_string()).or_default();
        if models.len() < MODELS_HINT_LIMIT && !models.iter().any(|m| m == model_id) {
            models.push(model_id.to_string());
        }
    }
    if by_preset.is_empty() {
        return Err("litellm: ни одной пригодной чат-модели".to_string());
    }
    let mut out: Vec<(String, Vec<String>)> = by_preset.into_iter().collect();
    for (_, models) in out.iter_mut() {
        models.sort();
    }
    Ok(out)
}

/// Значение `litellm_provider` → наш id провайдера.
fn provider_to_preset(litellm_provider: &str) -> Option<&'static str> {
    LITELLM_PROVIDER_TO_PRESET
        .iter()
        .find(|(lp, _)| *lp == litellm_provider)
        .map(|(_, preset)| *preset)
}

/// Только чатовые режимы попадают в подсказки провайдера.
///
/// Эмбеддинги, модерация, реранк и генерация картинок — не чат: предложить их
/// как чат-модели значило бы соврать пользователю о возможностях провайдера.
fn is_chat_mode(mode: Option<&str>) -> bool {
    matches!(mode, Some("chat") | Some("completion") | Some("responses") | None)
}

/// Убрать префикс провайдера из ключа записи.
///
/// Правила по проверенным фактам: `nebius/deepseek-ai/DeepSeek-R1` →
/// `deepseek-ai/DeepSeek-R1`, `vertex_ai/gemini-3-pro-preview` → `gemini-3-pro-preview`.
/// Чистые id (`claude-haiku-4-5`) возвращаются как есть.
fn strip_provider_prefix(litellm_provider: &str, key: &str) -> String {
    for sep in ['/', ':'] {
        if let Some((head, tail)) = key.split_once(sep) {
            if head == litellm_provider && !tail.is_empty() {
                let tail = tail.trim();
                if !tail.is_empty() {
                    return tail.to_string();
                }
            }
        }
    }
    key.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Синтетическая фикстура в формате litellm. Значения вымышленные.
    const FIXTURE: &str = r#"{
        "gemini/gemini-flash-x1": {"litellm_provider": "gemini", "mode": "chat", "max_input_tokens": 100},
        "vertex_ai/gemini-pro-x2": {"litellm_provider": "vertex_ai", "mode": "completion"},
        "vertex_ai-plain": {"litellm_provider": "vertex_ai-language-models", "mode": "responses"},
        "plain-chat": {"litellm_provider": "openai", "mode": "chat"},
        "embed-1": {"litellm_provider": "openai", "mode": "embedding"},
        "moderate-1": {"litellm_provider": "openai", "mode": "moderation"},
        "no-provider": {"mode": "chat"},
        "unknown-vendor": {"litellm_provider": "mystery_cloud", "mode": "chat"},
        "broken-entry": 42
    }"#;

    fn hints_for<'a>(hints: &'a [(String, Vec<String>)], preset: &str) -> &'a Vec<String> {
        hints.iter().find(|(id, _)| id == preset).map(|(_, m)| m).expect("preset hints")
    }

    #[test]
    fn prefixed_keys_are_stripped_and_grouped() {
        let hints = parse_litellm_hints(FIXTURE).expect("parse");
        let google = hints_for(&hints, "google");
        assert!(google.contains(&"gemini-flash-x1".to_string()), "{:?}", google);
        assert!(google.contains(&"gemini-pro-x2".to_string()), "{:?}", google);
        assert!(google.contains(&"vertex_ai-plain".to_string()), "{:?}", google);
    }

    #[test]
    fn non_chat_modes_are_excluded() {
        let hints = parse_litellm_hints(FIXTURE).expect("parse");
        let openai = hints_for(&hints, "openai");
        assert_eq!(openai, &vec!["plain-chat".to_string()]);
    }

    #[test]
    fn unknown_and_broken_entries_are_skipped_not_fatal() {
        // `no-provider`, `unknown-vendor`, `broken-entry` молча пропускаются,
        // остальные записи дают результат.
        let hints = parse_litellm_hints(FIXTURE).expect("parse");
        assert!(hints.iter().any(|(id, _)| id == "google"));
        assert!(hints.iter().any(|(id, _)| id == "openai"));
    }

    #[test]
    fn rejects_broken_and_empty() {
        assert!(parse_litellm_hints("nope").is_err());
        assert!(parse_litellm_hints("{}").is_err(), "пустая карта запрещена");
        assert!(parse_litellm_hints(r#"{"a":{"mode":"embedding"}}"#).is_err());
    }

    #[test]
    fn hints_are_capped_and_deduplicated() {
        let entries: Vec<String> = (0..MODELS_HINT_LIMIT + 10)
            .map(|i| format!("\"m{i:03}\":{{\"litellm_provider\":\"groq\",\"mode\":\"chat\"}}"))
            .collect();
        // Дубликат: JSON схлопывает одинаковые ключи, добавляем «dup» дважды.
        let raw = format!(
            "{{{entries},\"dup\":{{\"litellm_provider\":\"groq\",\"mode\":\"chat\"}},\"dup\":{{\"litellm_provider\":\"groq\",\"mode\":\"chat\"}}}}",
            entries = entries.join(",")
        );
        let hints = parse_litellm_hints(&raw).expect("parse");
        let groq = hints_for(&hints, "groq");
        assert_eq!(groq.len(), MODELS_HINT_LIMIT, "лимит режет хвост");
        assert!(groq.contains(&"dup".to_string()));
    }
}
