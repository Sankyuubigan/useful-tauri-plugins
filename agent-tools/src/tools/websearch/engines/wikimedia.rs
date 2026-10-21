//! Wikimedia Commons — движок Шага 3 (PRP): официальное MediaWiki API,
//! поиск файлов в namespace 6. Кейс юзера (`site:commons.wikimedia.org
//! File ...`), который DDG обрабатывал плохо.
//!
//! Маршрутизация ТОЛЬКО явная (PRP core §1.9, правка по живым фактам):
//! движок вызывается или по префиксам юзера (`site:commons.wikimedia.org`,
//! `File:` — через api_search::detect_prefix), или по явному
//! `engines=[commons]`. Из дефолт-цепочки он УБРАН, поэтому внутренней
//! детекции «профиль/не профиль» здесь НЕТ: что пришло — то и ищем.
//! Маркер `site:...` до этого отрезает strip_site_marker (в api_search).

use super::super::http;
use super::api_search::{fetch_json, push_nonempty};
use super::clip_chars;
use super::SearchResult;

/// Убрать маркер `site:commons.wikimedia.org` (по токенам); остаток —
/// текст запроса, уходящий в API (`srsearch`). Вызывается ДО движка —
/// на этапе префикс-детекта.
pub(super) fn strip_site_marker(query: &str) -> String {
    query
        .split_whitespace()
        .filter(|t| !t.to_lowercase().starts_with("site:commons.wikimedia.org"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) async fn search(query: &str, limit: usize) -> Result<Vec<SearchResult>, String> {
    let srsearch = query.trim();
    if srsearch.is_empty() {
        return Ok(Vec::new());
    }
    let url = format!(
        "https://commons.wikimedia.org/w/api.php?action=query&list=search&srnamespace=6&format=json&srlimit={}&srsearch={}&utf8=1",
        std::cmp::min(limit, 20),
        http::js_encode_uri_component(srsearch),
    );
    let json = fetch_json(&url, &[("Accept", "application/json")], 8000).await?;

    let mut out = Vec::new();
    for hit in json
        .get("query")
        .and_then(|q| q.get("search"))
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
    {
        let title = clip_chars(&str_field(hit, "title"), 200);
        if title.is_empty() {
            continue;
        }
        // URL страницы файла: кодирование через ':' допустимо (MediaWiki
        // принимает %3A) — так же, как encodeURIComponent в JS.
        let url = format!("https://commons.wikimedia.org/wiki/{}", http::js_encode_uri_component(&title));
        let snippet = clip_chars(
            &decode_entities(&http::strip_tags(&str_field(hit, "snippet"))),
            300,
        );
        push_nonempty(&mut out, title, url, snippet, "commons.wikimedia.org");
    }
    Ok(out)
}

fn str_field(v: &serde_json::Value, key: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).unwrap_or_default().to_string()
}

/// MediaWiki в `snippet` отдаёт HTML-мнемоники («7 Queen&#039;s Park Cres E»).
/// Чистим минимальный набор, который реально встречается в живых ответах.
fn decode_entities(s: &str) -> String {
    let mut out = s.to_string();
    for (entity, ch) in [
        ("&#039;", "'"),
        ("&#39;", "'"),
        ("&quot;", "\""),
        ("&nbsp;", " "),
        ("&amp;", "&"),
        ("&lt;", "<"),
        ("&gt;", ">"),
    ] {
        out = out.replace(entity, ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_marker_is_removed_for_api_query() {
        assert_eq!(strip_site_marker("site:commons.wikimedia.org File building windows"), "File building windows");
        assert_eq!(strip_site_marker("File:Building in Toronto.jpg"), "File:Building in Toronto.jpg");
        assert_eq!(strip_site_marker("site:commons.wikimedia.org"), "", "один маркер → пустой запрос");
    }

    /// Фикстура в форме живого ответа API (21.10.2026): title вида `File:...`,
    /// snippet с HTML-highlight лицензии, мнемоники декодируются.
    const FIXTURE: &str = r#"{
        "batchcomplete": "",
        "query": {
            "searchinfo": {"totalhits": 299493},
            "search": [
                {"ns": 6, "title": "File:Windows of the Frost Building (Toronto, Canada).jpg",
                 "pageid": 103025077, "size": 1324567,
                 "snippet": "Attribution-Share Alike 4.0 <span class=\"searchmatch\">Windows</span> of the Frost <span class=\"searchmatch\">Building</span>",
                 "timestamp": "2024-05-01T12:00:00Z"},
                {"ns": 6, "title": "", "snippet": "no title row"}
            ]
        }
    }"#;

    #[test]
    fn fixture_parses_file_title_license_snippet_and_skips_empty() {
        let json: serde_json::Value = serde_json::from_str(FIXTURE).expect("json");
        let mut out: Vec<SearchResult> = Vec::new();
        for hit in json
            .get("query")
            .and_then(|q| q.get("search"))
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
        {
            let title = clip_chars(&str_field(hit, "title"), 200);
            if title.is_empty() { continue; }
            let url = format!("https://commons.wikimedia.org/wiki/{}", http::js_encode_uri_component(&title));
            out.push(SearchResult {
                title,
                url,
                snippet: clip_chars(&decode_entities(&http::strip_tags(&str_field(hit, "snippet"))), 300),
                source: "commons.wikimedia.org".into(),
            });
        }
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "File:Windows of the Frost Building (Toronto, Canada).jpg");
        // encodeURIComponent НЕ кодирует скобки ( ) — они допустимы в URL
        // как есть; живой URL MediaWiki принимает.
        assert_eq!(
            out[0].url,
            "https://commons.wikimedia.org/wiki/File%3AWindows%20of%20the%20Frost%20Building%20(Toronto%2C%20Canada).jpg"
        );
        assert_eq!(out[0].snippet, "Attribution-Share Alike 4.0 Windows of the Frost Building");
        assert_eq!(out[0].source, "commons.wikimedia.org");
    }
}
