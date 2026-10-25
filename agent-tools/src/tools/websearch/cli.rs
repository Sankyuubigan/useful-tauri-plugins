//! Парсер аргументов CLI для `agent-tools websearch` (аналог `consult/cli.rs`).
//!
//! Контракт (см. PRP §5.2):
//!   agent-tools websearch ["тариф"] [--queries "a,b"] [--engines "exa,baidu"]
//!                        [--limit 10] [--stats]
//!   agent-tools websearch mcp          — режим MCP-сервера (stdin/stdout)
//!
//! Парсер ручной, а не clap: позиционное слово `mcp` — часть контракта тула
//! (движок Claude Desktop-стиля), и менять эту семантику на флаг нельзя.
//! Отсутствие query/queries → код 2, неизвестный движок → код 2 с полным
//! списком доступных.

use super::engines::ENGINE_NAMES;
use crate::exit::Exit;

/// Опции одного «обычного» (не MCP) вызова.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Options {
    /// Запросы: одна позиционная строка ИЛИ значения `--queries` (до 5).
    pub queries: Vec<String>,
    /// Явные движки (параллельный режим) или None (цепочка DEFAULT_ORDER).
    pub engines: Option<Vec<String>>,
    /// Лимит результатов, после клиппа 3..=20.
    pub limit: u64,
}

/// Что делать в `run()` после разбора аргументов.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Intent {
    /// Лом-сервер JSON-RPC.
    Mcp,
    /// Печать сводки статистики движков.
    Stats,
    /// Обычный поиск.
    Ask(Options),
}

pub(crate) const USAGE: &str = "\
Использование:
  agent-tools websearch <запрос> [--engines e1,e2] [--limit N] [--queries \"a,b\"]
  agent-tools websearch mcp
  agent-tools websearch --stats

Опции:
  --queries \"q1,q2\"   до 5 запросов (multi-query, дедуп по домену+URL)
  --engines \"a,b\"     движки для параллельного поиска; без флага — цепочка фолбэков
  --limit N            максимум результатов (3..20, по умолчанию 8)
  --stats              сводка статистики движков
Движки: duckduckgo, wiby, marginalia, brave, startpage, sogou, baidu, bing, juejin, csdn, exa
Тестовый крюк: env AGENT_TOOLS_WEBSEARCH_BASE_URL подменяет origin всех запросов (для стабов).";

/// Разобрать argv. Ошибка — текст для stderr (код возврата 2).
pub(crate) fn parse(args: &[String]) -> Result<Intent, String> {
    if args.is_empty() {
        return Err(USAGE.to_string());
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        // «Помощь» печатается как обычный успех: код 0, текст в stdout.
        return Err(format!("help:{USAGE}"));
    }
    if args[0] == "mcp" {
        return Ok(Intent::Mcp);
    }

    let mut positional: Vec<String> = Vec::new();
    let mut queries: Option<Vec<String>> = None;
    let mut engines: Option<Vec<String>> = None;
    let mut limit: Option<u64> = None;
    let mut stats = false;

    let mut i = 0usize;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--stats" => stats = true,
            "--queries" | "--engines" | "--limit" => {
                let Some(value) = args.get(i + 1) else {
                    return Err(format!("нет значения для {arg}"));
                };
                match arg.as_str() {
                    "--queries" => {
                        let list: Vec<String> = value
                            .split(',')
                            .map(|q| q.trim().to_string())
                            .filter(|q| !q.is_empty())
                            .collect();
                        if list.is_empty() {
                            return Err("--queries: нет ни одного непустого запроса".into());
                        }
                        if list.len() > 5 {
                            return Err(format!("--queries: максимум 5 запросов, получено {}", list.len()));
                        }
                        queries = Some(list);
                    }
                    "--engines" => {
                        let list: Vec<String> = value
                            .split(',')
                            .map(|e| e.trim().to_lowercase())
                            .filter(|e| !e.is_empty())
                            .collect();
                        if list.is_empty() {
                            return Err("--engines: нет ни одного непустого имени".into());
                        }
                        let unknown: Vec<String> = list
                            .iter()
                            .filter(|e| !ENGINE_NAMES.contains(&e.as_str()))
                            .cloned()
                            .collect();
                        if !unknown.is_empty() {
                            return Err(format!(
                                "неизвестные движки: {}. Доступны: {}",
                                unknown.join(", "),
                                ENGINE_NAMES.join(", ")
                            ));
                        }
                        engines = Some(list);
                    }
                    "--limit" => {
                        let Ok(parsed) = value.parse::<u64>() else {
                            return Err(format!("--limit: не число: {value}"));
                        };
                        if !(3..=20).contains(&parsed) {
                            return Err(format!("--limit: разрешено 3..20, получено {parsed}"));
                        }
                        limit = Some(parsed);
                    }
                    _ => unreachable!("ветка перечислена в match выше"),
                }
                i += 2;
                continue;
            }
            other => positional.push(other.to_string()),
        }
        i += 1;
    }

    if stats && positional.is_empty() && queries.is_none() {
        return Ok(Intent::Stats);
    }

    let mut final_queries = queries.unwrap_or_default();
    // Позиционный запрос не теряется при наличии --queries: идёт первым
    // элементом списка (боевой кейс: модель передавала и то, и другое,
    // а основной запрос молча выбрасывался).
    if !positional.is_empty() {
        let q = positional.join(" ");
        if !q.trim().is_empty() {
            final_queries.insert(0, q.trim().to_string());
        }
    }
    if final_queries.is_empty() {
        if stats {
            return Ok(Intent::Stats);
        }
        return Err(USAGE.to_string());
    }
    final_queries.truncate(5);

    Ok(Intent::Ask(Options {
        queries: final_queries,
        engines,
        limit: limit.unwrap_or(super::engines::MAX_RESULTS as u64),
    }))
}

/// Точка входа тула: разбор → исполнение → код возврата.
pub(crate) fn run_to_exit(args: &[String]) -> Exit {
    let intent = match parse(args) {
        Ok(intent) => intent,
        Err(text) => {
            // Формат «help:...» означает: пользователь попросил помощь — это
            // обычный вывод, а не ошибка.
            match text.strip_prefix("help:") {
                Some(usage) => {
                    println!("{usage}");
                    return Exit::Ok;
                }
                None => {
                    eprintln!("{text}");
                    return Exit::Input;
                }
            }
        }
    };
    match intent {
        Intent::Mcp => super::mcp::serve(),
        Intent::Stats => {
            println!("{}", super::stats::stats_summary());
            Exit::Ok
        }
        Intent::Ask(options) => {
            let mut arguments = serde_json::json!({ "queries": options.queries });
            if let Some(engines) = &options.engines {
                arguments["engines"] = serde_json::json!(engines);
            }
            arguments["limit"] = serde_json::json!(options.limit);
            let runtime = super::runtime();
            match runtime.block_on(super::handle(&arguments)) {
                Ok(text) => {
                    println!("{text}");
                    Exit::Ok
                }
                Err(err) => {
                    eprintln!("{}", err.message);
                    err.exit
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::websearch::run;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn empty_args_show_usage() {
        assert!(parse(&[]).is_err());
        assert!(parse(&args(&["--help"])).is_err());
        assert_eq!(parse(&args(&["сервисом"])).unwrap(), Intent::Ask(Options {
            queries: vec!["сервисом".to_string()],
            engines: None,
            limit: 8,
        }));
    }

    #[test]
    fn positional_is_multiword_and_default_limit_is_max_results() {
        let parsed = parse(&args(&["купить ржаную муку", "с доставкой"])).unwrap();
        let Intent::Ask(opts) = parsed else { panic!("not ask") };
        assert_eq!(opts.queries, vec!["купить ржаную муку с доставкой"]);
        assert_eq!(opts.limit, 8);
        assert!(opts.engines.is_none());
    }

    #[test]
    fn positional_query_goes_first_when_queries_flag_present() {
        // Боевой кейс: модель передавала и позиционный запрос, и --queries —
        // позиционный терялся. Теперь он первый элемент списка.
        let parsed = parse(&args(&["главный запрос", "--queries", "второй, третий"])).unwrap();
        let Intent::Ask(opts) = parsed else { panic!("not ask") };
        assert_eq!(opts.queries, vec!["главный запрос", "второй", "третий"]);
        // Ограничение 5 запросов соблюдается и на объединённом списке.
        let parsed = parse(&args(&[
            "a", "--queries", "b,c,d,e,f",
        ]))
        .unwrap();
        let Intent::Ask(opts) = parsed else { panic!("not ask") };
        assert_eq!(opts.queries, vec!["a", "b", "c", "d", "e"]);
    }

    #[test]
    fn queries_engines_limit_are_validated() {
        let parsed = parse(&args(&[
            "--queries", "один, два", "--engines", "EXA ,duckduckgo", "--limit", "20",
        ]))
        .unwrap();
        let Intent::Ask(opts) = parsed else { panic!("not ask") };
        assert_eq!(opts.queries, vec!["один", "два"]);
        assert_eq!(opts.engines, Some(vec!["exa".to_string(), "duckduckgo".to_string()]));
        assert_eq!(opts.limit, 20);

        assert_eq!(
            parse(&args(&["--engines", "yahoo"])).unwrap_err(),
            format!(
                "неизвестные движки: yahoo. Доступны: {}",
                crate::tools::websearch::engines::ENGINE_NAMES.join(", ")
            )
        );
        assert!(parse(&args(&["--queries", "1,2,3,4,5,6"])).is_err());
        assert!(parse(&args(&["--limit", "2"])).is_err());
        assert!(parse(&args(&["--limit", "двадцать"])).is_err());
        assert!(parse(&args(&["--queries"])).is_err());
    }

    #[test]
    fn mcp_and_stats_intents() {
        assert_eq!(parse(&args(&["mcp"])).unwrap(), Intent::Mcp);
        assert_eq!(parse(&args(&["--stats"])).unwrap(), Intent::Stats);
    }

    #[test]
    fn run_level_skips_program_name_and_tool_name() {
        // main.rs передаёт tool.run(полный argv): argv[0] — бинарь,
        // argv[1] — 'websearch'. Парсеру достаётся argv[2..] — иначе
        // 'websearch' стал бы позиционным запросом (регрессия codemap/cli.rs:424).
        // Сеть не трогаем: --help → usage, битые движки → мгновенный Input.
        assert_eq!(
            run(vec!["agent-tools.exe".into(), "websearch".into(), "--help".into()]),
            Exit::Ok
        );
        assert_eq!(
            run(vec![
                "agent-tools.exe".into(),
                "websearch".into(),
                "--engines".into(),
                "yahoo".into(),
            ]),
            Exit::Input
        );
    }
}
