//! Движок-менеджер тула `websearch` — порт движковой части `web_search.ts`.
//!
//! Каждый движок — свой файл в `engines/` с async `search(query, limit)`.
//! Здесь: реестр, DEFAULT_ORDER, цепочка фолбэков / параллельный режим,
//! дедуп по URL и по домену+URL, relevantScore (порог 0.3), чистка
//! tracking-параметров и форматирование вывода (1-в-1 с TS).

use std::future::Future;

use regex::Regex;
use super::stats;

pub mod baidu;
pub mod bing;
pub mod brave;
pub mod csdn;
pub mod duckduckgo;
pub mod exa;
pub mod juejin;
pub mod marginalia;
pub mod sogou;
pub mod startpage;
pub mod wiby;

pub const MAX_RESULTS: usize = 8;
pub const CHAIN_PAUSE_MS: u64 = 800;
pub const ENGINE_DEADLINE: std::time::Duration = std::time::Duration::from_millis(15_000);
/// UA Chrome 112 — используется несколькими движками (порт UA_CHROME_112).
pub const UA_CHROME_112: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/112.0.0.0 Safari/537.36";
/// UA Chrome 136 — sogou (порт).
pub const UA_136: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0 Safari/537.36";
/// UA Chrome 91 — baidu (порт).
pub const UA_91: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36";

/// Один результат поиска (порт `SearchResult`).
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    /// Движки заполняют не всегда: в TS поле опционально.
    pub source: String,
}

/// Ошибка движка для секции «Движки с ошибками».
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub name: String,
    pub message: String,
}

/// Имена движков в реестре — порядок как в TS `ENGINES` (Object.keys).
pub const ENGINE_NAMES: &[&str] = &[
    "duckduckgo",
    "wiby",
    "marginalia",
    "brave",
    "startpage",
    "sogou",
    "baidu",
    "bing",
    "juejin",
    "csdn",
    "exa",
];

/// Цепочка по надёжности — порт `DEFAULT_ORDER`.
pub const DEFAULT_ORDER: &[&str] = &[
    "duckduckgo",
    "wiby",
    "marginalia",
    "brave",
    "startpage",
    "sogou",
    "juejin",
    "csdn",
    "baidu",
    "bing",
    "exa",
];

/// Диспатч вызова движка (аналог ENGINES[name](q, l)).
pub async fn engine_search(name: &str, query: &str, limit: usize) -> Result<Vec<SearchResult>, String> {
    let future: std::pin::Pin<Box<dyn Future<Output = _> + Send>> = match name {
        "duckduckgo" => Box::pin(duckduckgo::search(query, limit)),
        "wiby" => Box::pin(wiby::search(query, limit)),
        "marginalia" => Box::pin(marginalia::search(query, limit)),
        "brave" => Box::pin(brave::search(query, limit)),
        "startpage" => Box::pin(startpage::search(query, limit)),
        "sogou" => Box::pin(sogou::search(query, limit)),
        "baidu" => Box::pin(baidu::search(query, limit)),
        "bing" => Box::pin(bing::search(query, limit)),
        "juejin" => Box::pin(juejin::search(query, limit)),
        "csdn" => Box::pin(csdn::search(query, limit)),
        "exa" => Box::pin(exa::search(query, limit)),
        other => return Err(format!("Неизвестный движок: {other}")),
    };
    future.await
}

/// Человекочитаемое описание HTTP-ошибок (порт `httpStatusError`).
pub fn http_status_error(status: u16) -> String {
    match status {
        429 => "лимит запросов (rate-limit) — попробуй позже".into(),
        521 => "антибот-защита сайта (521)".into(),
        403 => "доступ запрещён (403) — вероятна блокировка сервисом".into(),
        404 => "HTTP 404 (страница/эндпоинт не найден)".into(),
        503 => "сервис временно недоступен (503)".into(),
        _ => format!("HTTP {status}"),
    }
}

/// Лог статуса движка в stderr — порт engineLog (console.error).
/// Пустое имя — итоговая строка прогона (в TS это прямой console.error
/// без имени движка).
pub fn engine_log(name: &str, status: &str, extra: Option<&str>) {
    let body = match extra {
        Some(extra) => format!("{status} — {extra}"),
        None => status.to_string(),
    };
    let line = if name.is_empty() {
        format!("[ENGINE] {body}")
    } else {
        format!("[ENGINE] {name}: {body}")
    };
    let _ = std::io::Write::write_all(&mut std::io::stderr(), format!("{line}\n").as_bytes());
}

/// Дедуп по URL — порт `dedupe`.
pub fn dedupe(results: Vec<SearchResult>) -> Vec<SearchResult> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    results
        .into_iter()
        .filter(|r| {
            if r.url.is_empty() || !seen.insert(r.url.clone()) {
                return false;
            }
            true
        })
        .collect()
}

/// Домен URL — порт `extractDomain`.
pub fn extract_domain(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(u) => u.host_str().unwrap_or_default().to_string(),
        Err(_) => String::new(),
    }
}

/// Дедуп по домену + URL (порт `dedupeByDomain`, паттерн scira/web-search.ts).
/// web_search.ts сам её не зовёт — нужен этапу 2 (searxng_search.ts).
#[allow(dead_code)]
pub fn dedupe_by_domain(results: Vec<SearchResult>) -> Vec<SearchResult> {
    let mut seen_urls: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut seen_domains: std::collections::HashSet<String> = std::collections::HashSet::new();
    results
        .into_iter()
        .filter(|r| {
            if r.url.is_empty() || !seen_urls.insert(r.url.clone()) {
                return false;
            }
            let domain = extract_domain(&r.url);
            if !domain.is_empty() && !seen_domains.insert(domain) {
                return false;
            }
            true
        })
        .collect()
}

/// Оценка релевантности: доля результатов со значимым словом запроса
/// (порт `relevantScore`: слова ≥4 символов, порог 0.3 в runChain).
pub fn relevant_score(query: &str, results: &[SearchResult]) -> f64 {
    let word_chars = Regex::new(r"[^\p{L}\p{N}]").expect("константная регулярка");
    let words: Vec<String> = query
        .to_lowercase()
        .split_whitespace()
        .map(|w| word_chars.replace_all(w, "").into_owned())
        .filter(|w| w.chars().count() >= 4)
        .collect();
    if words.is_empty() || results.is_empty() {
        return 1.0;
    }
    let mut hit = 0usize;
    for r in results {
        let hay = format!("{} {}", r.title, r.snippet).to_lowercase();
        if words.iter().any(|w| hay.contains(w.as_str())) {
            hit += 1;
        }
    }
    hit as f64 / results.len() as f64
}

/// Убрать tracking-параметры (порт `cleanTrackingUrl`; порядок оставшихся
/// пар сохраняется, пере-кодирование — как у URLSearchParams: '+' для пробела).
pub fn clean_tracking_url(raw: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }
    let Ok(mut url) = url::Url::parse(raw) else {
        return raw.to_string();
    };
    let junk = Regex::new(r"(?i)^(ops_request_misc|request_id|biz_id|utm_|ttm_|spm|from|share_source|share_token)")
        .expect("константная регулярка");
    let pairs: Vec<(String, String)> = url.query_pairs()
        .filter(|(k, _)| !junk.is_match(&k))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if pairs.len() == url.query_pairs().count() {
        return raw.to_string(); // ничего не вырезано — строка не трогается
    }
    url.set_query(None);
    if !pairs.is_empty() {
        let mut ser = url::form_urlencoded::Serializer::new(String::new());
        for (k, v) in &pairs {
            ser.append_pair(k, v);
        }
        let query = ser.finish();
        url.set_query(Some(&query));
    }
    url.to_string()
}

/// Цепочка фолбэков (по умолчанию): первый рабочий движок выигрывает
/// — порт `runChain`. Возвращает сырые результаты + отказы (формат — как в TS).
pub async fn run_chain(
    query: &str,
    engines: &[String],
    limit: usize,
) -> (Vec<SearchResult>, Vec<Failure>) {
    let start = std::time::Instant::now();
    let mut all: Vec<SearchResult> = Vec::new();
    let mut failures: Vec<Failure> = Vec::new();
    let mut summary: Vec<String> = Vec::new();

    for name in engines {
        let t0 = std::time::Instant::now();
        match tokio::time::timeout(ENGINE_DEADLINE, engine_search(name, query, limit)).await {
            Err(_) => {
                let msg = "движок завис (дедлайн)".to_string();
                let sec = secs(t0.elapsed());
                stats::record_engine_call(name, false, t0.elapsed().as_millis() as u64, Some(&msg));
                engine_log(name, &format!("ОШИБКА за {sec}с"), Some(&msg));
                failures.push(Failure { name: name.clone(), message: msg.clone() });
                summary.push(format!("{name}:ERR"));
            }
            Ok(Err(msg)) => {
                let sec = secs(t0.elapsed());
                stats::record_engine_call(name, false, t0.elapsed().as_millis() as u64, Some(&msg));
                engine_log(name, &format!("ОШИБКА за {sec}с"), Some(&msg));
                failures.push(Failure { name: name.clone(), message: msg });
                summary.push(format!("{name}:ERR"));
            }
            Ok(Ok(found)) => {
                let sec = secs(t0.elapsed());
                if !found.is_empty() {
                    let score = relevant_score(query, &found);
                    stats::record_engine_call(name, true, t0.elapsed().as_millis() as u64, None);
                    let pct = (score * 100.0).round() as i64;
                    if score >= 0.3 {
                        let n = found.len();
                        engine_log(
                            name,
                            &format!("OK ({n} рез. за {sec}с, релевантность {pct}%)"),
                            None,
                        );
                        all.extend(found);
                        summary.push(format!("{name}:{n}"));
                        if all.len() >= limit {
                            break;
                        }
                    } else {
                        // Запасной вариант — вернём, если ничего лучше не найдём.
                        let n = found.len();
                        engine_log(
                            name,
                            &format!("нерелевантно ({n} рез. за {sec}с, совпало {pct}%)"),
                            None,
                        );
                        all.extend(found);
                        summary.push(format!("{name}:MISS"));
                    }
                } else {
                    engine_log(name, &format!("пусто (0 рез. за {sec}с)"), None);
                    summary.push(format!("{name}:0"));
                }
            }
        }
        // Пауза между движками цепочки (как в TS) — только если набор не полный.
        if all.len() < limit {
            tokio::time::sleep(std::time::Duration::from_millis(CHAIN_PAUSE_MS)).await;
        }
    }
    let total_sec = secs(start.elapsed());
    engine_log("", &format!("Итог по запросу \"{}\": {} | собрано {} рез. за {}с",
        clip(query, 60), summary.join(", "), all.len(), total_sec), None);
    (all, failures)
}

/// Параллельный режим (engines задан явно): одновременный запрос ко всем
/// — порт `runParallel`.
pub async fn run_parallel(
    query: &str,
    engines: &[String],
    limit: usize,
) -> (Vec<SearchResult>, Vec<Failure>) {
    let start = std::time::Instant::now();
    let per_engine = std::cmp::max(2, (limit as f64 / engines.len().max(1) as f64).ceil() as usize);
    let engines = engines.to_vec();
    let futures = engines.iter().map(|name| {
        let name = name.clone();
        async move {
            let t0 = std::time::Instant::now();
            match tokio::time::timeout(ENGINE_DEADLINE, engine_search(&name, query, per_engine)).await
            {
                Err(_) => {
                    let msg = "движок завис (дедлайн)".to_string();
                    let sec = secs(t0.elapsed());
                    stats::record_engine_call(&name, false, t0.elapsed().as_millis() as u64, Some(&msg));
                    engine_log(&name, &format!("ОШИБКА за {sec}с"), Some(&msg));
                    (name, Vec::new(), Some(msg))
                }
                Ok(Err(msg)) => {
                    let sec = secs(t0.elapsed());
                    stats::record_engine_call(&name, false, t0.elapsed().as_millis() as u64, Some(&msg));
                    engine_log(&name, &format!("ОШИБКА за {sec}с"), Some(&msg));
                    (name, Vec::new(), Some(msg))
                }
            Ok(Ok(found)) => {
                let sec = secs(t0.elapsed());
                let n = found.len();
                if n > 0 {
                    stats::record_engine_call(&name, true, t0.elapsed().as_millis() as u64, None);
                    engine_log(&name, &format!("OK ({n} рез. за {sec}с)"), None);
                } else {
                    engine_log(&name, &format!("пусто (0 рез. за {sec}с)"), None);
                }
                (name, found, None)
            }
            }
        }
    });
    let collected = futures_util::future::join_all(futures).await;

    let mut all = Vec::new();
    let mut failures = Vec::new();
    let mut summary = Vec::new();
    for (name, found, error) in collected {
        match error {
            Some(msg) => {
                summary.push(format!("{}:ERR", name));
                failures.push(Failure { name: name.clone(), message: msg });
            }
            None => {
                summary.push(format!("{}:{}", name, found.len()));
                all.extend(found);
            }
        }
    }
    let total_sec = secs(start.elapsed());
    engine_log("", &format!("Итог по запросу \"{}\": {} | собрано {} рез. за {}с",
        clip(query, 60), summary.join(", "), all.len(), total_sec), None);
    (all, failures)
}

fn secs(elapsed: std::time::Duration) -> String {
    format!("{:.1}", elapsed.as_secs_f64())
}

/// Верхушка строки для лога (port `query.slice(0, 60)`).
fn clip(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// Формат одного запроса — порт `formatResults` (байт-в-байт с текстом TS).
pub fn format_results(
    all: Vec<SearchResult>,
    failures: &[Failure],
    limit: usize,
) -> String {
    let deduped = dedupe(all).into_iter().take(limit).collect::<Vec<_>>();
    let mut lines: Vec<String> = deduped
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let extra = if r.source.is_empty() {
                String::new()
            } else {
                format!(" ({})", r.source)
            };
            format!(
                "[{}] {}{}\nСсылка: {}\nОписание: {}",
                i + 1,
                r.title,
                extra,
                r.url,
                if r.snippet.is_empty() { "—".to_string() } else { r.snippet.clone() }
            )
        })
        .collect();
    if !failures.is_empty() {
        lines.push(format!(
            "\n⚠️ Движки с ошибками: {}",
            failures
                .iter()
                .map(|f| format!("{} ({})", f.name, f.message))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    lines.join("\n")
}

/// Формат multi-query — порт `formatMultiResults` байт-в-байт: секции по
/// каждому запросу, сквозная нумерация, дедуп по домену МЕЖДУ запросами
/// (рукописный цикл TS, не `dedupeByDomain`: без dedupe по URL — URL уже
/// вычищены в обработчике).
pub fn format_multi_results(
    per_query: &[(&str, Vec<SearchResult>, Vec<Failure>)],
    limit: usize,
) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut seen_domains: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut index = 1usize;
    for (query, all, failures) in per_query {
        out.push(format!("=== Запрос: {query} ==="));
        for r in all {
            if index > limit {
                break;
            }
            let domain = extract_domain(&r.url);
            if !domain.is_empty() {
                if seen_domains.contains(&domain) {
                    continue;
                }
                seen_domains.insert(domain);
            }
            let extra = if r.source.is_empty() { String::new() } else { format!(" ({})", r.source) };
            out.push(format!(
                "[{}] {}{}\nСсылка: {}\nОписание: {}",
                index,
                r.title,
                extra,
                r.url,
                if r.snippet.is_empty() { "—".to_string() } else { r.snippet.clone() }
            ));
            index += 1;
        }
        if !failures.is_empty() {
            out.push(format!(
                "⚠️ Движки с ошибками: {}",
                failures
                    .iter()
                    .map(|f| format!("{} ({})", f.name, f.message))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        out.push(String::new());
    }
    out.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(url: &str) -> SearchResult {
        SearchResult { title: "t".into(), url: url.into(), snippet: "s".into(), source: "".into() }
    }

    #[test]
    fn dedupe_keeps_first_url_only() {
        let out = dedupe(vec![hit("https://a/1"), hit("https://a/1"), hit("https://b/1")]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].url, "https://a/1");
    }

    #[test]
    fn dedupe_by_domain_keeps_one_per_domain() {
        let out = dedupe_by_domain(vec![
            hit("https://a.com/1"),
            hit("https://a.com/2"),
            hit("https://b.org/1"),
            hit("https://b.org/2"),
        ]);
        let urls: Vec<&str> = out.iter().map(|r| r.url.as_str()).collect();
        assert_eq!(urls, vec!["https://a.com/1", "https://b.org/1"]);
    }

    #[test]
    fn relevant_score_counts_word_hits() {
        let a = SearchResult {
            title: "Rust tokio guide".into(),
            url: "u".into(),
            snippet: "async runtime".into(),
            source: "".into(),
        };
        let b = SearchResult {
            title: "Совсем другое".into(),
            url: "u2".into(),
            snippet: "".into(),
            source: "".into(),
        };
        let score = relevant_score("rust tokio", &[a.clone(), b]);
        assert_eq!(score, 0.5);
        // Слова короче 4 символов игнорируются — тогда всё «релевантно».
        assert_eq!(relevant_score("ау", &[a.clone()]), 1.0);
        // Пустые результаты не делят на ноль.
        assert_eq!(relevant_score("rust", &[]), 1.0);
    }

    #[test]
    fn clean_tracking_url_removes_junk_params_and_keeps_others() {
        assert_eq!(
            clean_tracking_url("https://b.csdn.net/p?ops_request_misc=x&utm_source=y&id=7"),
            "https://b.csdn.net/p?id=7"
        );
        assert_eq!(
            clean_tracking_url("https://site.t/p?a=1"),
            "https://site.t/p?a=1",
            "чистая строка не переписывается"
        );
    }

    #[test]
    fn format_results_uses_ts_layout() {
        let all = vec![SearchResult {
            title: "Заголовок".into(),
            url: "https://u/1".into(),
            snippet: "описание".into(),
            source: "site.ru".into(),
        }];
        let failures = vec![Failure { name: "brave".into(), message: "капча".into() }];
        let text = format_results(all, &failures, 8);
        assert_eq!(
            text,
            "[1] Заголовок (site.ru)\nСсылка: https://u/1\nОписание: описание\n\n⚠️ Движки с ошибками: brave (капча)"
        );
    }

    #[test]
    fn format_results_omits_empty_source_and_dash_snippet() {
        let all = vec![SearchResult {
            title: "T".into(),
            url: "https://u/2".into(),
            snippet: String::new(),
            source: String::new(),
        }];
        let text = format_results(all, &[], 8);
        assert!(text.starts_with("[1] T\nСсылка: https://u/2\nОписание: —"));
    }

    #[test]
    fn multi_format_numbers_across_queries_and_dedups_domains() {
        let q1 = vec![hit("https://a.com/1"), hit("https://b.com/2")];
        let q2 = vec![hit("https://a.com/3"), hit("https://c.com/4")];
        let failures1 = vec![Failure { name: "bing".into(), message: "HTTP 429".into() }];
        let text = format_multi_results(&[("первый", q1, failures1), ("второй", q2, vec![])], 8);
        assert!(text.contains("=== Запрос: первый ==="));
        assert!(text.contains("[1] t"));
        assert!(text.contains("https://a.com/1"));
        assert!(text.contains("https://b.com/2"));
        // Домен a.com во втором запросе уже занят — остаётся только c.com.
        assert!(text.contains("https://c.com/4"));
        assert!(!text.contains("https://a.com/3"));
        assert!(text.contains("⚠️ Движки с ошибками: bing (HTTP 429)"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn engine_result() {
        // Диспатч называет неизвестный движок честным текстом ошибки.
        let err = engine_search("нет-такого", "q", 8).await.unwrap_err();
        assert_eq!(err, "Неизвестный движок: нет-такого");
    }

    #[test]
    fn http_status_error_messages_match_ts() {
        assert_eq!(http_status_error(429), "лимит запросов (rate-limit) — попробуй позже");
        assert_eq!(http_status_error(521), "антибот-защита сайта (521)");
        assert_eq!(http_status_error(403), "доступ запрещён (403) — вероятна блокировка сервисом");
        assert_eq!(http_status_error(404), "HTTP 404 (страница/эндпоинт не найден)");
        assert_eq!(http_status_error(503), "сервис временно недоступен (503)");
        assert_eq!(http_status_error(400), "HTTP 400");
    }
}
