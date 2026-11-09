//! DuckDuckGo (primary) — порт `searchDuckDuckGo` из web_search.ts.
//!
//! Три ступени: Instant Answer API → html POST (ретрай при anomaly) →
//! lite POST. Антибот-страница "anomaly"/"execDeep" (202/JS-challenge).

use regex::Regex;

use super::super::http;
use super::{http_status_error, SearchResult};

/// Порт `isAnomaly`: 2xx + слова антибот-страницы.
fn is_anomaly(status: u16, body: &str) -> bool {
    (200..300).contains(&status) && (body.contains("anomaly") || body.contains("execDeep"))
}

/// Порт `ddgNormalizeUrl`: разворот внутренних редиректов /l/?uddg=.
pub(super) fn ddg_normalize_url(raw: &str) -> String {
    let Some(url) = http::normalize_http_url(raw, Some("https://duckduckgo.com/")) else {
        return String::new();
    };
    let Ok(u) = url::Url::parse(&url) else {
        return url;
    };
    // Поиск: hostname duckduckgo.com + путь /l/* + uddg у параметров.
    let host = u.host_str().unwrap_or_default();
    if host == "duckduckgo.com" && u.path().starts_with("/l/") {
        // searchParams.get возвращает значение уже декодированным; TS разъюэзит
        // его ВТОРОЙ раз (в uddg лежит double-encoded URL).
        let uddg = u.query_pairs().find(|(k, _)| k == "uddg").map(|(_, v)| v.into_owned());
        if let Some(first_decode) = uddg {
            // TS: decodeURIComponent бросает → catch в try/конструкции → url остаётся
            // как есть; но если повторная normalize дала пусто — TS присваивает
            // url='' и возвращает '' (исходный url теряется).
            if let Ok(second_decode) = percent_encoding::percent_decode_str(&first_decode).decode_utf8() {
                return http::normalize_http_url(&second_decode, Some("https://duckduckgo.com/"))
                    .unwrap_or_default();
            }
        }
    }
    url
}

pub(super) async fn search(query: &str, _limit: usize) -> Result<Vec<SearchResult>, String> {
    let mut all: Vec<SearchResult> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    // 1. Instant Answer API — мгновенный, но для многих запросов пуст.
    let ia_url = format!(
        "https://api.duckduckgo.com/?q={}&format=json&no_html=1&skip_disambig=1",
        http::js_encode_uri_component(query)
    );
    let ia_opts = request_opts_json(4000);
    match http::request(&ia_url, &ia_opts).await {
        Err(e) => errors.push(format!("Instant Answer: {e}")),
        Ok(res) => {
            if res.status == 200 {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&res.text) {
                    // AbstractText — «краткий ответ» целиком.
                    if let Some(text) = json.get("AbstractText").and_then(|v| v.as_str()) {
                        if !text.is_empty() {
                            let title = json.get("Heading").and_then(|v| v.as_str()).unwrap_or("Краткий ответ").to_string();
                            let url = json.get("AbstractURL").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                            all.push(SearchResult { title, url, snippet: text.to_string(), source: String::new() });
                        }
                    }
                    let topics = json.get("RelatedTopics").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                    for topic in topics {
                        if let Some(nested) = topic.get("Topics").and_then(|v| v.as_array()).cloned() {
                            for t in nested {
                                push_topic(&mut all, &t);
                            }
                        } else {
                            push_topic(&mut all, &topic);
                        }
                    }
                }
            }
        }
    }

    // 2. html POST с ретраем при антибот-странице (если мгновённый ответ мал).
    if all.len() < 3 {
        tokio::time::sleep(std::time::Duration::from_millis(700)).await;
        let body = format!("q={}", http::js_encode_uri_component(query));
        for _ in 0..2 {
            let opts = http::RequestOpts {
                method: Some("POST".into()),
                body: Some(body.clone()),
                headers: vec![
                    ("Content-Type".into(), "application/x-www-form-urlencoded".into()),
                    ("Referer".into(), "https://html.duckduckgo.com/".into()),
                ],
                timeout_ms: Some(6000),
                ..Default::default()
            };
            match http::request("https://html.duckduckgo.com/html/", &opts).await {
                Err(e) => errors.push(format!("DDG html: {e}")),
                Ok(res) => {
                    if res.status != 200 {
                        errors.push(format!("DDG html: {}", http_status_error(res.status)));
                    } else if is_anomaly(res.status, &res.text) {
                        errors.push("DDG html: аномальная страница (anomaly)".into());
                    } else {
                        let link_re = Regex::new(
                            r#"(?i)class="[^"]*result__a[^"]*"[^>]*href="([^"]+)"[^>]*>([\s\S]*?)</a>"#,
                        )
                        .expect("константная регулярка");
                        let snip_re = Regex::new(
                            r#"(?i)class="[^"]*result__snippet[^"]*"[^>]*>([\s\S]*?)</a>"#,
                        )
                        .expect("константная регулярка");
                        for block in http::parse_blocks_by_class(&res.text, "result") {
                            let Some(a) = link_re.captures(&block) else { continue };
                            let url = ddg_normalize_url(&a[1]);
                            let title = http::strip_tags(&a[2]);
                            if url.is_empty() || title.is_empty() {
                                continue;
                            }
                            let snippet = snip_re
                                .captures(&block)
                                .map(|s| http::strip_tags(&s[1]))
                                .unwrap_or_default();
                            all.push(SearchResult { title, url, snippet, source: String::new() });
                        }
                        if all.len() >= 3 {
                            break;
                        }
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        }
    }

    // 3. lite POST — последний фолбэк.
    if all.len() < 3 {
        tokio::time::sleep(std::time::Duration::from_millis(700)).await;
        let body = format!("q={}", http::js_encode_uri_component(query));
        let opts = http::RequestOpts {
            method: Some("POST".into()),
            body: Some(body),
            headers: vec![
                ("Content-Type".into(), "application/x-www-form-urlencoded".into()),
                ("Referer".into(), "https://lite.duckduckgo.com/".into()),
            ],
            timeout_ms: Some(6000),
            ..Default::default()
        };
        match http::request("https://lite.duckduckgo.com/lite/", &opts).await {
            Err(e) => errors.push(format!("DDG lite: {e}")),
            Ok(res) => {
                if res.status != 200 {
                    errors.push(format!("DDG lite: {}", http_status_error(res.status)));
                } else if is_anomaly(res.status, &res.text) {
                    errors.push("DDG lite: аномальная страница (anomaly)".into());
                } else {
                    // Два порядка атрибутов ссылки (class до href / href до class).
                    let link_re = Regex::new(concat!(
                        r#"(?i)<a[^>]+class='result-link'[^>]+href="([^"]+)"[^>]*>([\s\S]*?)</a>"#,
                        r#"|<a[^>]+href="([^"]+)"[^>]+class='result-link'[^>]*>([\s\S]*?)</a>"#
                    ))
                    .expect("константная регулярка");
                    // Без (?i): TS-регулярка `<td class='result-snippet'>` — без i-флага,
                    // реальная разметка DDG lite использует точный lowercase.
                    let snip_re = Regex::new(r"<td class='result-snippet'>([\s\S]*?)</td>")
                        .expect("константная регулярка");
                    for caps in link_re.captures_iter(&res.text) {
                        let whole = caps.get(0).expect("captures 0");
                        // В первой альтернативе href=группа1/title=группа2, во второй — 3/4.
                        let (raw_url, raw_title) = match (caps.get(1), caps.get(2)) {
                            (Some(u), Some(t)) => (u.as_str(), t.as_str()),
                            _ => (
                                caps.get(3).map(|m| m.as_str()).unwrap_or_default(),
                                caps.get(4).map(|m| m.as_str()).unwrap_or_default(),
                            ),
                        };
                        let url = ddg_normalize_url(raw_url);
                        let title = http::strip_tags(raw_title);
                        if url.is_empty() || title.is_empty() {
                            continue;
                        }
                        // Сниппет ищется ПОСЛЕ ссылки (res.text.slice(m.index+m[0].length)).
                        let after = &res.text[whole.end()..];
                        let snippet = snip_re
                            .captures(after)
                            .map(|s| http::strip_tags(&s[1]))
                            .unwrap_or_default();
                        all.push(SearchResult { title, url, snippet, source: String::new() });
                    }
                }
            }
        }
    }

    if all.is_empty() {
        let joined = errors.join("; ");
        return Err(if joined.trim().is_empty() {
            "сервис вернул пустой ответ (антибот-защита)".to_string()
        } else {
            joined
        });
    }
    Ok(all)
}

fn push_topic(all: &mut Vec<SearchResult>, t: &serde_json::Value) {
    // Порт pushTopic: FirstURL | Url → ddgNormalizeUrl, Text | Result → text.
    let raw_url = t.get("FirstURL").or_else(|| t.get("Url")).and_then(|v| v.as_str()).unwrap_or_default();
    let raw_text = t.get("Text").or_else(|| t.get("Result")).and_then(|v| v.as_str()).unwrap_or_default();
    let url = ddg_normalize_url(raw_url);
    let text = http::strip_tags(raw_text);
    if url.is_empty() || text.is_empty() {
        return;
    }
    // title: Text (до 120 символов), иначе URL; snippet — до 400.
    let chars = |s: &str, n: usize| s.chars().take(n).collect::<String>();
    let title = if raw_text.is_empty() { chars(&url, 120) } else { chars(raw_text, 120) };
    all.push(SearchResult {
        title,
        url,
        snippet: chars(&text, 400),
        source: String::new(),
    });
}

fn request_opts_json(timeout_ms: u64) -> http::RequestOpts {
    // В TS у Instant Answer свой Accept: application/json (UA — дефолт web_http).
    http::RequestOpts {
        headers: vec![("Accept".into(), "application/json".into())],
        timeout_ms: Some(timeout_ms),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_HTML_RESULTS: &str = r##"
<div class="result results_links results_links_default">
<div class="links_main links__result">
<h2 class="result__title"><a rel="nofollow" class="result__a" href="https://example.org/guide">Rust guide</a></h2>
<a class="result__snippet" href="//example.org/guide">Very useful snippet text</a>
</div></div>
<div class="result">
<h2 class="result__title"><a class="result__a" href="/l/?uddg=https%3A%2F%2Ftarget.test%2Fpage&amp;rut=abc">Wrapped</a></h2>
  <a class="result__snippet" href="#">wrapped snippet</a>
       </div>
       "##;

const FIXTURE_LITE: &str = r##"
<tr><td><a class='result-link' href="https://one.test/a">First</a></td></tr>
<tr><td class='result-snippet'>first snippet</td></tr>
<tr><td><a href="https://two.test/b" class='result-link'>Second</a></td></tr>
<tr><td class='result-snippet'>second snippet</td></tr>
"##;

    fn parse_html_fixture(html: &str) -> Vec<SearchResult> {
        let link_re = Regex::new(r#"(?i)class="[^"]*result__a[^"]*"[^>]*href="([^"]+)"[^>]*>([\s\S]*?)</a>"#).unwrap();
        let snip_re = Regex::new(r#"(?i)class="[^"]*result__snippet[^"]*"[^>]*>([\s\S]*?)</a>"#).unwrap();
        let mut out = Vec::new();
        for block in http::parse_blocks_by_class(html, "result") {
            let Some(a) = link_re.captures(&block) else { continue };
            let url = ddg_normalize_url(&a[1]);
            let title = http::strip_tags(&a[2]);
            if url.is_empty() || title.is_empty() { continue; }
            let snippet = snip_re.captures(&block).map(|s| http::strip_tags(&s[1])).unwrap_or_default();
            out.push(SearchResult { title, url, snippet, source: String::new() });
        }
        out
    }

    #[test]
    fn anomaly_detection_matches_ts() {
        assert!(is_anomaly(202, "anomaly detected"));
        assert!(is_anomaly(200, "execDeep();"));
        assert!(!is_anomaly(500, "anomaly"));
        assert!(!is_anomaly(200, "обычная выдача"));
    }

    #[test]
    fn html_fixture_parses_titles_urls_snippets() {
        let out = parse_html_fixture(FIXTURE_HTML_RESULTS);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].title, "Rust guide");
        assert_eq!(out[0].url, "https://example.org/guide");
        assert_eq!(out[0].snippet, "Very useful snippet text");
        // Внутренний /l/?uddg= развернулся в целевой URL (double-decode).
        assert_eq!(out[1].url, "https://target.test/page");
    }

    #[test]
    fn lite_fixture_parses_both_attribute_orders() {
        // Обе формы ссылки: class='result-link' до href и href до class.
        let link_re = Regex::new(concat!(
            r#"(?i)<a[^>]+class='result-link'[^>]+href="([^"]+)"[^>]*>([\s\S]*?)</a>"#,
            r#"|<a[^>]+href="([^"]+)"[^>]+class='result-link'[^>]*>([\s\S]*?)</a>"#
        )).unwrap();
        let snip_re = Regex::new(r#"(?i)<td class='result-snippet'>([\s\S]*?)</td>"#).unwrap();
        let mut out = Vec::new();
        for caps in link_re.captures_iter(FIXTURE_LITE) {
            let (u, t) = match (caps.get(1), caps.get(2)) {
                (Some(u), Some(t)) => (u.as_str(), t.as_str()),
                _ => (caps.get(3).map(|m| m.as_str()).unwrap_or(""), caps.get(4).map(|m| m.as_str()).unwrap_or("")),
            };
            let after = &FIXTURE_LITE[caps.get(0).unwrap().end()..];
            out.push((
                ddg_normalize_url(u),
                http::strip_tags(t),
                snip_re.captures(after).map(|s| http::strip_tags(&s[1])).unwrap_or_default(),
            ));
        }
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, "https://one.test/a");
        assert_eq!(out[0].2, "first snippet");
        assert_eq!(out[1].0, "https://two.test/b");
        assert_eq!(out[1].2, "second snippet");
    }

    #[test]
    fn topic_push_builds_title_and_snippet() {
        let mut all = Vec::new();
        push_topic(&mut all, &serde_json::json!({
            "FirstURL": "https://topic.test/x",
            "Text": "Тема про <b>rust</b> с деталями"
        }));
        assert_eq!(all[0].url, "https://topic.test/x");
        // В TS title берётся из СЫРОГО t.Text (с тегами) — порт поведению 1-в-1.
        assert_eq!(all[0].title, "Тема про <b>rust</b> с деталями");
        // Пустые topic пропускаются молча.
        push_topic(&mut all, &serde_json::json!({"Text": "нет URL"}));
        assert_eq!(all.len(), 1);
    }
}
