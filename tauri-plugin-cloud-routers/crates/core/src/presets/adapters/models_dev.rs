//! Адаптер `models.dev` → наш формат каталога.
//!
//! Источники: `api.json` (провайдеры с endpoints и моделями) и `catalog.json`
//! (тот же набор, обёрнутый в `{"providers": {...}}`). Формат записи:
//! `{ "<id>": { "id", "env", "npm", "name", "doc", "api"?, "models": {...} } }`.
//!
//! Разбор идёт через `serde_json::Value`, а не строгие структуры: чужие поля
//! могут переименоваться в любом релизе models.dev, и это не должно ронять
//! весь sync. Берём только то, что понимаем, остальное игнорируем.
//!
//! Проверенные факты (2026-10-16), закодированные ниже как правила:
//! - поля `api` нет у `google`, `groq`, `cerebras`, `openai`, `anthropic`,
//!   `mistral`, `cohere`, `xai`, `togetherai`, `azure` → таким провайдерам
//!   base_url подставляет слияние из overrides, а не адаптер;
//! - `api` бывает шаблоном с `${VAR}` (`neon`, `databricks`, `snowflake-cortex`,
//!   `infomaniak`) → как литеральный base_url непригоден, отбрасывается;
//! - id `wafer.ai` содержит точку → slugify, иначе не пройдёт `validate`;
//! - `deepseek` публикует `api` без `/v1` → данные источника как есть, правки
//!   curated-значений — в слиянии, а не здесь.

use super::super::sources::MODELS_HINT_LIMIT;
use super::super::{is_valid_provider_id, slugify, unique_provider_id};

/// Провайдер, извлечённый из внешнего источника, до слияния с overrides.
///
/// `api` здесь — сырое значение из источника: может отсутствовать или быть
/// шаблоном `${VAR}`. Решение «брать или отбросить» принимает слияние
/// ([`super::super::sources::merge_catalogs`]), а не адаптер.
#[derive(Debug, Clone, PartialEq)]
pub struct ExternalProvider {
    /// id, уже приведённый к нашему алфавиту и уникализированный в источнике.
    pub id: String,
    /// Отображаемое имя (fallback — id, если источник имени не дал).
    pub name: String,
    /// Сырой `api` источника (`None` — поле отсутствует).
    pub api: Option<String>,
    /// Ссылка на документацию (`doc` источника).
    pub api_docs_url: Option<String>,
    /// id моделей источника, обрезанные до [`MODELS_HINT_LIMIT`].
    pub models: Vec<String>,
}

/// Разобрать тело `api.json` или `catalog.json` в список провайдеров.
///
/// Обе обёртки принимаются: `catalog.json` отдаёт `{"providers": {...}}`,
/// `api.json` — сразу карту провайдеров. Пустой/битый JSON — ошибка с текстом,
/// а не паника: вызывающий (шлюз) пробует следующий источник.
///
/// Дискриминатор провайдерской записи — ключ `models` (карта моделей).
/// Проверено 2026-10-16: в `api.json` он есть у 232/232 записей, а в телах
/// `models.json` (0/552) и litellm (ни у одной) его нет. Без этого требования
/// диспетчер «успешно» разбирал чужие форматы как тысячи провайдеров с пустыми
/// моделями — и подсказки из настоящих фолбэков терялись (поймано E2E).
pub fn parse_models_dev(raw: &str) -> Result<Vec<ExternalProvider>, String> {
    let root: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| format!("models.dev: не JSON: {}", e))?;
    let providers_map = root
        .get("providers")
        .unwrap_or(&root)
        .as_object()
        .ok_or_else(|| "models.dev: нет карты провайдеров".to_string())?;
    if providers_map.is_empty() {
        return Err("models.dev: нет ни одного провайдера".to_string());
    }

    let mut out: Vec<ExternalProvider> = Vec::with_capacity(providers_map.len());
    let mut used_ids: Vec<String> = Vec::with_capacity(providers_map.len());
    for (raw_id, entry) in providers_map {
        let Some(obj) = entry.as_object() else {
            continue; // запись не объект — пропускаем, а не роняем весь sync
        };
        if obj.get("models").and_then(|v| v.as_object()).is_none() {
            continue; // не провайдерская запись (тело litellm/models.json) — не наше
        }
        let id = unique_provider_id(&used_ids, &slug_if_needed(raw_id));
        used_ids.push(id.clone());

        let name = obj
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| id.clone());
        let api = obj
            .get("api")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        let api_docs_url = obj
            .get("doc")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        let models = obj
            .get("models")
            .and_then(|v| v.as_object())
            .map(|m| {
                m.keys()
                    .map(|k| k.trim().to_string())
                    .filter(|k| !k.is_empty())
                    .take(MODELS_HINT_LIMIT)
                    .collect()
            })
            .unwrap_or_default();

        out.push(ExternalProvider { id, name, api, api_docs_url, models });
    }
    if out.is_empty() {
        return Err("models.dev: ни одной пригодной записи провайдера".to_string());
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Привести id источника к нашему алфавиту, если он ему не соответствует.
///
/// `wafer.ai` → `wafer-ai`. Валидный id возвращается как есть: slugify
/// идемпотентен на валидных значениях, но явная проверка честнее.
fn slug_if_needed(raw_id: &str) -> String {
    if is_valid_provider_id(raw_id) {
        raw_id.to_string()
    } else {
        slugify(raw_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Синтетическая фикстура в формате models.dev: покрывает все закодированные
    /// правила. Значения вымышленные, живых URL здесь нет.
    const FIXTURE: &str = r#"{
        "acme": {"id":"acme","name":"Acme AI","doc":"https://docs.acme.example/models","api":"https://api.acme.example/v1","models":{"m1":{},"m2":{}}},
        "no-api": {"id":"no-api","name":"No Api","models":{"m1":{}}},
        "templated": {"id":"templated","name":"Templated","api":"https://${TEMPLATED_HOST}/v1","models":{}},
        "wafer.ai": {"id":"wafer.ai","name":"Wafer AI","api":"https://api.wafer.example/v1","models":{}},
        "nameless": {"id":"nameless","api":"https://api.nameless.example/v1","models":{}},
        "not-an-object": 42
    }"#;

    #[test]
    fn parses_acme_shape() {
        let list = parse_models_dev(FIXTURE).expect("parse");
        assert_eq!(list.len(), 5, "запись-не-объект пропускается, остальные берутся");
        let acme = list.iter().find(|p| p.id == "acme").expect("acme");
        assert_eq!(acme.name, "Acme AI");
        assert_eq!(acme.api.as_deref(), Some("https://api.acme.example/v1"));
        assert_eq!(acme.api_docs_url.as_deref(), Some("https://docs.acme.example/models"));
        assert_eq!(acme.models, vec!["m1".to_string(), "m2".to_string()]);
    }

    #[test]
    fn dotted_id_is_slugified() {
        let list = parse_models_dev(FIXTURE).expect("parse");
        assert!(list.iter().any(|p| p.id == "wafer-ai"), "точка обязана стать дефисом");
        assert!(!list.iter().any(|p| p.id.contains('.')));
    }

    #[test]
    fn missing_name_falls_back_to_id() {
        let list = parse_models_dev(FIXTURE).expect("parse");
        let p = list.iter().find(|p| p.id == "nameless").expect("nameless");
        assert_eq!(p.name, "nameless");
    }

    #[test]
    fn raw_api_is_preserved_verbatim_for_merge() {
        // Адаптер НЕ решает за слияние: сырое значение (включая шаблон)
        // передаётся дальше, отбрасывание — в `merge_catalogs`.
        let list = parse_models_dev(FIXTURE).expect("parse");
        let no_api = list.iter().find(|p| p.id == "no-api").expect("no-api");
        assert_eq!(no_api.api, None);
        let templated = list.iter().find(|p| p.id == "templated").expect("templated");
        assert_eq!(templated.api.as_deref(), Some("https://${TEMPLATED_HOST}/v1"));
    }

    #[test]
    fn catalog_wrapper_is_unwrapped() {
        let wrapped = format!(r#"{{"providers":{}}}"#, FIXTURE);
        let a = parse_models_dev(FIXTURE).expect("parse");
        let b = parse_models_dev(&wrapped).expect("parse wrapped");
        assert_eq!(a, b, "catalog.json и api.json дают один результат");
    }

    #[test]
    fn rejects_broken_and_empty() {
        assert!(parse_models_dev("nope").is_err());
        assert!(parse_models_dev(r#"{"providers":{}}"#).is_err(), "пустая карта запрещена");
        assert!(parse_models_dev(r#"{"x":1}"#).is_err(), "нет карты провайдеров");
    }

    #[test]
    fn models_hint_is_capped() {
        let models: String =
            (0..MODELS_HINT_LIMIT + 20).map(|i| format!("\"m{i}\":{{}}")).collect::<Vec<_>>().join(",");
        let raw = format!(r#"{{"big":{{"name":"Big","api":"https://big.example/v1","models":{{{models}}}}}}}"#);
        let list = parse_models_dev(&raw).expect("parse");
        assert_eq!(list[0].models.len(), MODELS_HINT_LIMIT);
    }

    #[test]
    fn duplicate_ids_after_slugify_are_uniquified() {
        let raw = r#"{"a.b": {"name":"A","api":"https://a.example/v1","models":{}}, "a-b": {"name":"B","api":"https://b.example/v1","models":{}}}"#;
        let list = parse_models_dev(raw).expect("parse");
        let mut ids: Vec<&str> = list.iter().map(|p| p.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 2, "коллизия после slugify обязана разрешиться суффиксом");
    }

    #[test]
    fn foreign_bodies_are_rejected_not_misparsed() {
        // Регрессия против E2E-дефекта: тела litellm и models.json разбирались
        // как тысячи «провайдеров» с пустыми моделями, и подсказки терялись.
        // Дискриминатор — ключ `models` (проверено: api.json 232/232,
        // models.json 0/552, litellm ни у одной записи).
        let litellm_like =
            r#"{"gemini-flash-x1":{"litellm_provider":"gemini","mode":"chat","max_input_tokens":100}}"#;
        let err = parse_models_dev(litellm_like).expect_err("litellm-тело обязано отвергаться");
        assert!(err.contains("пригодной записи"), "{err}");
        let models_like =
            r#"{"sao10k/l3-lunaris-8b":{"id":"sao10k/l3-lunaris-8b","name":"Lunaris","modalities":{"input":["text"],"output":["text"]}}}"#;
        let err = parse_models_dev(models_like).expect_err("models.json-тело обязано отвергаться");
        assert!(err.contains("пригодной записи"), "{err}");
    }
}
