//! Startpage — порт `searchStartpage` из web_search.ts.
//!
//! Работает через sc-токен с главной, POST /sp/search, гипервалентный
//! приоритет: Anubis PoW-челлендж (JS proof-of-work) без браузера не проходится
//! и в TS, и здесь — движок честно отказывается.

use regex::Regex;

use super::super::http;
use super::{http_status_error, SearchResult};

pub(super) async fn search(query: &str, _limit: usize) -> Result<Vec<SearchResult>, String> {
    let home = http::request("https://www.startpage.com/", &http::RequestOpts {
        timeout_ms: Some(6000),
        ..Default::default()
    })
    .await
    .map_err(|e| e.message)?;
    if home.status != 200 {
        return Err(http_status_error(home.status));
    }
    if home.text.contains("anubis_challenge") || home.text.contains("anubis") {
        return Err("Anubis PoW-челлендж — нужен браузер, движок не работает без него".to_string());
    }
    // sc-токен: name="sc" value=... ИЛИ value=... name="sc" (два порядка).
    let sc = match token(&home.text) {
        Some(token) => token,
        None => return Err("не найден sc-токен".to_string()),
    };

    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    let post_headers = vec![
        ("Content-Type".into(), "application/x-www-form-urlencoded".into()),
        ("Origin".into(), "https://www.startpage.com".into()),
        ("Referer".into(), "https://www.startpage.com/".into()),
    ];

    let body = http::form_urlencoded(&[
        ("query", query),
        ("cat", "web"),
        ("t", "device"),
        ("sc", &sc),
        ("abp", "1"),
        ("abd", "1"),
        ("abe", "1"),
    ]);
    let opts = http::RequestOpts {
        method: Some("POST".into()),
        body: Some(body),
        headers: post_headers.clone(),
        timeout_ms: Some(6000),
        ..Default::default()
    };
    let mut res = http::request("https://www.startpage.com/sp/search", &opts)
        .await
        .map_err(|e| e.message)?;
    if res.status != 200 {
        return Err(http_status_error(res.status));
    }

    // Интерстициальная страница: первый запрос возвращает скрипт с payload
    // (var data = {...}), который надо отправить ещё раз.
    let data_re = Regex::new(r"(?s)var data = (\{[\s\S]*?\});").expect("константная регулярка");
    if let Some(payload) = data_re.captures(&res.text).map(|c| c[1].to_string()) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&payload) {
            if let Some(sgt) = json.get("sgt").and_then(|v| v.as_str()) {
                tokio::time::sleep(std::time::Duration::from_millis(700)).await;
                let body = http::form_urlencoded(&[
                    ("query", query),
                    ("sgt", sgt),
                    ("cat", "web"),
                    ("t", "device"),
                    ("sc", &sc),
                    ("abp", "1"),
                    ("abd", "1"),
                    ("abe", "1"),
                ]);
                let opts = http::RequestOpts {
                    method: Some("POST".into()),
                    body: Some(body),
                    headers: post_headers.clone(),
                    timeout_ms: Some(6000),
                    ..Default::default()
                };
                res = http::request("https://www.startpage.com/sp/search", &opts)
                    .await
                    .map_err(|e| e.message)?;
                if res.status != 200 {
                    return Err(http_status_error(res.status));
                }
            }
        }
    }
    let captcha = Regex::new(r"(?i)/sp/captcha|verify you are human|unusual traffic")
        .expect("константная регулярка");
    if captcha.is_match(&res.text) {
        return Err("капча".to_string());
    }

    let link_re1 = Regex::new(r#"(?i)<a[^>]+class="[^"]*result-link[^"]*"[^>]+href="([^"]+)"[^>]*>([\s\S]*?)</a>"#)
        .expect("константная регулярка");
    let link_re2 = Regex::new(r#"(?i)<a[^>]+href="([^"]+)"[^>]+class="[^"]*result-link[^"]*"[^>]*>([\s\S]*?)</a>"#)
        .expect("константная регулярка");
    let h2_re = Regex::new(r"(?i)<h2[^>]*>([\s\S]*?)</h2>").expect("константная регулярка");
    let desc_re = Regex::new(r#"(?i)class="[^"]*description[^"]*"[^>]*>([\s\S]*?)<"#)
        .expect("константная регулярка");

    let mut out = Vec::new();
    for block in http::parse_blocks_by_class(&res.text, "result") {
        let caps = link_re1.captures(&block).or_else(|| link_re2.captures(&block));
        let Some(caps) = caps else { continue };
        let Some(url) = http::normalize_http_url(&caps[1], Some("https://www.startpage.com/")) else {
            continue;
        };
        let inner = caps[2].to_string();
        // title — из <h2> внутри ссылки, иначе весь текст ссылки.
        let raw_title = h2_re
            .captures(&inner)
            .map(|c| c[1].to_string())
            .unwrap_or(inner);
        let title = http::strip_tags(&raw_title);
        if title.is_empty() {
            continue;
        }
        let snippet = desc_re
            .captures(&block)
            .map(|c| clip(&http::strip_tags(&c[1]), 400))
            .unwrap_or_default();
        // source = hostname URL (как в TS: new URL(url).hostname).
        let source = url::Url::parse(&url)
            .map(|u| u.host_str().unwrap_or_default().to_string())
            .unwrap_or_default();
        out.push(SearchResult {
            title: clip(&title, 200),
            url,
            snippet,
            source,
        });
    }
    Ok(out)
}

fn token(html_text: &str) -> Option<String> {
    let re1 = Regex::new(r#"(?i)name="sc"\s+value="([^"]+)""#).expect("константная регулярка");
    let re2 = Regex::new(r#"(?i)value="([^"]+)"[^>]*name="sc""#).expect("константная регулярка");
    re1.captures(html_text)
        .or_else(|| re2.captures(html_text))
        .map(|c| c[1].to_string())
}

/// Общий стриппер как в brave.rs (определение утилиты там, реэкспортируем).
use super::brave::clip_chars as clip;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sc_token_parses_both_attribute_orders() {
        assert_eq!(
            token(r#"<input type="hidden" name="sc" value="abc123">"#),
            Some("abc123".to_string())
        );
        assert_eq!(
            token(r#"<input value="def456" name="sc" type="hidden">"#),
            Some("def456".to_string())
        );
        assert_eq!(token(r#"<input name="q" value="x">"#), None);
    }

    const FIXTURE: &str = r#"
<div class="result">
<a class="result-link" href="https://found.test/page"><h2>Name of the page</h2></a>
<p class="description">Description text here</p>
</div>
<div class="result">
<a href="https://second.test/other" class="result-link"><h2>Second title</h2></a>
</div>
"#;

    fn parse_fixture(html: &str) -> Vec<SearchResult> {
        let link_re1 = Regex::new(r#"(?i)<a[^>]+class="[^"]*result-link[^"]*"[^>]+href="([^"]+)"[^>]*>([\s\S]*?)</a>"#).unwrap();
        let link_re2 = Regex::new(r#"(?i)<a[^>]+href="([^"]+)"[^>]+class="[^"]*result-link[^"]*"[^>]*>([\s\S]*?)</a>"#).unwrap();
        let h2_re = Regex::new(r"(?i)<h2[^>]*>([\s\S]*?)</h2>").unwrap();
        let desc_re = Regex::new(r#"(?i)class="[^"]*description[^"]*"[^>]*>([\s\S]*?)<"#).unwrap();
        let mut out = Vec::new();
        for block in http::parse_blocks_by_class(html, "result") {
            let caps = link_re1.captures(&block).or_else(|| link_re2.captures(&block));
            let Some(caps) = caps else { continue };
            let Some(url) = http::normalize_http_url(&caps[1], Some("https://www.startpage.com/")) else { continue };
            let inner = caps[2].to_string();
            let raw_title = h2_re.captures(&inner).map(|c| c[1].to_string()).unwrap_or(inner);
            let title = http::strip_tags(&raw_title);
            if title.is_empty() { continue; }
            out.push(SearchResult {
                title: clip(&title, 200),
                url: url.clone(),
                snippet: desc_re.captures(&block).map(|c| clip(&http::strip_tags(&c[1]), 400)).unwrap_or_default(),
                source: url::Url::parse(&url).map(|u| u.host_str().unwrap_or_default().to_string()).unwrap_or_default(),
            });
        }
        out
    }

    #[test]
    fn fixture_parses_result_link_both_orders_h2_title_domain_source() {
        let out = parse_fixture(FIXTURE);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].title, "Name of the page");
        assert_eq!(out[0].url, "https://found.test/page");
        assert_eq!(out[0].snippet, "Description text here");
        assert_eq!(out[0].source, "found.test");
        // Второй порядок атрибутов (href до class) тоже распарсился.
        assert_eq!(out[1].title, "Second title");
        assert_eq!(out[1].snippet, "");
        assert_eq!(out[1].source, "second.test");
    }
}
