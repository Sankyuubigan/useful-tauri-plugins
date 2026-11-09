//! Juejin — порт `searchJuejin` из web_search.ts: JSON API search_api/v1.

use super::super::http;
use super::clip_chars;
use super::{http_status_error, SearchResult, UA_CHROME_112};

pub(super) async fn search(query: &str, limit: usize) -> Result<Vec<SearchResult>, String> {
    let url = format!(
        "https://api.juejin.cn/search_api/v1/search?aid=2608&uuid=7259393293459605051&spider=0&query={}&id_type=0&cursor=0&limit={}&search_type=0&sort_type=0&version=1",
        http::js_encode_uri_component(query),
        std::cmp::min(limit, 20)
    );
    let opts = http::RequestOpts {
        headers: vec![
            ("Host".into(), "api.juejin.cn".into()),
            ("User-Agent".into(), UA_CHROME_112.into()),
            ("Accept".into(), "application/json".into()),
        ],
        timeout_ms: Some(4000),
        ..Default::default()
    };
    let res = http::request(&url, &opts).await.map_err(|e| e.message)?;
    if res.status != 200 {
        return Err(http_status_error(res.status));
    }
    let json: serde_json::Value = serde_json::from_str(&res.text)
        .map_err(|_| "невалидный JSON".to_string())?;
    if json.get("err_no").map(|v| v != &serde_json::json!(0)).unwrap_or(true) {
        // TS: 'err_no=' + json.err_no — значение конкатенируется как есть
        // (строка → строка, null → 'null', bool → 'true', иных типов нет).
        let shown = match json.get("err_no") {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(serde_json::Value::Number(n)) => n.to_string(),
            Some(serde_json::Value::Bool(b)) => b.to_string(),
            Some(serde_json::Value::Null) => "null".to_string(),
            // Массив/объект в TS конкатенировались бы как «[object Object]» /
            // «,»-join — на практике err_no скалярен; JSON-формаShown — честный фолбэк.
            Some(other) => other.to_string(),
            None => "undefined".to_string(),
        };
        return Err(format!("err_no={shown}"));
    }

    let mut out = Vec::new();
    for item in json.get("data").and_then(|d| d.as_array()).into_iter().flatten() {
        let m = item.get("result_model").cloned().unwrap_or(serde_json::Value::Null);
        let info = m.get("article_info").cloned().unwrap_or(serde_json::Value::Null);
        let author = m
            .get("author_user_info")
            .and_then(|a| a.get("user_name"))
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        let article_id = m.get("article_id").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        let url2 = if article_id.is_empty() {
            String::new()
        } else {
            format!("https://juejin.cn/post/{article_id}")
        };
        // JS || пропускает пустые строки — первый непустой из двух.
        let raw_title = first_nonempty_str(item.get("title_highlight"), info.get("title"));
        let title = http::strip_tags(&raw_title);
        if title.is_empty() || url2.is_empty() {
            continue;
        }
        let desc = clip_chars(
            &http::strip_tags(&first_nonempty_str(item.get("content_highlight"), info.get("brief_content"))),
            300,
        );
        let mut meta_parts: Vec<String> = Vec::new();
        if let Some(cat) = str_of(m.get("category")) {
            meta_parts.push(format!("Категория: {cat}"));
        }
        if !author.is_empty() {
            meta_parts.push(format!("Автор: {author}"));
        }
        if let Some(digg) = m.get("article_info").and_then(|i| i.get("digg_count")).and_then(|v| v.as_u64()) {
            if digg > 0 {
                meta_parts.push(format!("👍 {digg}"));
            }
        }
        let meta = meta_parts.join(" · ");
        let mut snippet_parts: Vec<String> = Vec::new();
        if !desc.is_empty() {
            snippet_parts.push(desc);
        }
        if !meta.is_empty() {
            snippet_parts.push(meta);
        }
        out.push(SearchResult {
            title: clip_chars(&title, 200),
            url: url2,
            snippet: snippet_parts.join("\n"),
            source: if author.is_empty() { "juejin.cn".into() } else { author },
        });
    }
    Ok(out)
}

fn str_of(v: Option<&serde_json::Value>) -> Option<String> {
    v.and_then(|x| x.as_str()).map(|s| s.to_string()).filter(|s| !s.is_empty())
}

/// Порт `x || y` для JSON-строк: ненулевые/непустые строки выигрывают.
fn first_nonempty_str(a: Option<&serde_json::Value>, b: Option<&serde_json::Value>) -> String {
    str_of(a).or_else(|| str_of(b)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine_json(extra: &str) -> String {
        format!(r#"{{"err_no":0,"data":[{extra}]}}"#)
    }

    fn parse_json(text: &str) -> Result<Vec<SearchResult>, String> {
        let json: serde_json::Value = serde_json::from_str(text).map_err(|_| "невалидный JSON".to_string())?;
        if json.get("err_no").map(|v| v != &serde_json::json!(0)).unwrap_or(true) {
            let shown = match json.get("err_no") {
                Some(v @ serde_json::Value::Number(_)) => v.to_string(),
                Some(_) => "не число".to_string(),
                None => "undefined".to_string(),
            };
            return Err(format!("err_no={shown}"));
        }
        let mut out = Vec::new();
        for item in json.get("data").and_then(|d| d.as_array()).into_iter().flatten() {
            let m = item.get("result_model").cloned().unwrap_or(serde_json::Value::Null);
            let info = m.get("article_info").cloned().unwrap_or(serde_json::Value::Null);
            let author = m.get("author_user_info").and_then(|a| a.get("user_name"))
                .and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            let article_id = m.get("article_id").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            let url2 = if article_id.is_empty() { String::new() } else { format!("https://juejin.cn/post/{article_id}") };
            let raw_title = first_nonempty_str(item.get("title_highlight"), info.get("title"));
            let title = http::strip_tags(&raw_title);
            if title.is_empty() || url2.is_empty() { continue; }
            let desc = clip_chars(&http::strip_tags(&first_nonempty_str(item.get("content_highlight"), info.get("brief_content"))), 300);
            let mut meta_parts: Vec<String> = Vec::new();
            if let Some(cat) = str_of(m.get("category")) { meta_parts.push(format!("Категория: {cat}")); }
            if !author.is_empty() { meta_parts.push(format!("Автор: {author}")); }
            if let Some(digg) = m.get("article_info").and_then(|i| i.get("digg_count")).and_then(|v| v.as_u64()) {
                if digg > 0 { meta_parts.push(format!("👍 {digg}")); }
            }
            let meta = meta_parts.join(" · ");
            let mut snippet_parts: Vec<String> = Vec::new();
            if !desc.is_empty() { snippet_parts.push(desc); }
            if !meta.is_empty() { snippet_parts.push(meta); }
            out.push(SearchResult {
                title: clip_chars(&title, 200),
                url: url2,
                snippet: snippet_parts.join("\n"),
                source: if author.is_empty() { "juejin.cn".into() } else { author },
            });
        }
        Ok(out)
    }

    #[test]
    fn err_no_nonzero_gives_honest_error() {
        assert!(parse_json(r#"{"err_no":301,"data":[]}"#).unwrap_err() == "err_no=301");
        assert!(parse_json(r#"{}"#).unwrap_err() == "err_no=undefined");
    }

    #[test]
    fn fxiture_formats_meta_snippet_and_fallbacks() {
        let full = engine_json(r#"{
            "title_highlight": "Механика <b>Rust</b>",
            "content_highlight": "Про borrowing",
            "result_model": {"article_id": "111", "category": "backend",
              "author_user_info": {"user_name": "Ася"},
              "article_info": {"title": "не нужен", "brief_content": "не нужен", "digg_count": 3}}
        }"#);
        let out = parse_json(&full).expect("парсится");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "Механика Rust");
        assert_eq!(out[0].url, "https://juejin.cn/post/111");
        assert_eq!(out[0].snippet, "Про borrowing\nКатегория: backend · Автор: Ася · 👍 3");
        assert_eq!(out[0].source, "Ася");

        // item.title_highlight пуст -> фолбэк на article_info.title.
        let fb = engine_json(r#"{
            "title_highlight": "",
            "content_highlight": "",
            "result_model": {"article_id": "222",
              "author_user_info": {},
              "article_info": {"title": "Сырой title", "brief_content": "сырой текст"}}
        }"#);
        let out2 = parse_json(&fb).expect("парсится");
        assert_eq!(out2[0].title, "Сырой title");
        assert_eq!(out2[0].snippet, "сырой текст");
        // Без автора source=juejin (fallback), без digg_count мета-строки нет.
        assert_eq!(out2[0].source, "juejin.cn");

        // Нет article_id -> url пуст -> результат отброшен.
        let no_id = engine_json(r#"{"title_highlight": "X", "result_model": {}}"#);
        assert!(parse_json(&no_id).expect("парсится").is_empty());
    }
}
