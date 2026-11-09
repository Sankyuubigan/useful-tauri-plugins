//! MDN — адресный API-движок (PRP Шаг 4): официальный поиск
//! developer.mozilla.org. Адресный: только префикс `mdn:` или явный engines.
//!
//! Живая проверка 21.10.2026: ответ ~8 мс, documents[]{title, mdn_url,
//! summary} — путь сборки `https://developer.mozilla.org{mdn_url}`.

use super::super::http;
use super::api_search::{fetch_json, push_nonempty};
use super::clip_chars;
use super::SearchResult;

pub(super) async fn search(query: &str, limit: usize) -> Result<Vec<SearchResult>, String> {
    let url = format!(
        "https://developer.mozilla.org/api/v1/search?q={}",
        http::js_encode_uri_component(query)
    );
    let json = fetch_json(&url, &[("Accept", "application/json")], 6000).await?;

    let mut out = Vec::new();
    for doc in json
        .get("documents")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .take(limit.max(3))
    {
        let title = clip_chars(&str_field(doc, "title"), 120);
        let path = str_field(doc, "mdn_url");
        if title.is_empty() || path.is_empty() {
            continue;
        }
        let url = format!("https://developer.mozilla.org{path}");
        let snippet = clip_chars(&http::strip_tags(&str_field(doc, "summary")), 300);
        push_nonempty(&mut out, title, url, snippet, "developer.mozilla.org");
    }
    Ok(out)
}

fn str_field(v: &serde_json::Value, key: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_json(text: &str) -> Result<Vec<SearchResult>, String> {
        let json: serde_json::Value =
            serde_json::from_str(text).map_err(|_| "невалидный JSON".to_string())?;
        let mut out = Vec::new();
        for doc in json.get("documents").and_then(|v| v.as_array()).into_iter().flatten() {
            let title = clip_chars(&str_field(doc, "title"), 120);
            let path = str_field(doc, "mdn_url");
            if title.is_empty() || path.is_empty() { continue; }
            out.push(SearchResult {
                title,
                url: format!("https://developer.mozilla.org{path}"),
                snippet: clip_chars(&http::strip_tags(&str_field(doc, "summary")), 300),
                source: "developer.mozilla.org".into(),
            });
        }
        Ok(out)
    }

    /// Фикстура в форме живого ответа (21.10.2026).
    const FIXTURE: &str = r#"{
        "documents": [
            {"title": "Compiling from Rust to WebAssembly",
             "mdn_url": "/en-US/docs/WebAssembly/Guides/Rust_to_Wasm",
             "summary": "This tutorial shows you how to compile a Rust project into WebAssembly.",
             "score": 61.69},
            {"summary": "no title here", "mdn_url": "/x"}
        ]
    }"#;

    #[test]
    fn fixture_joins_mdn_domain_with_path() {
        let out = parse_json(FIXTURE).expect("парсится");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "Compiling from Rust to WebAssembly");
        assert_eq!(out[0].url, "https://developer.mozilla.org/en-US/docs/WebAssembly/Guides/Rust_to_Wasm");
        assert_eq!(out[0].snippet, "This tutorial shows you how to compile a Rust project into WebAssembly.");
        assert_eq!(out[0].source, "developer.mozilla.org");
    }
}
