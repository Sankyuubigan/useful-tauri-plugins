//! npm — адресный API-движок (PRP Шаг 4): официальный публичный search
//! registry.npmjs.org. Адресный: только префикс `npm:` или явный engines.
//!
//! Живая проверка 21.10.2026: безлимитный публичный API, поля
//! objects[].package{name, description, links}.

use super::super::http;
use super::api_search::{fetch_json, push_nonempty};
use super::clip_chars;
use super::SearchResult;

pub(super) async fn search(query: &str, limit: usize) -> Result<Vec<SearchResult>, String> {
    let url = format!(
        "https://registry.npmjs.org/-/v1/search?text={}&size={}",
        http::js_encode_uri_component(query),
        std::cmp::min(limit, 20),
    );
    let json = fetch_json(&url, &[("Accept", "application/json")], 6000).await?;

    let mut out = Vec::new();
    for obj in json
        .get("objects")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
    {
        let package = obj.get("package").cloned().unwrap_or(serde_json::Value::Null);
        let name = str_of(package.get("name"));
        let Some(name) = name.filter(|n| !n.is_empty()) else { continue };
        let title = clip_chars(&name, 120);
        let url = format!("https://www.npmjs.com/package/{name}");
        let snippet = clip_chars(&http::strip_tags(&str_of(package.get("description")).unwrap_or_default()), 300);
        push_nonempty(&mut out, title, url, snippet, "npmjs.com");
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
        for obj in json.get("objects").and_then(|v| v.as_array()).into_iter().flatten() {
            let package = obj.get("package").cloned().unwrap_or(serde_json::Value::Null);
            let Some(name) = str_of(package.get("name")).filter(|n| !n.is_empty()) else { continue };
            out.push(SearchResult {
                title: clip_chars(&name, 120),
                url: format!("https://www.npmjs.com/package/{name}"),
                snippet: clip_chars(
                    &http::strip_tags(&str_of(package.get("description")).unwrap_or_default()),
                    300,
                ),
                source: "npmjs.com".into(),
            });
        }
        Ok(out)
    }

    /// Фикстура в форме живого ответа (21.10.2026).
    const FIXTURE: &str = r#"{
        "objects": [
            {"downloads": {"monthly": 2032917}, "searchScore": 116.8,
             "package": {"name": "uniffi-bindgen-react-native",
                         "description": "Uniffi bindings generator for Rust",
                         "version": "0.31.0-6",
                         "links": {"homepage": "https://github.com/x/y"}}},
            {"package": {"description": "no name"}},
            {"package": {"name": ""}}
        ],
        "total": 275131
    }"#;

    #[test]
    fn fixture_builds_npmjs_url_and_keeps_description() {
        let out = parse_json(FIXTURE).expect("парсится");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "uniffi-bindgen-react-native");
        assert_eq!(out[0].url, "https://www.npmjs.com/package/uniffi-bindgen-react-native");
        assert_eq!(out[0].snippet, "Uniffi bindings generator for Rust");
        assert_eq!(out[0].source, "npmjs.com");
    }
}
