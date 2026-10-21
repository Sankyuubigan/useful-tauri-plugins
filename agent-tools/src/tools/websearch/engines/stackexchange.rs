//! StackExchange — адресный API-движок (PRP Шаг 4): excerpts-поиск по
//! stackoverflow.com. Адресный: срабатывает только по префиксу `so:`
//! или явному списку engines (см. api_search.rs).
//!
//! Живая проверка 21.10.2026: `quota_max: 300` запросов/день на IP без
//! ключа (core §2.6 соблюдён — keyless).

use super::super::http;
use super::api_search::{fetch_json, push_nonempty};
use super::clip_chars;
use super::SearchResult;

pub(super) async fn search(query: &str, limit: usize) -> Result<Vec<SearchResult>, String> {
    let url = format!(
        "https://api.stackexchange.com/2.3/search/excerpts?order=desc&sort=activity&q={}&site=stackoverflow&pagesize={}",
        http::js_encode_uri_component(query),
        std::cmp::min(limit, 20),
    );
    let json = fetch_json(
        &url,
        &[("Accept", "application/json")],
        6000,
    )
    .await?;

    let quota_empty = json.get("quota_remaining").and_then(|v| v.as_u64()) == Some(0)
        && json.get("items").and_then(|v| v.as_array()).map_or(true, |a| a.is_empty());
    if quota_empty {
        return Err("лимит StackExchange истёк (300/день без ключа)".to_string());
    }

    let mut out = Vec::new();
    for item in json
        .get("items")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
    {
        let title = clip_chars(&http::strip_tags(&str_field(item, "title")), 120);
        let qid = item.get("question_id").and_then(|v| v.as_u64());
        let Some(qid) = qid else { continue };
        // Поле link в excerpts-API отсутствует (проверено живьём 21.10.2026) —
        // канонический URL строится из question_id; сайт редиректит полный путь.
        let url = format!("https://stackoverflow.com/questions/{qid}");
        let snippet = clip_chars(&http::strip_tags(&str_field(item, "excerpt")), 400);
        push_nonempty(&mut out, title, url, snippet, "stackoverflow.com");
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
        for item in json.get("items").and_then(|v| v.as_array()).into_iter().flatten() {
            let title = clip_chars(&http::strip_tags(&str_field(item, "title")), 120);
            let Some(qid) = item.get("question_id").and_then(|v| v.as_u64()) else { continue };
            let url = format!("https://stackoverflow.com/questions/{qid}");
            let snippet = clip_chars(&http::strip_tags(&str_field(item, "excerpt")), 400);
            push_nonempty(&mut out, title, url, snippet, "stackoverflow.com");
        }
        Ok(out)
    }

    /// Фикстура в форме живого ответа API (21.10.2026): excerpt с
    /// highlight-тегами, link-поля нет.
    const FIXTURE: &str = r#"{
        "items": [
            {"title": "writing NUMA aware high performance programs in rust",
             "excerpt": "I would like to write <span class=\"highlight\">rust</span> async code",
             "item_type": "question", "question_id": 79908314},
            {"excerpt": "no title", "question_id": 42},
            {"title": "no id"}
        ],
        "has_more": true, "quota_max": 300, "quota_remaining": 298
    }"#;

    #[test]
    fn fixture_builds_url_from_question_id_and_strips_highlight() {
        let out = parse_json(FIXTURE).expect("парсится");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "writing NUMA aware high performance programs in rust");
        assert_eq!(out[0].url, "https://stackoverflow.com/questions/79908314");
        assert_eq!(out[0].snippet, "I would like to write rust async code");
        assert_eq!(out[0].source, "stackoverflow.com");
    }

    #[test]
    fn quota_exhausted_is_honest_error_only_when_empty() {
        let text = r#"{"items": [], "quota_remaining": 0}"#;
        let json: serde_json::Value = serde_json::from_str(text).expect("json");
        let empty = json.get("items").and_then(|v| v.as_array()).map_or(true, |a| a.is_empty());
        let quota_zero = json.get("quota_remaining").and_then(|v| v.as_u64()) == Some(0);
        assert!(empty && quota_zero, "условие честной ошибки срабатывает");
    }
}
