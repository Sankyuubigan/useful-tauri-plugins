//! Форматирование вывода websearch — перенесено из `engines/mod.rs`
//! (PRP Шаг 5): `format_results` (порт `formatResults`) и
//! `format_multi_results` (порт `formatMultiResults`) байт-в-байт с TS.

use super::{dedupe, extract_domain, Failure, SearchResult};

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
    fn format_results_dedupes_urls_and_respects_limit() {
        let all = vec![hit("https://a/1"), hit("https://a/1"), hit("https://b/1"), hit("https://c/1")];
        let text = format_results(all, &[], 3);
        assert!(text.contains("[1] t"));
        assert!(text.contains("https://b/1"));
        assert!(text.contains("https://c/1"));
        // limit=3, уникальных 3 — всё влезло; [4] не должно появиться.
        assert!(!text.contains("[4]"), "n° результата за пределами лимита нет");
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
}
