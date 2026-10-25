//! Движок-менеджер тула `websearch` — порт движковой части `web_search.ts`.
//!
//! Каждый движок — свой файл в `engines/` с async `search(query, limit)`.
//! Здесь: реестр, DEFAULT_ORDER, цепочка фолбэков / параллельный режим,
//! дедуп по URL и по домену+URL, relevantScore (порог 0.3), чистка
//! tracking-параметров и форматирование вывода (1-в-1 с TS).

use std::future::Future;

use regex::Regex;
use super::stats;

pub mod api_search;
pub mod baidu;
pub mod bing;
pub mod brave;
pub mod crates;
pub mod csdn;
pub mod duckduckgo;
pub mod engine_ranking;
pub mod exa;
pub mod format;
pub mod huggingface;
pub mod juejin;
pub mod marginalia;
pub mod mdn;
pub mod npm;
pub mod sogou;
pub mod startpage;
pub mod stackexchange;
pub mod text_util;
pub mod wiby;
pub mod wikimedia;

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

/// Имена движков в реестре. Адресные движки (commons, stackexchange, npm,
/// mdn, huggingface, crates) в списке есть, но в DEFAULT_ORDER не входят
/// (PRP Шаг 4: строгое правило маршрутизации).
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
    // Доменный/адресные движки:
    "commons",
    "stackexchange",
    "npm",
    "mdn",
    "huggingface",
    "crates",
];

/// Цепочка по надёжности — порт `DEFAULT_ORDER`. Остаётся fallback
/// при пустой статистике (PRP Шаг 1). Адресные движки (commons и 5 API)
/// сюда НЕ входят: цепочка зовёт только универсальные —commons мог бы
/// отсекать непрофильные запросы «пусто без HTTP», но делая HTTP ВСЕХ
/// остальных — большинство запросов заканчивалось бы раньше; clean-
/// решение — commons живёт ТОЛЬКО в адресном режиме (PRP core §1.9).
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

/// Дефолтная цепочка с учётом боевой статистики (PRP Шаг 1): порядок
/// считается из search_stats.json; пустая статистика → fallback выше.
pub fn default_chain() -> Vec<String> {
    engine_ranking::current_default_chain()
}

/// Дефолтная цепочка под конкретный батч запросов: статистический фильтр
/// (мёртвые/cooldown) + языковой гейт — без CJK китайские платформы
/// (juejin/csdn/baidu/sogou) уходят в хвост, после западных движков.
pub fn default_chain_for_queries(queries: &[String]) -> Vec<String> {
    engine_ranking::demote_cjk_engines(default_chain(), queries)
}

/// Дверь текстовых утилит (PRP Шаг 5): движки зовут `super::clip_chars`,
/// а не чужой модуль.
pub use text_util::clip_chars;

/// Дверь форматирования (вынесено в format.rs, PRP Шаг 5).
pub use format::{format_multi_results, format_results};

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
        "commons" => Box::pin(wikimedia::search(query, limit)),
        "stackexchange" => Box::pin(stackexchange::search(query, limit)),
        "npm" => Box::pin(npm::search(query, limit)),
        "mdn" => Box::pin(mdn::search(query, limit)),
        "huggingface" => Box::pin(huggingface::search(query, limit)),
        "crates" => Box::pin(crates::search(query, limit)),
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

/// Оценка релевантности: доля результатов, в которых совпало достаточное
/// число РАЗЛИЧНЫХ слов запроса (слова ≥4 символов, дубли схлопываются).
/// «Достаточно» — ≥2 слов, для коротких запросов (1–2 слова) — ≥50% слов.
/// Одиночное совпадение в длинном запросе не считается релевантностью:
/// боевая выдача (окт 2026) набирала «релевантность 100%» на одном токене —
/// `upright` в CSS-статье, порт `9001` в статье про MinIO.
pub fn relevant_score(query: &str, results: &[SearchResult]) -> f64 {
    let word_chars = Regex::new(r"[^\p{L}\p{N}]").expect("константная регулярка");
    let mut words: Vec<String> = query
        .to_lowercase()
        .split_whitespace()
        .map(|w| word_chars.replace_all(w, "").into_owned())
        .filter(|w| w.chars().count() >= 4)
        .collect();
    words.sort();
    words.dedup();
    if words.is_empty() || results.is_empty() {
        return 1.0;
    }
    let half = (words.len() + 1) / 2; // ≥50% слов
    let min_hits = half.max(if words.len() <= 2 { 1 } else { 2 });
    let mut hit = 0usize;
    for r in results {
        let hay = format!("{} {}", r.title, r.snippet).to_lowercase();
        let matched = words.iter().filter(|w| hay.contains(w.as_str())).count();
        if matched >= min_hits {
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
                    // Порог 0.5: при наивном 0.3 мусорный движок, набравший
                    // лимит, обрывал цепочку до DDG/brave (боевая регрессия).
                    if score >= 0.5 {
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
        clip_chars(query, 60), summary.join(", "), all.len(), total_sec), None);
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
        clip_chars(query, 60), summary.join(", "), all.len(), total_sec), None);
    (all, failures)
}

fn secs(elapsed: std::time::Duration) -> String {
    format!("{:.1}", elapsed.as_secs_f64())
}

// Форматирование вывода (formatResults/formatMultiResults) вынесено
// в `format.rs` (PRP Шаг 5, счётчик строк — core §2.4).

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
    fn single_token_match_in_long_query_is_not_relevant() {
        // Боевые кейсы окт 2026: одно совпадение токена не делает результат
        // релевантным.
        let css = SearchResult {
            title: "数字滚动效果实现 text-orientation: upright".into(),
            url: "https://juejin.cn/post/1".into(),
            snippet: "CSS свойство".into(),
            source: "".into(),
        };
        // CSS-статья со словом `upright` из запроса про tarot dataset.
        let score =
            relevant_score("tarot card dataset upright reversed meanings github", &[css]);
        assert_eq!(score, 0.0);
        // MinIO-статья с портом `9001` из запроса про RFC QUIC.
        let minio = SearchResult {
            title: "MinIO console default port 9001".into(),
            url: "https://min.io/x".into(),
            snippet: "secret key setup".into(),
            source: "".into(),
        };
        let score = relevant_score("RFC 9001 QUIC initial secret derivation", &[minio]);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn multi_word_match_is_relevant() {
        let gh = SearchResult {
            title: "GitHub - tarot deck JSON dataset".into(),
            url: "https://github.com/x".into(),
            snippet: "78 cards upright reversed keywords".into(),
            source: "".into(),
        };
        let score = relevant_score("tarot card dataset upright reversed meanings github", &[gh]);
        assert_eq!(score, 1.0);
    }

    #[test]
    fn short_query_needs_half_words() {
        let rust_only = SearchResult {
            title: "Rust book".into(),
            url: "u".into(),
            snippet: String::new(),
            source: "".into(),
        };
        // 2 слова: 50% — одного совпадения достаточно.
        assert_eq!(relevant_score("rust tokio", &[rust_only.clone()]), 1.0);
        // 3 слова: нужно ≥2 различных.
        assert_eq!(relevant_score("rust tokio server", &[rust_only]), 0.0);
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

    // Формат-тесты переехали в `format.rs` (PRP Шаг 5).

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
