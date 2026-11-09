//! Wiby — порт `searchWiby` из web_search.ts: индекс старых сайтов wiby.me.

use regex::Regex;

use super::super::http;
use super::clip_chars;
use super::{http_status_error, SearchResult, UA_CHROME_112};

pub(super) async fn search(query: &str, _limit: usize) -> Result<Vec<SearchResult>, String> {
    let opts = http::RequestOpts {
        headers: vec![("User-Agent".into(), UA_CHROME_112.into())],
        timeout_ms: Some(6000),
        ..Default::default()
    };
    let url = format!("https://wiby.me/?q={}", http::js_encode_uri_component(query));
    let res = http::request(&url, &opts).await.map_err(|e| e.message)?;
    if res.status != 200 {
        return Err(http_status_error(res.status));
    }

    let a_re = Regex::new(r#"(?i)<a[^>]+href="([^"]+)"[^>]*>\s*([\s\S]*?)\s*</a>"#)
        .expect("константная регулярка");
    let next_re = Regex::new(r#"(?i)<p class="url">[\s\S]*?</p>\s*<p>([\s\S]*?)</p>"#)
        .expect("константная регулярка");

    let mut out = Vec::new();
    for block in http::parse_blocks_by_class(&res.text, "tlink") {
        let Some(a) = a_re.captures(&block) else { continue };
        let url = http::normalize_http_url(&a[1], Some("https://wiby.me/")).unwrap_or_default();
        let title = http::strip_tags(&a[2]);
        if url.is_empty() || title.is_empty() {
            continue;
        }
        // Сниппет ищется ПОСЛЕ блока в исходном тексте (как в TS: indexOf + length).
        let after = match res.text.find(&block) {
            Some(idx) => &res.text[idx + block.len()..],
            None => continue,
        };
        let snippet = next_re
            .captures(after)
            .map(|c| clip_chars(&http::strip_tags(&c[1]), 400))
            .unwrap_or_default();
        out.push(SearchResult {
            title: clip_chars(&title, 200),
            url,
            snippet,
            source: String::new(),
        });
    }
    // Wiby индексирует в основном англоязычные сайты: пустая выдача — не ошибка.
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = concat!(
r#"<blockquote><a class="tlink" href="/pages/1">Старый сайт</a><br><p class="url">oldsite.t</p><p>Снippet старого сайта</p></blockquote>"#,
r#"<blockquote><a class="tlink" href="https://plain.t/page">Прямая ссылка</a><p>Блок без p.url — сниппет не берётся</p></blockquote>"#
    );

    fn parse_fixture(html: &str) -> Vec<SearchResult> {
        let a_re = Regex::new(r#"(?i)<a[^>]+href="([^"]+)"[^>]*>\s*([\s\S]*?)\s*</a>"#).unwrap();
        let next_re = Regex::new(r#"(?i)<p class="url">[\s\S]*?</p>\s*<p>([\s\S]*?)</p>"#).unwrap();
        let mut out = Vec::new();
        for block in http::parse_blocks_by_class(html, "tlink") {
            let Some(a) = a_re.captures(&block) else { continue };
            let url = http::normalize_http_url(&a[1], Some("https://wiby.me/")).unwrap_or_default();
            let title = http::strip_tags(&a[2]);
            if url.is_empty() || title.is_empty() { continue; }
            let after = match html.find(&block) {
                Some(idx) => &html[idx + block.len()..],
                None => continue,
            };
            let snippet = next_re.captures(after).map(|c| clip_chars(&http::strip_tags(&c[1]), 400)).unwrap_or_default();
            out.push(SearchResult {
                title: clip_chars(&title, 200),
                url,
                snippet,
                source: String::new(),
            });
        }
        out
    }

    #[test]
    fn fixture_resolves_relative_and_reads_snippet_after_block() {
        let out = parse_fixture(FIXTURE);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].title, "Старый сайт");
        assert_eq!(out[0].url, "https://wiby.me/pages/1");
        assert_eq!(out[0].snippet, "Снippet старого сайта");
        // Второй блок: только <p> без p.class="url" — pattern requires exactly
        // p class="url" затем p, сниппета нет.
        assert_eq!(out[1].url, "https://plain.t/page");
        assert_eq!(out[1].snippet, "");
    }
}
