//! Sogou (через расширение ссылок sogou.com) — порт `searchSogou` из web_search.ts.

use regex::Regex;

use super::super::http;
use super::{http_status_error, SearchResult, UA_136};

/// Порт `sogouExpandUrl`: у ссылки может лежать целевой URL в url/u/link.
fn sogou_expand_url(href: &str) -> String {
    let Ok(u) = url::Url::parse(href).or_else(|_| {
        url::Url::parse("https://www.sogou.com/").and_then(|base| base.join(href))
    }) else {
        return String::new();
    };
    for key in ["url", "u", "link"] {
        let val = u
            .query_pairs()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.into_owned());
        if let Some(val) = val {
            if val.is_empty() {
                continue; // в TS пустое значение — «не найдено», идём к следующему key
            }
            // searchParams.get уже декодировал один раз; TS разъюэзит ещё раз.
            // decodeURIComponent бросает на невалидном % — TS это ловит общим
            // try/catch и возвращает '' (не исходное значение).
            let decoded = percent_encoding::percent_decode_str(&val)
                .decode_utf8()
                .map(|d| d.into_owned());
            match decoded {
                Ok(d) => {
                    return http::normalize_http_url(&d, Some("https://www.sogou.com/"))
                        .unwrap_or_default()
                }
                Err(_) => return String::new(),
            }
        }
    }
    http::normalize_http_url(href, Some("https://www.sogou.com/")).unwrap_or_default()
}

pub(super) async fn search(query: &str, _limit: usize) -> Result<Vec<SearchResult>, String> {
    let url = format!(
        "https://www.sogou.com/web?query={}&page=1&ie=utf8",
        http::js_encode_uri_component(query)
    );
    let opts = http::RequestOpts {
        headers: vec![
            ("User-Agent".into(), UA_136.into()),
            ("Referer".into(), "https://www.sogou.com/".into()),
        ],
        timeout_ms: Some(5000),
        ..Default::default()
    };
    let res = http::request(&url, &opts).await.map_err(|e| e.message)?;
    if res.status != 200 {
        return Err(http_status_error(res.status));
    }
    let block_re = Regex::new(r"(?i)antispider|请输入验证码|访问过于频繁|搜狗搜索验证")
        .expect("константная регулярка");
    if block_re.is_match(&res.text) {
        return Err("заблокирован (челлендж)".to_string());
    }

    let a_re = Regex::new(r#"(?i)<a[^>]+href="([^"]+)"[^>]*>([\s\S]*?)</a>"#).expect("константная регулярка");
    let desc_re = Regex::new(r#"(?i)class="[^"]*(?:str_info|ft|text-layout|fz-mid)[^"]*"[^>]*>([\s\S]*?)<"#)
        .expect("константная регулярка");
    let desc_p_re = Regex::new(r"(?i)<p[^>]*>([\s\S]*?)</p>").expect("константная регулярка");
    let cite_re = Regex::new(r#"(?i)class="[^"]*(?:citeurl|g|url)[^"]*"[^>]*>([\s\S]*?)<"#)
        .expect("константная регулярка");

    let mut out = Vec::new();
    // Два контейнера результатов: vrwrap и rb (как в TS, порядок сохранён).
    for cls in ["vrwrap", "rb"] {
        for block in http::parse_blocks_by_class(&res.text, cls) {
            let Some(a) = a_re.captures(&block) else { continue };
            let url = sogou_expand_url(&a[1]);
            let title = http::strip_tags(&a[2]);
            if url.is_empty() || title.is_empty() {
                continue;
            }
            let snippet = desc_re
                .captures(&block)
                .map(|c| http::strip_tags(&c[1]))
                .or_else(|| desc_p_re.captures(&block).map(|c| http::strip_tags(&c[1])))
                .map(|s| crate::tools::websearch::engines::brave::clip_chars(&s, 400))
                .unwrap_or_default();
            let source = cite_re
                .captures(&block)
                .map(|c| crate::tools::websearch::engines::brave::clip_chars(&http::strip_tags(&c[1]), 80))
                .unwrap_or_default();
            out.push(SearchResult {
                title: crate::tools::websearch::engines::brave::clip_chars(&title, 200),
                url,
                snippet,
                source,
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_url_pulls_target_from_url_u_link() {
        assert_eq!(
            sogou_expand_url("/link?url=https%3A%2F%2Ftarget.test%2Fa"),
            "https://target.test/a"
        );
        // Непригодные ссылки — нормализуются от базового sogou-хоста.
        assert_eq!(
            sogou_expand_url("/other?x=1"),
            "https://www.sogou.com/other?x=1"
        );
        assert_eq!(sogou_expand_url(""), "");
    }

    const FIXTURE: &str = r#"
<div class="vrwrap"><h3><a href="/link?url=https%3A%2F%2Fs.test%2Fone">Титул первый</a></h3>
<div class="str_info">Описание номер один</div><div class="citeurl">s.test</div></div>
<div class="rb"><h3><a href="https://direct.test/two">Direct link</a></h3>
<p>Пешого есаки.</p></div>
"#;

    fn parse_fixture(html: &str) -> Vec<SearchResult> {
        let a_re = Regex::new(r#"(?i)<a[^>]+href="([^"]+)"[^>]*>([\s\S]*?)</a>"#).unwrap();
        let desc_re = Regex::new(r#"(?i)class="[^"]*(?:str_info|ft|text-layout|fz-mid)[^"]*"[^>]*>([\s\S]*?)<"#).unwrap();
        let desc_p_re = Regex::new(r"(?i)<p[^>]*>([\s\S]*?)</p>").unwrap();
        let cite_re = Regex::new(r#"(?i)class="[^"]*(?:citeurl|g|url)[^"]*"[^>]*>([\s\S]*?)<"#).unwrap();
        let mut out = Vec::new();
        for cls in ["vrwrap", "rb"] {
            for block in http::parse_blocks_by_class(html, cls) {
                let Some(a) = a_re.captures(&block) else { continue };
                let url = sogou_expand_url(&a[1]);
                let title = http::strip_tags(&a[2]);
                if url.is_empty() || title.is_empty() { continue; }
                let snippet = desc_re.captures(&block).map(|c| http::strip_tags(&c[1]))
                    .or_else(|| desc_p_re.captures(&block).map(|c| http::strip_tags(&c[1])))
                    .map(|s| crate::tools::websearch::engines::brave::clip_chars(&s, 400)).unwrap_or_default();
                out.push(SearchResult {
                    title: crate::tools::websearch::engines::brave::clip_chars(&title, 200),
                    url,
                    snippet,
                    source: cite_re.captures(&block).map(|c| crate::tools::websearch::engines::brave::clip_chars(&http::strip_tags(&c[1]), 80)).unwrap_or_default(),
                });
            }
        }
        out
    }

    #[test]
    fn fixture_parses_vrwrap_and_rb_containers() {
        let out = parse_fixture(FIXTURE);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].url, "https://s.test/one");
        assert_eq!(out[0].title, "Титул первый");
        assert_eq!(out[0].snippet, "Описание номер один");
        assert_eq!(out[0].source, "s.test");
        // Второй блок: .rb без str_info — фолбэк на <p>; прямой URL без расширения.
        assert_eq!(out[1].url, "https://direct.test/two");
        assert_eq!(out[1].snippet, "Пешого есаки.");
        assert_eq!(out[1].source, "");
    }
}
