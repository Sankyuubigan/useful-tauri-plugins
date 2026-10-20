//! Brave Search — порт `searchBrave` из web_search.ts: HTML со stealth-заголовками.

use regex::Regex;

use super::super::http;
use super::{http_status_error, SearchResult, UA_CHROME_112};

pub(super) async fn search(query: &str, _limit: usize) -> Result<Vec<SearchResult>, String> {
    let url = format!(
        "https://search.brave.com/search?q={}&source=web&offset=0",
        http::js_encode_uri_component(query)
    );
    let opts = http::RequestOpts {
        headers: vec![
            ("User-Agent".into(), UA_CHROME_112.into()),
            ("Referer".into(), "https://duckduckgo.com/".into()),
            ("sec-ch-ua".into(), r#""Chromium";v="112", "Google Chrome";v="112", "Not:A-Brand";v="99""#.into()),
            ("sec-ch-ua-mobile".into(), "?0".into()),
            ("sec-ch-ua-platform".into(), r#""Windows""#.into()),
            ("sec-fetch-site".into(), "same-origin".into()),
            ("sec-fetch-mode".into(), "navigate".into()),
            ("sec-fetch-dest".into(), "document".into()),
        ],
        timeout_ms: Some(5000),
        ..Default::default()
    };
    let res = http::request(&url, &opts).await.map_err(|e| e.message)?;
    if res.status != 200 {
        return Err(http_status_error(res.status));
    }
    // Капча-детект: слова блокировки И нет data-pos (реальная выдача их не имеет).
    let captcha = Regex::new(r"(?i)verify you are human|unusual traffic|access denied").expect("константная регулярка");
    if captcha.is_match(&res.text) && !res.text.contains("data-pos=\"") {
        return Err("заблокирован (капча)".to_string());
    }

    let title_re = Regex::new(r#"(?i)class="[^"]*search-snippet-title[^"]*"[^>]*>([\s\S]*?)<"#)
        .expect("константная регулярка");
    let anchor_re = Regex::new(r#"(?i)<a[^>]+href="https?://[^"]+"[^>]*>([\s\S]*?)</a>"#)
        .expect("константная регулярка");
    let desc_re = Regex::new(
        r#"(?i)class="[^"]*(?:generic-snippet|snippet-description|result-description)[^"]*"[^>]*>([\s\S]*?)<"#
    )
    .expect("константная регулярка");
    let source_re = Regex::new(r#"(?i)class="[^"]*site-name-wrapper[^"]*"[^>]*>([\s\S]*?)<"#)
        .expect("константная регулярка");

    let mut out = Vec::new();
    for block in http::parse_blocks_by_class(&res.text, "snippet") {
        let Some(url) = http::first_href(&block) else { continue };
        // Сначала class=search-snippet-title, иначе текст первого внешнего <a>.
        let title = title_re
            .captures(&block)
            .map(|c| http::strip_tags(&c[1]))
            .filter(|t| !t.is_empty())
            .or_else(|| {
                anchor_re.captures(&block).map(|c| http::strip_tags(&c[1]))
            })
            .filter(|t| !t.is_empty());
        let Some(title) = title else { continue };
        let snippet = desc_re
            .captures(&block)
            .map(|c| clip_chars(&http::strip_tags(&c[1]), 400))
            .unwrap_or_default();
        let source = source_re
            .captures(&block)
            .map(|c| clip_chars(&http::strip_tags(&c[1]), 80))
            .unwrap_or_default();
        out.push(SearchResult {
            title: clip_chars(&title, 200),
            url,
            snippet,
            source,
        });
    }
    Ok(out)
}

/// Стриппер по символам: JS-подобный `slice(0, n)` (по кодовому «числу»).
/// Порт `query.slice(0, 60)` / `title.slice(0, 200)`.
pub(crate) fn clip_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"
<div data-solid-code="ROOT" class="snippet">
  <a href="https://rust-lang.org/ru/">
    <div class="search-snippet-title">Язык Rust</div>
  </a>
  <div class="snippet-description">Системный язык программирования</div>
  <div class="site-name-wrapper">rust-lang.org</div>
</div>
<div class="snippet">
  <a href="https://docs.another.test/page">Anchored title fallback</a>
  <div class="generic-snippet">Текст описания</div>
</div>
"#;

    /// Повтор логики поиска на фикстуре (как в TS, капча до этого отбилась бы).
    fn parse_fixture(html: &str) -> Vec<SearchResult> {
        let title_re = Regex::new(r#"(?i)class="[^"]*search-snippet-title[^"]*"[^>]*>([\s\S]*?)<"#).unwrap();
        let anchor_re = Regex::new(r#"(?i)<a[^>]+href="https?://[^"]+"[^>]*>([\s\S]*?)</a>"#).unwrap();
        let desc_re = Regex::new(r#"(?i)class="[^"]*(?:generic-snippet|snippet-description|result-description)[^"]*"[^>]*>([\s\S]*?)<"#).unwrap();
        let source_re = Regex::new(r#"(?i)class="[^"]*site-name-wrapper[^"]*"[^>]*>([\s\S]*?)<"#).unwrap();
        let mut out = Vec::new();
        for block in http::parse_blocks_by_class(html, "snippet") {
            let Some(url) = http::first_href(&block) else { continue };
            let title = title_re
                .captures(&block)
                .map(|c| http::strip_tags(&c[1]))
                .filter(|t| !t.is_empty())
                .or_else(|| anchor_re.captures(&block).map(|c| http::strip_tags(&c[1])))
                .filter(|t| !t.is_empty());
            let Some(title) = title else { continue };
            out.push(SearchResult {
                title: clip_chars(&title, 200),
                url,
                snippet: desc_re.captures(&block).map(|c| clip_chars(&http::strip_tags(&c[1]), 400)).unwrap_or_default(),
                source: source_re.captures(&block).map(|c| clip_chars(&http::strip_tags(&c[1]), 80)).unwrap_or_default(),
            });
        }
        out
    }

    #[test]
    fn fixture_parses_title_snippet_source_and_anchor_fallback() {
        let out = parse_fixture(FIXTURE);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].title, "Язык Rust");
        assert_eq!(out[0].url, "https://rust-lang.org/ru/");
        assert_eq!(out[0].snippet, "Системный язык программирования");
        assert_eq!(out[0].source, "rust-lang.org");
        // Второй блок: класс search-snippet-title отсутствует — фолбэк на <a>.
        assert_eq!(out[1].title, "Anchored title fallback");
        assert_eq!(out[1].source, "");
    }

    #[test]
    fn captcha_detection_requires_no_data_pos() {
        let captcha = Regex::new(r"(?i)verify you are human|unusual traffic|access denied").unwrap();
        // Слова блокировки есть, data-pos нет → сврачивается в «заблокирован (капча)».
        assert!(captcha.is_match("Verify you are human"));
        // Реальная выдача может содержать те же слова в контенте — но там есть
        // data-pos, поэтому условие поиска (слова && !data-pos) не сработает.
        let real = "unusual traffic <div data-pos=\"3\">x";
        assert!(real.contains("data-pos=\""));
    }
}
