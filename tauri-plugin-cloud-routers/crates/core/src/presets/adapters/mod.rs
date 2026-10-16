//! Адаптеры внешних форматов каталога провайдеров.
//!
//! Каждый внешний источник говорит на своём языке: `models.dev` —
//! `{id, name, doc, api, models}`, `litellm` — цены и характеристики моделей
//! вообще без привязки к endpoints. Адаптеры переводят их в **наш** формат
//! [`super::ProviderPresetsCatalog`]. Прямого чтения чужих форматов шлюзом нет:
//! иначе чужой релиз-цикл (переименование поля, переезд файла) ломал бы наш
//! парсер — именно от этого защищает собственный формат (см. [`super`]).

pub mod litellm;
pub mod models_dev;
pub mod models_json;

use models_dev::{parse_models_dev, ExternalProvider};

/// Формат тела источника каталога.
///
/// Классификатор — в `core`, а не в шлюзе: разбор тел — доменное знание без
/// сети, а шлюз только качает байты. Проверенные дискриминаторы (2026-10-16):
/// карта провайдерских записей (у каждой ключ `models`) — models.dev-формат;
/// плоская карта с `litellm_provider` — litellm; плоская карта
/// `"провайдер/модель"` — models.json.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceBodyKind {
    /// Провайдеры с моделями (`api.json`, `catalog.json`).
    ModelsDev,
    /// Цены и характеристики (`model_prices_and_context_window.json`).
    Litellm,
    /// Характеристики моделей (`models.json`).
    ModelsJson,
    /// Ни один известный формат.
    Unknown,
}

/// Классифицировать тело источника до разбора.
///
/// Пустая карта — `Unknown`, а не «формат без записей»: иначе пустой ответ
/// сервера выглядел бы валидным источником.
pub fn classify_source_body(raw: &str) -> SourceBodyKind {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(raw) else {
        return SourceBodyKind::Unknown;
    };
    let map = root.get("providers").unwrap_or(&root);
    let Some(map) = map.as_object() else { return SourceBodyKind::Unknown };
    if map.is_empty() {
        return SourceBodyKind::Unknown;
    }
    let mut saw_models_dev = false;
    let mut saw_litellm = false;
    let mut saw_models_json = false;
    for (key, entry) in map {
        let Some(obj) = entry.as_object() else { continue };
        if obj.get("models").and_then(|v| v.as_object()).is_some() {
            saw_models_dev = true;
        } else if obj.get("litellm_provider").and_then(|v| v.as_str()).is_some() {
            saw_litellm = true;
        } else if key.contains('/') {
            saw_models_json = true;
        }
        // Два формата в одном теле — такого не бывает; первое свидетельство
        // решает, дальше не смотрим.
        if saw_models_dev || saw_litellm || saw_models_json {
            break;
        }
    }
    if saw_models_dev {
        SourceBodyKind::ModelsDev
    } else if saw_litellm {
        SourceBodyKind::Litellm
    } else if saw_models_json {
        SourceBodyKind::ModelsJson
    } else {
        SourceBodyKind::Unknown
    }
}

/// Разобрать тело models.dev-формата в провайдеров.
///
/// Вызывать только для [`SourceBodyKind::ModelsDev`]: классификатор уже
/// доказал, что тело — провайдерская карта.
pub fn parse_models_dev_body(raw: &str) -> Result<Vec<ExternalProvider>, String> {
    parse_models_dev(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifier_tells_formats_apart() {
        let models_dev = r#"{"acme":{"name":"A","api":"https://a.example/v1","models":{"m":{}}}}"#;
        assert_eq!(classify_source_body(models_dev), SourceBodyKind::ModelsDev);
        let wrapped = r#"{"providers":{"acme":{"name":"A","api":"https://a.example/v1","models":{"m":{}}}}}"#;
        assert_eq!(classify_source_body(wrapped), SourceBodyKind::ModelsDev);
        let litellm = r#"{"m1":{"litellm_provider":"openai","mode":"chat"}}"#;
        assert_eq!(classify_source_body(litellm), SourceBodyKind::Litellm);
        let models_json = r#"{"acme/m1":{"id":"acme/m1","modalities":{"input":["text"],"output":["text"]}}}"#;
        assert_eq!(classify_source_body(models_json), SourceBodyKind::ModelsJson);
    }

    #[test]
    fn classifier_rejects_garbage() {
        assert_eq!(classify_source_body("nope"), SourceBodyKind::Unknown);
        assert_eq!(classify_source_body("{}"), SourceBodyKind::Unknown);
        assert_eq!(classify_source_body(r#"{"providers":{}}"#), SourceBodyKind::Unknown);
        assert_eq!(classify_source_body("[1,2]"), SourceBodyKind::Unknown);
        // Карта без единого известного ключа — не формат, а мусор.
        assert_eq!(classify_source_body(r#"{"a":1,"b":2}"#), SourceBodyKind::Unknown);
    }
}
