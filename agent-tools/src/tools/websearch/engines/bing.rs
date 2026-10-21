//! Bing — порт `searchBing` из web_search.ts: www.bing.com с фолбэком на
//! cn.bing.com, декодирование редиректов из base64url-параметра `u`.

use base64::Engine as _;
use regex::Regex;

use super::super::http;
use super::clip_chars;
use super::{http_status_error, SearchResult};

/// Порт `isBingBlocked`: блокировка по ≥2 капча-словам ИЛИ одно слово
/// «captcha» при отсутствии реальной выдачи (li class="b_algo").
fn is_bing_blocked(text: &str) -> bool {
    let keywords = [
        "captcha",
        "verify you are human",
        "access denied",
        "blocked",
        "too many requests",
        "请验证",
        "验证码",
    ];
    let lower = text.to_lowercase();
    let hits = keywords.iter().filter(|k| lower.contains(*k)).count();
    hits >= 2 || (lower.contains("captcha") && !text.contains("<li class=\"b_algo\""))
}

/// Порт `bingRealUrl`: внутри bing.com цель лежит в base64url-параметре `u`.
fn bing_real_url(href: &str) -> String {
    let Ok(u) = url::Url::parse(href)
        .or_else(|_| url::Url::parse("https://www.bing.com/").and_then(|b| b.join(href)))
    else {
        return String::new();
    };
    if u.host_str().map(|h| h.contains("bing.com")).unwrap_or(false) {
        let Some(b64) = u
            .query_pairs()
            .find(|(k, _)| k == "u")
            .map(|(_, v)| v.into_owned())
        else {
            // Как в TS: до bing-редиректа без параметра u добраться нечем.
            return String::new();
        };
        // Buffer.from(b64, 'base64url') прощает паддинг и мусор; в Rust
        // уравниваем: выкидываем неалфавитные символы и паддинг перед декодом.
        let alphabet: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let cleaned: String = b64
            .chars()
            .filter(|c| alphabet.contains(&(*c as u8)))
            .collect();
        let cleaned = cleaned.trim_end_matches('=');
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(cleaned)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .unwrap_or_default();
        return http::normalize_http_url(&decoded, Some("https://www.bing.com/")).unwrap_or_default();
    }
    http::normalize_http_url(href, Some("https://www.bing.com/")).unwrap_or_default()
}

pub(super) async fn search(query: &str, _limit: usize) -> Result<Vec<SearchResult>, String> {
    let headers = vec![
        ("Cache-Control".into(), "no-cache".into()),
        ("Pragma".into(), "no-cache".into()),
    ];
    // Первый запрос: сетевая ошибка ТЕРПИТСЯ (в TS try/catch → res = null).
    let first = http::request(
        &format!(
            "https://www.bing.com/search?q={}&count=10&setlang=ru-ru",
            http::js_encode_uri_component(query)
        ),
        &http::RequestOpts { headers: headers.clone(), timeout_ms: Some(5000), ..Default::default() },
    )
    .await
    .ok();
    let res = match first {
        Some(r) if r.status == 200 => r,
        _ => {
            http::request(
                &format!(
                    "https://cn.bing.com/search?q={}&count=10&setlang=ru-ru&ensearch=0",
                    http::js_encode_uri_component(query)
                ),
                &http::RequestOpts { headers, timeout_ms: Some(5000), ..Default::default() },
            )
            .await
            .map_err(|e| e.message)?
        }
    };
    if res.status != 200 {
        return Err(http_status_error(res.status));
    }
    if is_bing_blocked(&res.text) {
        return Err("заблокирован (капча) — попробуйте позже".to_string());
    }

    let h2_anchor = Regex::new(r#"(?is)<h2[^>]*>[\s\S]*?<a[^>]+href="([^"]+)"[^>]*>([\s\S]*?)</a>"#)
        .expect("константная регулярка");
    let desc_re = Regex::new(r"(?is)<p[^>]*>([\s\S]*?)</p>").expect("константная регулярка");
    let cite_re = Regex::new(r"(?is)<cite[^>]*>([\s\S]*?)</cite>").expect("константная регулярка");

    let mut out = Vec::new();
    for block in http::parse_blocks_by_class(&res.text, "b_algo") {
        let Some(a) = h2_anchor.captures(&block) else { continue };
        let url = bing_real_url(&a[1]);
        let title = http::strip_tags(&a[2]);
        if url.is_empty() || title.is_empty() {
            continue;
        }
        let snippet = desc_re
            .captures(&block)
            .map(|c| clip_chars(&http::strip_tags(&c[1]), 400))
            .unwrap_or_default();
        let source = cite_re
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocked_flags_two_keyword_hits_and_captcha_without_b_algo() {
        assert!(is_bing_blocked("Access denied. Please verify you are human"));
        assert!(is_bing_blocked("<html>CAPTCHA page"));
        // С одним словом, но без выдачи — тоже блок (captcha без b_algo).
        // Реальная выдача со словом в контенте и b_algo — не блок.
        let real = "<li class=\"b_algo\">Обычные результаты слова «blocked» в тексте</li>";
        assert!(!is_bing_blocked(real));
        // Только одно капча-слово + есть выдача → не блок.
        assert!(!is_bing_blocked("<li class=\"b_algo\">captcha somewhere in text</li>"));
    }

    #[test]
    fn real_url_decodes_base64url_param_and_ignores_plain_bing_refs() {
        // u= закодированный base64url от "https://target.test/page"
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(b"https://target.test/page");
        assert_eq!(bing_real_url(&format!("https://www.bing.com/ck/a?!&u=a1{b64}")), "");
        // Всё же: настоящий Bing префиксует u значением «на всё подряд» — TS
        // декодирует «как есть», поэтому префикс «a1a2» расшифровался бы в
        // мусор и normalizeHttpUrl вернул ''. Проверяем честное поведение:
        assert_eq!(
            bing_real_url(&format!("https://www.bing.com/ck/a?u=a1{b64}")),
            ""
        );
        // Идеальный кейс без префикса — раскодируется в реальный URL.
        assert_eq!(
            bing_real_url(&format!("https://www.bing.com/ck/a?u={b64}")),
            "https://target.test/page"
        );
        // Внешняя ссылка без u — нормализуется как есть.
        assert_eq!(
            bing_real_url("https://example.com/docs"),
            "https://example.com/docs"
        );
        // bing-ссылка без параметра u -> '' (блок пропускается).
        assert_eq!(bing_real_url("https://www.bing.com/other?x=1"), "");
    }

    const FIXTURE: &str = concat!(
        r#"<li class="b_algo"><h2><a href="https://example.com/one">Первый заголовок</a></h2>"#,
        r#"<p>Описание один</p><cite>example.com</cite></li>"#,
        r#"<li class="b_algo"><h2><a href="https://www.bing.com/ck/a?u=verybadbase64">b64 сломан</a></h2>"#,
        r#"<p>not parsed</p></li>"#
    );

    fn parse_fixture(html: &str) -> Vec<SearchResult> {
        let h2_anchor = Regex::new(r#"(?is)<h2[^>]*>[\s\S]*?<a[^>]+href="([^"]+)"[^>]*>([\s\S]*?)</a>"#).unwrap();
        let desc_re = Regex::new(r"(?is)<p[^>]*>([\s\S]*?)</p>").unwrap();
        let cite_re = Regex::new(r"(?is)<cite[^>]*>([\s\S]*?)</cite>").unwrap();
        let mut out = Vec::new();
        for block in http::parse_blocks_by_class(html, "b_algo") {
            let Some(a) = h2_anchor.captures(&block) else { continue };
            let url = bing_real_url(&a[1]);
            let title = http::strip_tags(&a[2]);
            if url.is_empty() || title.is_empty() { continue; }
            out.push(SearchResult {
                title: clip_chars(&title, 200),
                url,
                snippet: desc_re.captures(&block).map(|c| clip_chars(&http::strip_tags(&c[1]), 400)).unwrap_or_default(),
                source: cite_re.captures(&block).map(|c| clip_chars(&http::strip_tags(&c[1]), 80)).unwrap_or_default(),
            });
        }
        out
    }

    #[test]
    fn fixture_skips_decoded_garbage_and_keeps_plain_results() {
        let out = parse_fixture(FIXTURE);
        let urls: Vec<(&str, &str)> = out.iter().map(|r| (r.url.as_str(), r.title.as_str())).collect();
        // Второй блок с битым base64u-редиректом отброшен, внешний url потерял
        // hash-часть нет — оба результата простые.
        assert_eq!(urls, vec![("https://example.com/one", "Первый заголовок")]);
        assert_eq!(out[0].snippet, "Описание один");
        assert_eq!(out[0].source, "example.com");
    }
}
