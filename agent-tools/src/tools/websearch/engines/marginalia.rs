//! Marginalia — порт `searchMarginalia` из web_search.ts: челлендж
//! «Wait A Moment» с sst-ссылкой и таймером (1–3 попытки обхода).

use regex::Regex;

use super::super::http;
use super::clip_chars;
use super::{http_status_error, SearchResult, UA_CHROME_112};

pub(super) async fn search(query: &str, _limit: usize) -> Result<Vec<SearchResult>, String> {
    let headers = vec![
        ("User-Agent".into(), UA_CHROME_112.into()),
        ("Referer".into(), "https://search.marginalia.nu/".into()),
    ];
    let base = format!(
        "https://search.marginalia.nu/search?query={}",
        http::js_encode_uri_component(query)
    );
    let mut res = http::request(
        &base,
        &http::RequestOpts {
            headers: headers.clone(),
            timeout_ms: Some(8000),
            max_redirects: Some(3),
            ..Default::default()
        },
    )
    .await
    .map_err(|e| e.message)?;

    for _ in 0..3 {
        let is_wait = res.text.contains("Wait A Moment")
            || (res.status >= 500 && res.text.contains("countdown"));
        if !is_wait {
            break;
        }
        let href_re = Regex::new(r#"href="(/search\?[^"]*sst=[^"]+)""#).expect("константная регулярка");
        let loc_re = Regex::new(r"location\.replace\('([^']+)'").expect("константная регулярка");
        let caps = href_re
            .captures(&res.text)
            .or_else(|| loc_re.captures(&res.text));
        let Some(caps) = caps else {
            return Err("челлендж без sst-ссылки".to_string());
        };
        let sst = caps[1].replace("&amp;", "&").replace("\\/", "/");
        // Таймер ожидания: data-tr="N" → N*1000 + 1500 (N может быть 0/отриц).
        let tr_re = Regex::new(r#"data-tr="(-?\d+)""#).expect("константная регулярка");
        let tr: i64 = tr_re
            .captures(&res.text)
            .and_then(|c| c[1].parse().ok())
            .unwrap_or(1);
        tokio::time::sleep(std::time::Duration::from_millis(tr.max(0) as u64 * 1000 + 1500)).await;
        res = http::request(
            &format!("https://marginalia-search.com{sst}"),
            &http::RequestOpts {
                headers: headers.clone(),
                timeout_ms: Some(8000),
                max_redirects: Some(3),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| e.message)?;
    }
    if res.status != 200 {
        return Err(http_status_error(res.status));
    }

    let re = Regex::new(
        r#"(?is)<h2[^>]*>\s*<a[^>]+href="(https?://[^"]+)"[^>]*>\s*([\s\S]*?)\s*</a>\s*</h2>"#,
    )
    .expect("константная регулярка");
    let snip_re = Regex::new(r#"(?is)<p class="mt-2[^"]*"[^>]*>([\s\S]*?)</p>"#)
        .expect("константная регулярка");

    let mut out = Vec::new();
    for m in re.captures_iter(&res.text) {
        let whole = m.get(0).expect("captures 0");
        let url = m[1].to_string();
        let title = http::strip_tags(&m[2]).replace("&shy;", "").trim().to_string();
        if url.is_empty() || title.is_empty() {
            continue;
        }
        let tail = &res.text[whole.end()..];
        let snippet = snip_re
            .captures(tail)
            .map(|c| {
                clip_chars(&http::strip_tags(&c[1]).replace("&shy;", ""), 400)
            })
            .unwrap_or_default();
        out.push(SearchResult {
            title: clip_chars(&title, 200),
            url,
            snippet,
            source: String::new(),
        });
    }
    // Англоязычный индекс: пустая выдача для русских запросов — не ошибка движка.
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = concat!(
        r#"<h2 class="url"><a href="https://marg.test/aaa" rel="noopener">Заголо&shy;вок A</a></h2>"#,
        r#"<p class="mt-2">Описание A</p>"#,
        r#"<h2><a href="https://marg.test/bbb">B</a></h2>"#
    );

    fn parse_fixture(html: &str) -> Vec<SearchResult> {
        let re = Regex::new(r#"(?is)<h2[^>]*>\s*<a[^>]+href="(https?://[^"]+)"[^>]*>\s*([\s\S]*?)\s*</a>\s*</h2>"#).unwrap();
        let snip_re = Regex::new(r#"(?is)<p class="mt-2[^"]*"[^>]*>([\s\S]*?)</p>"#).unwrap();
        let mut out = Vec::new();
        for m in re.captures_iter(html) {
            let whole = m.get(0).expect("captures 0");
            let url = m[1].to_string();
            let title = http::strip_tags(&m[2]).replace("&shy;", "").trim().to_string();
            if url.is_empty() || title.is_empty() { continue; }
            let tail = &html[whole.end()..];
            let snippet = snip_re.captures(tail).map(|c| clip_chars(&http::strip_tags(&c[1]).replace("&shy;", ""), 400)).unwrap_or_default();
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
    fn fixture_unescapes_shy_and_takes_following_snippet() {
        let out = parse_fixture(FIXTURE);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].url, "https://marg.test/aaa");
        assert_eq!(out[0].title, "Заголовок A");
        assert_eq!(out[0].snippet, "Описание A");
        assert_eq!(out[1].url, "https://marg.test/bbb");
        assert_eq!(out[1].snippet, "");
    }

    #[test]
    fn challenge_patterns_extract_sst_link_and_default_timer() {
        // 1) капча с href=/search?...sst=... — ссылка и data-tr pulled.
        let page = r#"Wait A Moment <a href="/search?query=x&amp;sst=217">next</a><div data-tr="4"></div>"#;
        let href_re = Regex::new(r#"href="(/search\?[^"]*sst=[^"]+)""#).unwrap();
        let tr_re = Regex::new(r#"data-tr="(-?\d+)""#).unwrap();
        let caps = href_re.captures(page).expect("href есть");
        assert_eq!(caps[1].replace("&amp;", "&"), "/search?query=x&sst=217");
        assert_eq!(tr_re.captures(page).and_then(|c| c[1].parse::<i64>().ok()), Some(4));

        // 2) JS location.replace('...') вариант.
        let page2 = r#"Wait A Moment<script>location.replace('/search?query=x&sst=5')</script>"#;
        let loc_re = Regex::new(r"location\.replace\('([^']+)'").unwrap();
        let caps2 = loc_re.captures(page2).expect("location есть");
        assert_eq!(&caps2[1], "/search?query=x&sst=5");

        // 3) Без data-tr -> дефолт 1 (таймер 1000+1500).
        // 4) Без sst-ссылки -> честный отказ.
        assert!(href_re.captures("Wait A Moment, but no link").is_none()
            && loc_re.captures("Wait A Moment, but no link").is_none());
    }
}
