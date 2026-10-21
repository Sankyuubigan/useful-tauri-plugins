//! Baidu — порт `searchBaidu` из web_search.ts: h3-цикл по выдаче.

use regex::Regex;

use super::super::http;
use super::clip_chars;
use super::{http_status_error, SearchResult, UA_91};

pub(super) async fn search(query: &str, _limit: usize) -> Result<Vec<SearchResult>, String> {
    let url = format!(
        "https://www.baidu.com/s?wd={}&pn=0&ie=utf-8&tn=baidu",
        http::js_encode_uri_component(query)
    );
    let opts = http::RequestOpts {
        headers: vec![
            ("User-Agent".into(), UA_91.into()),
            ("Accept-Language".into(), "zh-CN,zh;q=0.9".into()),
        ],
        timeout_ms: Some(5000),
        ..Default::default()
    };
    let res = http::request(&url, &opts).await.map_err(|e| e.message)?;
    if res.status != 200 {
        return Err(http_status_error(res.status));
    }
    let blocked = Regex::new(r"(?i)wappass|百度安全验证|访问过于频繁|请输入验证码")
        .expect("константная регулярка");
    if blocked.is_match(&res.text) {
        return Err("заблокирован (антибот)".to_string());
    }

    let h3_re = Regex::new(r"(?is)<h3[^>]*>([\s\S]*?)</h3>").expect("константная регулярка");
    let a_re = Regex::new(r#"(?i)<a[^>]+href="([^"]+)"[^>]*>([\s\S]*?)</a>"#).expect("константная регулярка");
    let desc_re = Regex::new(r#"(?i)class="[^"]*(?:c-span-last|c-color-text|cos-row)[^"]*"[^>]*>([\s\S]*?)<"#)
        .expect("константная регулярка");

    let mut out = Vec::new();
    for m in h3_re.captures_iter(&res.text) {
        let Some(a) = a_re.captures(&m[1]) else { continue };
        let Some(url) = http::normalize_http_url(&a[1], Some("https://www.baidu.com/")) else { continue };
        let title = http::strip_tags(&a[2]);
        if url.is_empty() || title.is_empty() {
            continue;
        }
        // В TS срез идёт от НАЧАЛА h3-моатча (m.index), а не после него.
        let after = &res.text[m.get(0).expect("captures 0").start()..];
        let snippet = desc_re
            .captures(after)
            .map(|c| clip_chars(&http::strip_tags(&c[1]), 400))
            .unwrap_or_default();
        out.push(SearchResult {
            title: clip_chars(&title, 200),
            url,
            snippet,
            source: "baidu.com".into(),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = concat!(
        r#"<h3 class="c-title"><a href="https://first.test/a" class="t">Первый заголовок</a></h3>"#,
        r#"<div class="c-span-last">Снippet первого</div>"#,
        r#"<h3><a href="http://second.test/b">Second</a></h3>"#,
        r#"<span class="c-color-text">Second desc</span>"#
    );

    fn parse_fixture(html: &str) -> Vec<SearchResult> {
        let h3_re = Regex::new(r"(?is)<h3[^>]*>([\s\S]*?)</h3>").unwrap();
        let a_re = Regex::new(r#"(?i)<a[^>]+href="([^"]+)"[^>]*>([\s\S]*?)</a>"#).unwrap();
        let desc_re = Regex::new(r#"(?i)class="[^"]*(?:c-span-last|c-color-text|cos-row)[^"]*"[^>]*>([\s\S]*?)<"#).unwrap();
        let mut out = Vec::new();
        for m in h3_re.captures_iter(html) {
            let start = m.get(0).expect("captures 0").start();
            let Some(a) = a_re.captures(&m[1]) else { continue };
            let Some(url) = http::normalize_http_url(&a[1], Some("https://www.baidu.com/")) else { continue };
            let title = http::strip_tags(&a[2]);
            if url.is_empty() || title.is_empty() { continue; }
            let after = &html[start..];
            out.push(SearchResult {
                title: clip_chars(&title, 200),
                url,
                snippet: desc_re.captures(after).map(|c| clip_chars(&http::strip_tags(&c[1]), 400)).unwrap_or_default(),
                source: "baidu.com".into(),
            });
        }
        out
    }

    #[test]
    fn blocked_keywords_fail_honestly() {
        let blocked = Regex::new(r"wappass|百度安全验证|访问过于频繁|请输入验证码").unwrap();
        assert!(blocked.is_match("_redirect wappass/login"));
    }

    #[test]
    fn fixture_parses_h3_loop_with_snippet_after_h3_start() {
        let out = parse_fixture(FIXTURE);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].title, "Первый заголовок");
        assert_eq!(out[0].url, "https://first.test/a");
        assert_eq!(out[0].snippet, "Снippet первого");
        assert_eq!(out[0].source, "baidu.com");
        assert_eq!(out[1].title, "Second");
        assert_eq!(out[1].url, "http://second.test/b");
        assert_eq!(out[1].snippet, "Second desc");
    }
}
