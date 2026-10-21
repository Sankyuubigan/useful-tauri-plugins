//! crates.io — адресный API-движок (PRP Шаг 4): официальный публичный
//! поиск crates.io/api/v1/crates. Адресный: только префикс `crates:` или
//! явный engines. Тул сам Rust-стек — профильный запрос частый кейс.

use super::super::http;
use super::api_search::{fetch_json, push_nonempty};
use super::clip_chars;
use super::SearchResult;

pub(super) async fn search(query: &str, limit: usize) -> Result<Vec<SearchResult>, String> {
    let url = format!(
        "https://crates.io/api/v1/crates?q={}&per_page={}",
        http::js_encode_uri_component(query),
        std::cmp::min(limit, 20),
    );
    let json = fetch_json(
        &url,
        &[("Accept", "application/json")],
        6000,
    )
    .await?;

    let mut out = Vec::new();
    for crate_obj in json
        .get("crates")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
    {
        let Some(id) = str_of(crate_obj.get("id")).filter(|i| !i.is_empty()) else { continue };
        let title = clip_chars(&str_of(crate_obj.get("name")).unwrap_or_else(|| id.clone()), 80);
        let url = format!("https://crates.io/crates/{id}");
        // Честные метаданные: описание; при его отсутствии — репозиторий.
        let mut snippet = http::strip_tags(&str_of(crate_obj.get("description")).unwrap_or_default());
        if snippet.is_empty() {
            snippet = str_of(crate_obj.get("repository")).unwrap_or_default();
        }
        if let Some(ver) = str_of(crate_obj.get("default_version")).filter(|v| !v.is_empty()) {
            if !snippet.is_empty() {
                snippet.push_str(" — версия ");
            }
            snippet.push_str(&ver);
        }
        push_nonempty(&mut out, title, url, clip_chars(&snippet, 300), "crates.io");
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
        for crate_obj in json.get("crates").and_then(|v| v.as_array()).into_iter().flatten() {
            let Some(id) = crate_obj.get("id").and_then(|v| v.as_str()).map(str::to_string).filter(|i| !i.is_empty()) else { continue };
            let title = crate_obj.get("name").and_then(|v| v.as_str()).unwrap_or(&id).to_string();
            let mut snippet = crate_obj.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if snippet.is_empty() {
                snippet = crate_obj.get("repository").and_then(|v| v.as_str()).unwrap_or("").to_string();
            }
            if let Some(ver) = crate_obj.get("default_version").and_then(|v| v.as_str()).filter(|v| !v.is_empty()) {
                if !snippet.is_empty() { snippet.push_str(" — версия "); }
                snippet.push_str(ver);
            }
            out.push(SearchResult {
                title,
                url: format!("https://crates.io/crates/{id}"),
                snippet,
                source: "crates.io".into(),
            });
        }
        Ok(out)
    }

    /// Фикстура в форме живого ответа (21.10.2026).
    const FIXTURE: &str = r#"{
        "crates": [
            {"id": "pyo3-async-runtimes", "name": "pyo3-async-runtimes",
             "description": "PyO3 bridges from Rust runtimes to Python's Asyncio library",
             "repository": "https://github.com/PyO3/pyo3-async-runtimes",
             "default_version": "0.22.0"},
            {"id": "no-desc", "name": "no-desc", "description": null,
             "repository": "https://github.com/x/nd"},
            {"id": "", "name": "empty id"}
        ],
        "meta": {"total": 13780}
    }"#;

    #[test]
    fn fixture_builds_crate_url_and_version_meta() {
        let out = parse_json(FIXTURE).expect("парсится");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].title, "pyo3-async-runtimes");
        assert_eq!(out[0].url, "https://crates.io/crates/pyo3-async-runtimes");
        assert_eq!(
            out[0].snippet,
            "PyO3 bridges from Rust runtimes to Python's Asyncio library — версия 0.22.0"
        );
        assert_eq!(out[0].source, "crates.io");
        // Нет описания → честный фолбэк на repository.
        assert_eq!(out[1].snippet, "https://github.com/x/nd");
    }
}
