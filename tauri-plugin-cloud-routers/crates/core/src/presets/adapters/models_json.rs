//! Адаптер `models.dev/models.json` → подсказки моделей для наших провайдеров.
//!
//! Формат: плоская карта `"провайдер/модель" → метаданные`. Провайдеров и
//! base_url здесь нет — только характеристики моделей (контекст, capabilities).
//! Используется как фолбэк подсказок `default_models_hint`.
//!
//! Проверенные факты (2026-10-16), закодированные ниже как правила:
//! - ключ — `"провайдер/модель"` (`google/gemini-flash-latest`), берём часть
//!   после ПЕРВОГО `/` целиком (`meta/llama-4-...` — имя со слэшем валидно
//!   внутри id модели, но НЕ валидно как id провайдера; нам нужен только хвост);
//! - поле `type` есть только у не-чатовых (`embedding`, `reranking`, `video`),
//!   чатовые записи его не имеют вовсе — отсутствие `type` = чат;
//! - `modalities.output` бывает и строкой (`"text"`), и массивом (`["text"]`),
//!   проверяем оба представления, иначе половина записей теряется;
//! - чат = выход содержит `text` (генерация картинок/видео/аудио — не чат).
use super::super::sources::MODELS_HINT_LIMIT;

/// Разобрать тело `models.json` в карту `наш id провайдера → id моделей`.
///
/// Не-чатовые записи (эмбеддинги, реранк, видео/аудиогенерация) пропускаются:
/// предложить их как чат-модели значило бы соврать о возможностях провайдера.
pub fn parse_models_json_hints(raw: &str) -> Result<Vec<(String, Vec<String>)>, String> {
    let root: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("models.json: не JSON: {}", e))?;
    let map = root.as_object().ok_or_else(|| "models.json: нет карты моделей".to_string())?;
    if map.is_empty() {
        return Err("models.json: карта моделей пуста".to_string());
    }

    let mut by_preset: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for (key, entry) in map {
        let Some(obj) = entry.as_object() else { continue };
        if !is_chat_entry(obj) {
            continue;
        }
        let Some((provider, model)) = split_provider_model(key) else { continue };
        let provider = provider.trim();
        let model = model.trim();
        if provider.is_empty() || model.is_empty() {
            continue;
        }
        let models = by_preset.entry(provider.to_string()).or_default();
        if models.len() < MODELS_HINT_LIMIT && !models.iter().any(|m| m == model) {
            models.push(model.to_string());
        }
    }
    if by_preset.is_empty() {
        return Err("models.json: ни одной пригодной чат-модели".to_string());
    }
    Ok(by_preset.into_iter().collect())
}

/// Чатовая ли запись: нет метки не-чатового `type` И выход содержит `text`.
fn is_chat_entry(obj: &serde_json::Map<String, serde_json::Value>) -> bool {
    if let Some(t) = obj.get("type").and_then(|v| v.as_str()) {
        if !t.trim().is_empty() {
            return false;
        }
    }
    output_has_text(obj.get("modalities").and_then(|m| m.get("output")))
}

/// Выходные модальности содержат `text` (строка или массив строк).
fn output_has_text(output: Option<&serde_json::Value>) -> bool {
    match output {
        Some(serde_json::Value::String(s)) => s.split_whitespace().any(|w| w == "text"),
        Some(serde_json::Value::Array(items)) => {
            items.iter().filter_map(|v| v.as_str()).any(|s| s.split_whitespace().any(|w| w == "text"))
        }
        _ => false,
    }
}

/// Разрезать ключ `"провайдер/модель"` по ПЕРВОМУ слэшу.
///
/// Хвост может содержать слэши (`meta/llama-4-...-instruct`) — режем только
/// голову. Без слэша — не наша запись.
fn split_provider_model(key: &str) -> Option<(&str, &str)> {
    key.split_once('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Синтетическая фикстура в формате models.json. Значения вымышленные.
    const FIXTURE: &str = r#"{
        "acme/chat-x1": {"id":"acme/chat-x1","name":"Chat","modalities":{"input":["text"],"output":["text"]}},
        "acme/chat-x2": {"id":"acme/chat-x2","name":"Chat 2","modalities":{"input":"text","output":"text"}},
        "acme/embed-x1": {"id":"acme/embed-x1","type":"embedding","modalities":{"input":"text","output":"text"}},
        "acme/img-x1": {"id":"acme/img-x1","modalities":{"input":["text"],"output":["image"]}},
        "meta/llama-4-scout": {"id":"meta/llama-4-scout","modalities":{"input":["text"],"output":["text"]}},
        "noslash": {"id":"noslash","modalities":{"input":["text"],"output":["text"]}},
        "broken": 42
    }"#;

    #[test]
    fn chat_models_grouped_by_provider() {
        let hints = parse_models_json_hints(FIXTURE).expect("parse");
        let acme = hints.iter().find(|(id, _)| id == "acme").map(|(_, m)| m).expect("acme");
        assert!(acme.contains(&"chat-x1".to_string()), "{:?}", acme);
        assert!(acme.contains(&"chat-x2".to_string()), "{:?}", acme);
        assert!(!acme.iter().any(|m| m == "embed-x1"), "эмбеддинг — не чат: {acme:?}");
        assert!(!acme.iter().any(|m| m == "img-x1"), "картинки — не чат: {acme:?}");
        // Хвост со слэшем сохраняется целиком.
        let meta = hints.iter().find(|(id, _)| id == "meta").map(|(_, m)| m).expect("meta");
        assert_eq!(meta, &vec!["llama-4-scout".to_string()]);
    }

    #[test]
    fn rejects_broken_and_empty() {
        assert!(parse_models_json_hints("nope").is_err());
        assert!(parse_models_json_hints("{}").is_err(), "пустая карта запрещена");
        assert!(parse_models_json_hints(r#"{"a/b":{"type":"embedding"}}"#).is_err());
    }
}
