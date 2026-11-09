//! Exa — порт `searchExa` из web_search.ts: внутренний keyless-эндпоинт
//! exa.ai/search/api/search-fast (POST с фиксированным payload).

use super::super::http;
use super::clip_chars;
use super::{http_status_error, SearchResult};

pub(super) async fn search(query: &str, limit: usize) -> Result<Vec<SearchResult>, String> {
    // Порядок полей фиксирован как в TS (JSON.stringify({...})).
    let payload = serde_json::json!({
        "numResults": limit,
        "query": query,
        "type": "auto",
        "useAutoprompt": true,
        "domainFilterType": "include",
        "text": true,
        "density": "compact",
        "resolvedSearchType": "neural",
        "moderation": true,
        "fastMode": false,
        "rerankerType": "default",
    })
    .to_string();
    let opts = http::RequestOpts {
        method: Some("POST".into()),
        body: Some(payload),
        timeout_ms: Some(4000),
        headers: vec![
            ("content-type".into(), "text/plain;charset=UTF-8".into()),
            ("origin".into(), "https://exa.ai".into()),
            ("referer".into(), "https://exa.ai/".into()),
            ("sec-fetch-site".into(), "same-origin".into()),
            ("sec-fetch-mode".into(), "cors".into()),
            ("sec-fetch-dest".into(), "empty".into()),
            ("Accept".into(), "*/*".into()),
        ],
        ..Default::default()
    };
    let res = http::request("https://exa.ai/search/api/search-fast", &opts)
        .await
        .map_err(|e| e.message)?;
    if res.status != 200 {
        return Err(http_status_error(res.status));
    }
    let json: serde_json::Value =
        serde_json::from_str(&res.text).map_err(|_| "невалидный JSON".to_string())?;

    let mut out = Vec::new();
    for r in json.get("results").and_then(|d| d.as_array()).into_iter().flatten() {
        let Some(url) = str_field(r, "url").filter(|u| !u.is_empty()) else { continue };
        let Some(title_raw) = str_field(r, "title").filter(|t| !t.is_empty()) else { continue };
        // Основной сниппет — из поля text (payload просит "text": true);
        // раньше он выбрасывался и «Описание» оставалось пустым или
        // мета-строкой про автора.
        let mut snippet_parts: Vec<String> = Vec::new();
        if let Some(text) = str_field(r, "text").filter(|t| !t.is_empty()) {
            snippet_parts.push(clip_chars(&http::strip_tags(&text), 300));
        }
        let mut meta_parts: Vec<String> = Vec::new();
        if let Some(author) = str_field(r, "author").filter(|a| !a.is_empty()) {
            meta_parts.push(format!("Автор: {author}"));
        }
        if let Some(date) = str_field(r, "publishedDate").filter(|d| !d.is_empty()) {
            meta_parts.push(format!("Опубликовано: {date}"));
        }
        if !meta_parts.is_empty() {
            snippet_parts.push(meta_parts.join(". "));
        }
        let source = url::Url::parse(&url)
            .map(|u| u.host_str().unwrap_or_default().to_string())
            .unwrap_or_default();
        out.push(SearchResult {
            title: clip_chars(&title_raw, 200),
            url,
            snippet: snippet_parts.join("\n"),
            source,
        });
    }
    Ok(out)
}

fn str_field(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_json(text: &str) -> Result<Vec<SearchResult>, String> {
        let json: serde_json::Value = serde_json::from_str(text).map_err(|_| "невалидный JSON".to_string())?;
        let mut out = Vec::new();
        for r in json.get("results").and_then(|d| d.as_array()).into_iter().flatten() {
            let Some(url) = str_field(r, "url").filter(|u| !u.is_empty()) else { continue };
            let Some(title_raw) = str_field(r, "title").filter(|t| !t.is_empty()) else { continue };
            let mut snippet_parts: Vec<String> = Vec::new();
            if let Some(text) = str_field(r, "text").filter(|t| !t.is_empty()) {
                snippet_parts.push(clip_chars(&http::strip_tags(&text), 300));
            }
            let mut meta_parts: Vec<String> = Vec::new();
            if let Some(author) = str_field(r, "author").filter(|a| !a.is_empty()) {
                meta_parts.push(format!("Автор: {author}"));
            }
            if let Some(date) = str_field(r, "publishedDate").filter(|d| !d.is_empty()) {
                meta_parts.push(format!("Опубликовано: {date}"));
            }
            if !meta_parts.is_empty() {
                snippet_parts.push(meta_parts.join(". "));
            }
            out.push(SearchResult {
                title: clip_chars(&title_raw, 200),
                url: url.clone(),
                snippet: snippet_parts.join("\n"),
                source: url::Url::parse(&url).map(|u| u.host_str().unwrap_or_default().to_string()).unwrap_or_default(),
            });
        }
        Ok(out)
    }

    #[test]
    fn fixture_builds_meta_line_and_hostname_source() {
        let text = r#"{"results":[
            {"url": "https://exa-blog.test/post", "title": "Neural search deep dive",
             "text": "Neural search explainer body", "author": "Иванов", "publishedDate": "2024-01-05"},
            {"url": "", "title": "no url"},
            {"url": "https://b.test/x", "title": ""}
        ]}"#;
        let out = parse_json(text).expect("парсится");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "Neural search deep dive");
        assert_eq!(out[0].url, "https://exa-blog.test/post");
        assert_eq!(out[0].snippet, "Neural search explainer body\nАвтор: Иванов. Опубликовано: 2024-01-05");
        assert_eq!(out[0].source, "exa-blog.test");
    }

    #[test]
    fn fixture_without_text_keeps_meta_only_snippet() {
        let text = r#"{"results":[
            {"url": "https://exa-blog.test/post", "title": "Neural search deep dive",
             "author": "Иванов", "publishedDate": "2024-01-05"}
        ]}"#;
        let out = parse_json(text).expect("парсится");
        assert_eq!(out[0].snippet, "Автор: Иванов. Опубликовано: 2024-01-05");
    }
}
