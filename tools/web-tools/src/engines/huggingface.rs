//! HuggingFace — адресный API-движок (PRP Шаг 4): официальный публичный
//! каталог моделей huggingface.co/api/models. Адресный: только префикс
//! `hf:` или явный engines — тематических эвристик нет (core §1.9).
//!
//! Живая проверка 21.10.2026: верхний уровень — массив, ключевые поля
//! modelId/tags/createdAt/downloads/pipeline_tag.

use super::super::http;
use super::api_search::{fetch_json, push_nonempty};
use super::clip_chars;
use super::SearchResult;

pub(super) async fn search(query: &str, limit: usize) -> Result<Vec<SearchResult>, String> {
    let url = format!(
        "https://huggingface.co/api/models?search={}&limit={}",
        http::js_encode_uri_component(query),
        std::cmp::min(limit, 20),
    );
    let json = fetch_json(&url, &[("Accept", "application/json")], 6000).await?;

    let mut out = Vec::new();
    // API отвечает массивом моделей на верхнем уровне.
    for model in json.as_array().into_iter().flatten() {
        let Some(model_id) = str_of(model.get("modelId")).filter(|m| !m.is_empty()) else { continue };
        let title = clip_chars(&model_id, 120);
        let url = format!("https://huggingface.co/{model_id}");
        // Честные метаданные каталога: пайплайн/вес скачиваний/дата/теги.
        let mut meta: Vec<String> = Vec::new();
        if let Some(tag) = str_of(model.get("pipeline_tag")).filter(|t| !t.is_empty()) {
            meta.push(format!("Пайплайн: {tag}"));
        }
        if let Some(d) = model.get("downloads").and_then(|v| v.as_u64()) {
            if d > 0 {
                meta.push(format!("Скачивания: {d}"));
            }
        }
        if let Some(likes) = model.get("likes").and_then(|v| v.as_u64()) {
            if likes > 0 {
                meta.push(format!(r"👍 {likes}"));
            }
        }
        let created = str_of(model.get("createdAt"))
            .map(|c| c.chars().take(10).collect::<String>())
            .unwrap_or_default();
        if !created.is_empty() {
            meta.push(format!("Создана: {created}"));
        }
        let mut snippet = meta.join(" · ");
        if let Some(tags) = model.get("tags").and_then(|v| v.as_array()) {
            let short: Vec<String> = tags
                .iter()
                .filter_map(|t| t.as_str().map(str::to_string))
                .take(3)
                .collect();
            if !short.is_empty() {
                if !snippet.is_empty() {
                    snippet.push_str(" · ");
                }
                snippet.push_str(&format!("Теги: {}", short.join(", ")));
            }
        }
        push_nonempty(&mut out, title, url, clip_chars(&snippet, 300), "huggingface.co");
    }
    Ok(out)
}

fn str_of(v: Option<&serde_json::Value>) -> Option<String> {
    v.and_then(|x| x.as_str()).map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_json(text: &str) -> Result<Vec<SearchResult>, String> {
        let json: serde_json::Value =
            serde_json::from_str(text).map_err(|_| "невалидный JSON".to_string())?;
        let mut out = Vec::new();
        for model in json.as_array().into_iter().flatten() {
            let model = model.as_object().expect("модель объект");
            let Some(model_id) = model.get("modelId").and_then(|v| v.as_str()).map(str::to_string).filter(|m| !m.is_empty()) else { continue };
            let mut meta: Vec<String> = Vec::new();
            if let Some(tag) = model.get("pipeline_tag").and_then(|v| v.as_str()).filter(|t| !t.is_empty()) {
                meta.push(format!("Пайплайн: {tag}"));
            }
            if let Some(d) = model.get("downloads").and_then(|v| v.as_u64()) {
                if d > 0 { meta.push(format!("Скачивания: {d}")); }
            }
            out.push(SearchResult {
                title: clip_chars(&model_id, 120),
                url: format!("https://huggingface.co/{model_id}"),
                snippet: meta.join(" · "),
                source: "huggingface.co".into(),
            });
        }
        Ok(out)
    }

    /// Фикстура в форме живого ответа (21.10.2026): верхний уровень — массив.
    const FIXTURE: &str = r#"[
        {"_id": "x", "createdAt": "2024-07-18T08:56:00.000Z", "downloads": 5255644,
         "likes": 42, "modelId": "meta-llama/Llama-3.1-8B-Instruct",
         "pipeline_tag": "text-generation",
         "tags": ["transformers", "safetensors", "llama"]},     {"modelId": "", "tags": []}
    ]"#;

    #[test]
    fn fixture_builds_model_url_and_honest_meta() {
        let out = parse_json(FIXTURE).expect("парсится");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "meta-llama/Llama-3.1-8B-Instruct");
        assert_eq!(out[0].url, "https://huggingface.co/meta-llama/Llama-3.1-8B-Instruct");
        assert_eq!(out[0].snippet, "Пайплайн: text-generation · Скачивания: 5255644");
        assert_eq!(out[0].source, "huggingface.co");
    }
}
