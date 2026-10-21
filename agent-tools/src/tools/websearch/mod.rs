//! Тул `websearch` — полный порт `web_search.ts` (King Orch): 11 движков
//! DuckDuckGo/Wiby/Marginalia/Brave/Startpage/Sogou/Baidu/Bing/Juejin/CSDN/Exa,
//! цепочка фолбэков или параллельный режим, multi-query с дедупом по домену,
//! кеш на диске, статистика движков, CLI и MCP с единым обработчиком.
//!
//! Режимы вызова (см. `cli.rs`):
//!   websearch <запрос>       — одиночный поиск,
//!   websearch mcp            — MCP-сервер (tools/list + tools/call WebSearch),
//!   websearch --stats        — сводка сбоев движков.
//!
//! Код возврата: 0 успех, 2 ошибка ввода (нет query, битый --engines), 3 сети
//! (все движки не дали результатов). Причины движков — в stderr.
//!
//! Тестовая вёрстка: сетевые юнит-тесты запрещены (гейт Г4 — весь пакет
//! зелёный без сети), поэтому валидация выделена в чистые функции
//! ([`parse_limit`], [`resolve_engines`]), а реальный вызов движков проверен
//! стаб-сквозняками `tests/websearch_e2e.rs` (гейт Г3).

use std::sync::OnceLock;

use serde_json::Value;

use crate::exit::Exit;

pub(crate) mod cache;
pub(crate) mod cli;
pub(crate) mod engines;
pub(crate) mod http;
pub(crate) mod mcp;
pub(crate) mod stats;
pub(crate) mod store;

use engines::MAX_RESULTS;

/// Рантайм tokio один на процесс: CLI-вызову и каждому tools/call нужно
/// асинхронное ядро, а строить его дважды дорого.
pub(crate) fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("обычный рантайм tokio обязан строиться")
    })
}

/// Ошибка SSOT-обработчика с кодом выхода для CLI.
#[derive(Debug, Clone)]
pub(crate) struct HandlerError {
    pub exit: Exit,
    pub message: String,
}

impl HandlerError {
    fn input(message: impl Into<String>) -> Self {
        Self { exit: Exit::Input, message: message.into() }
    }
    fn network(message: impl Into<String>) -> Self {
        Self { exit: Exit::Network, message: message.into() }
    }
}

/// Порт `parseInt(args.limit, 10) || MAX_RESULTS` + клипп `Math.max(3, Math.min(20, …))`.
/// Семантика JS parseInt: ведущие пробелы/знак/цифры (суффикс отбрасывается),
/// строки без цифр и не-числа → NaN → дефолт; 0 (falsy) → тоже дефолт,
/// как в TS (`0 || MAX_RESULTS`).
pub(crate) fn parse_limit(raw: Option<&Value>) -> usize {
    let parsed: Option<i64> = match raw {
        Some(Value::Number(n)) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f as i64)),
        Some(Value::String(s)) => js_parse_int(s),
        _ => None,
    };
    // `X || MAX_RESULTS`: NaN и 0 подменяются дефолтом.
    let value = match parsed {
        Some(0) | None => MAX_RESULTS as i64,
        Some(v) => v,
    };
    value.clamp(3, 20) as usize
}

/// JS `parseInt(строка, 10)`: `[ws] [sign] digits`. Всё прочее — None (NaN).
fn js_parse_int(s: &str) -> Option<i64> {
    let bytes = s.as_bytes();
    let mut idx = 0usize;
    let mut sign = 1i64;
    if idx < bytes.len() && (bytes[idx] == b'-' || bytes[idx] == b'+') {
        if bytes[idx] == b'-' {
            sign = -1;
        }
        idx += 1;
    }
    if idx >= bytes.len() || !bytes[idx].is_ascii_digit() {
        return None;
    }
    let end = bytes[idx..]
        .iter()
        .position(|b| !b.is_ascii_digit())
        .map(|p| idx + p)
        .unwrap_or(bytes.len());
    s[idx..end].parse::<i64>().ok().and_then(|v| {
        //溢 на u64-диапазоне практических значений не бывает; всё равно защищаемся.
        sign.checked_mul(v).or(Some(if sign > 0 { i64::MAX } else { i64::MIN }))
    })
}

/// Разрешение списка движков (порт ветки `Array.isArray(args.engines)`).
/// Возвращает resolved-список, флаг parallel и опциональную ошибку ввода —
/// только на случай «все имена неизвестны» (как в TS).
pub(crate) fn resolve_engines(args: &Value) -> Result<(Vec<String>, bool), HandlerError> {
    // Дефолт — ранкинг из боевой статистики (PRP Шаг 1, SSOT stats.rs);
    // пустая статистика → fallback DEFAULT_ORDER внутри engines::default_chain.
    // Явный список engines=[...] обходу не мешается (cooldown не применяется).
    let default: Vec<String> = engines::default_chain();
    let Some(list) = args
        .get("engines")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(|e| e.trim().to_lowercase())
                .collect::<Vec<_>>()
        })
    else {
        return Ok((default, false));
    };

    // Даже если дальше часть имён отсечётся как неизвестные, режим остаётся
    // параллельным — как в TS (parallel ставится ДО отсечения).
    let parallel = !list.is_empty();
    let unknown: Vec<String> = list
        .iter()
        .filter(|e| !engines::ENGINE_NAMES.contains(&e.as_str()))
        .cloned()
        .collect();
    if unknown.is_empty() {
        // Как в TS: unknown пуст → пересечения нет, список идёт в поиск как есть.
        return Ok((list, parallel));
    }
    engines::engine_log(
        "web_search",
        &format!("неизвестные движки: {}", unknown.join(", ")),
        None,
    );
    let engines: Vec<String> = list
        .into_iter()
        .filter(|e| engines::ENGINE_NAMES.contains(&e.as_str()))
        .collect();
    if engines.is_empty() {
        return Err(HandlerError::input(format!(
            "WebSearch: неизвестные движки: {}. Доступны: {}",
            unknown.join(", "),
            engines::ENGINE_NAMES.join(", ")
        )));
    }
    Ok((engines, parallel))
}

/// Единый SSOT-обработчик поиска (порт `handlers.WebSearch` из web_search.ts):
/// сюда приходят аргументы и из CLI, и из MCP — логика одна.
pub(crate) async fn handle(args: &Value) -> Result<String, HandlerError> {
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let mut queries: Vec<String> = args
        .get("queries")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|q| !q.is_empty())
                .map(str::to_string)
                .take(5)
                .collect()
        })
        .unwrap_or_default();
    if queries.is_empty() && !query.is_empty() {
        queries = vec![query];
    }
    if queries.is_empty() {
        return Err(HandlerError::input("WebSearch: укажи 'query' или 'queries'."));
    }

    let limit = parse_limit(args.get("limit"));
    let (mut engines, mut parallel) = resolve_engines(args)?;

    // Маршрутизация адресных движков (PRP Шаг 4): префикс в тексте запроса
    // Перекрывает дефолт-цепочку ТОЛЬКО когда явный engines список не задан
    // (явный `--engines` юзера имеет приоритет над авторазрутированием).
    let explicit_engines = args
        .get("engines")
        .and_then(Value::as_array)
        .map(|a| !a.is_empty())
        .unwrap_or(false);
    if !explicit_engines {
        match engines::api_search::apply_prefixes(&mut queries) {
            Ok(Some(name)) => {
                // Префикс однозначен: рулит ТОЛЬКО адресный движок
                // (универсальные не запускаются), режим параллельный.
                engines = vec![name.to_string()];
                parallel = true;
            }
            Ok(None) => {}
            Err(msg) => return Err(HandlerError::input(format!("WebSearch: {msg}"))),
        }
    }

    let cache_key_str = cache::cache_key("web", &engines, parallel, &queries.join("|"));
    if let Some(cached) = cache::cache_get(&cache_key_str) {
        engines::engine_log(
            "web_search",
            &format!("из кеша (query: \"{}\")", clip_chars(&queries[0], 60)),
            None,
        );
        return Ok(cached);
    }

    let per_engine_limit = std::cmp::max(3, (limit as f64 / queries.len() as f64).ceil() as usize);
    let futures = queries.iter().map(|q| {
        let engines = engines.clone();
        async move {
            let (all, failures) = if parallel {
                engines::run_parallel(q, &engines, per_engine_limit).await
            } else {
                engines::run_chain(q, &engines, per_engine_limit).await
            };
            (q.as_str(), engines::dedupe(all), failures)
        }
    });
    let per_query: Vec<(&str, Vec<engines::SearchResult>, Vec<engines::Failure>)> =
        futures_util::future::join_all(futures).await;

    let total: usize = per_query.iter().map(|(_, all, _)| all.len()).sum();
    if total == 0 {
        let reasons = per_query
            .iter()
            .flat_map(|(_, _, failures)| failures.iter().map(|f| format!("{}: {}", f.name, f.message)))
            .collect::<Vec<_>>()
            .join("; ");
        let tail = if reasons.is_empty() {
            "Все движки вернули пусто.".to_string()
        } else {
            format!("Причины: {reasons}")
        };
        return Err(HandlerError::network(format!("Поиск не дал результатов. {tail}")));
    }

    let out = if queries.len() == 1 {
        // TS: formatResults(perQuery[0].all, perQuery[0].failures, limit).
        let (_, all, failures) = &per_query[0];
        engines::format_results(all.clone(), failures, limit)
    } else {
        engines::format_multi_results(
            &per_query
                .iter()
                .map(|(q, all, failures)| (*q, all.clone(), failures.clone()))
                .collect::<Vec<_>>(),
            limit,
        )
    };
    cache::cache_put(&cache_key_str, &out);
    Ok(out)
}

/// Верхушка строки для лога (`query.slice(0, 60)`).
fn clip_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// Точка входа реестра тула (вызывается из tools/mod.rs). Полный `argv`
/// включает бинарь (argv[0]) и имя инструмента (argv[1]) — они не относятся
/// к парсеру CLI (регрессия codemap/cli.rs:424 — здесь та же).
pub(crate) fn run(args: Vec<String>) -> Exit {
    cli::run_to_exit(args.get(2..).unwrap_or(&[]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn handler_requires_query_or_queries() {
        let err = runtime().block_on(handle(&json!({}))).unwrap_err();
        assert_eq!(err.exit, Exit::Input);
        assert!(err.message.contains("укажи 'query'"));
    }

    #[test]
    fn limit_is_clamped_ts_style() {
        assert_eq!(parse_limit(None), 8);
        assert_eq!(parse_limit(Some(&json!(50))), 20);
        assert_eq!(parse_limit(Some(&json!(1))), 3);
        assert_eq!(parse_limit(Some(&json!(10))), 10);
        assert_eq!(parse_limit(Some(&json!("50"))), 20);
        assert_eq!(parse_limit(Some(&json!("мусор"))), 8);
        assert_eq!(parse_limit(Some(&json!(true))), 8);
        // Семантика JS parseInt: суффикс отбрасывается, 0 — falsy → дефолт,
        // отрицательные клампятся нижней границей (max(3, min(20, -5)) = 3),
        // дробные — к целой части (parseInt(1.5) = 1).
        assert_eq!(parse_limit(Some(&json!("50шт"))), 20);
        assert_eq!(parse_limit(Some(&json!("5и"))), 5);
        assert_eq!(parse_limit(Some(&json!(0))), 8);
        assert_eq!(parse_limit(Some(&json!(-5))), 3);
        assert_eq!(parse_limit(Some(&json!(1.5))), 3);
        assert_eq!(parse_limit(Some(&json!(""))), 8);
    }

    #[test]
    fn unknown_engines_error_lists_all_available() {
        let err = resolve_engines(&json!({ "engines": ["yahoo"] })).unwrap_err();
        assert_eq!(err.exit, Exit::Input);
        assert!(err.message.contains("неизвестные движки: yahoo"));
        assert!(err.message.contains("Доступны:"));
    }

    #[test]
    fn engines_without_array_gives_default_chain_not_parallel() {
        let (engines, parallel) = resolve_engines(&json!({})).expect("дефолт");
        assert!(!parallel);
        // Дефолт — ранкинг из stats: состав может отличаться от DEFAULT_ORDER
        // (dead-движки отсеиваются), поэтому проверяем честные инварианты.
        assert!(!engines.is_empty(), "дефолтная цепочка не пустая");
        for name in &engines {
            assert!(engines::ENGINE_NAMES.contains(&name.as_str()), "имя {name} из реестра");
        }
    }

    #[test]
    fn mixed_engines_keep_unknown_logged_and_valid_kept() {
        let (engines, parallel) = resolve_engines(&json!({ "engines": ["EXA", "yahoo", "wiby"] }))
            .expect("валидные имена остаются");
        assert!(parallel, "в TS параллельность ставится ДО отсечения неизвестных");
        assert_eq!(engines, vec!["exa".to_string(), "wiby".to_string()]);
    }

    #[test]
    fn engines_array_of_nonstrings_means_empty_list_not_error() {
        // TS: [7] → filter(typeof string) → [] → parallel=false;
        // unknown тоже [] → throw НЕТ (проверено на web_search.ts:771-776),
        // пустой список идёт дальше и даёт «Все движки вернули пусто.».
        let (engines, parallel) = resolve_engines(&json!({ "engines": [7] })).expect("пустой список");
        assert!(!parallel);
        assert!(engines.is_empty());
        // Пустые строки после trim тоже остаются (TS map(toLowerCase().trim)):
        // '' → это "неизвестный" движок → ошибка с пустым unknown, как в TS.
        let err = resolve_engines(&json!({ "engines": [""] })).unwrap_err();
        assert_eq!(err.exit, Exit::Input);
        assert!(err.message.contains("неизвестные движки: "));
    }

    #[test]
    fn queries_more_than_five_are_cut() {
        let queries = handle_queries_slice(&json!({
            "queries": ["a", "b", "c", "d", "e", "f", "g"]
        }));
        assert_eq!(queries.len(), 5);
        assert_eq!(queries[4], "e");
    }

    fn handle_queries_slice(args: &Value) -> Vec<String> {
        args.get("queries")
            .and_then(Value::as_array)
            .map(|items| {
                items.iter().filter_map(Value::as_str).map(str::trim)
                    .filter(|q| !q.is_empty()).map(str::to_string).take(5).collect()
            })
            .unwrap_or_default()
    }
}
