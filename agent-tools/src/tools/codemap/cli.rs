//! Разбор аргументов и операции инструмента `codemap`.
//!
//! ## Почему операции живут здесь, а не в `main.rs`
//!
//! Один и тот же набор действий («построить индекс», «показать карту»,
//! «найти символ») требуется и командной строке, и MCP-серверу. Если бы
//! операции остались в ветке CLI, MCP-режим вынужден был бы разбирать вывод
//! командной строки обратно — источник правды в двух местах, а значит
//! расхождение по тексту. Здесь операции возвращают `String`, и оба режима
//! лишь доставляют её: CLI печатает в stdout, MCP — в поле `content`.
//!
//! ## Бюджет по умолчанию
//!
//! 2048 токенов. Это значение из измерений Aider на реальных репозиториях:
//! карта такого размера помещается в один запрос вместе с задачей агента и не
//! вытесняет её из контекста. Увеличение бюджета — сознательный выбор
//! вызывающего, а не поведение инструмента по умолчанию.

use clap::{Args, Parser, Subcommand};
use std::path::{Path, PathBuf};

use crate::exit::Exit;

use super::body;
use super::graph;
use super::lookup;
use super::mcp;
use super::rank;
use super::render;
use super::scan;
use super::search;
use super::skeleton;
use super::store;

/// Бюджет токенов карты по умолчанию.
pub const DEFAULT_TOKENS: u32 = 2048;

/// Разбор одной сабкоманды: имя программы + имя инструмента + аргументы.
#[derive(Debug, Parser)]
#[command(name = "agent-tools", bin_name = "agent-tools")]
pub struct CodemapParser {
    #[command(subcommand)]
    pub command: CodemapSub,
}

/// Подкоманды `codemap`.
#[derive(Debug, Subcommand)]
pub enum CodemapSub {
    /// Проиндексировать проект и сохранить карту в `<root>/.codemap`.
    #[command(about = ABOUT)]
    Index(IndexArgs),
    /// Показать карту проекта, отобранную под бюджет токенов.
    #[command(about = ABOUT)]
    Map(MapArgs),
    /// Найти символы и текст по запросу.
    #[command(about = ABOUT)]
    Search(SearchArgs),
    /// Показать, кто вызывает символ.
    #[command(about = ABOUT)]
    Callers(CallersArgs),
    /// Показать скелет одного файла: импорты и сигнатуры с номерами строк.
    #[command(about = ABOUT)]
    Skeleton(SkeletonArgs),
    /// Показать тело одного символа файла.
    #[command(about = ABOUT)]
    Body(BodyArgs),
    /// Работать как MCP-сервер (JSON-RPC через stdin/stdout).
    #[command(about = ABOUT)]
    Mcp(McpArgs),
}

const ABOUT: &str = "Построить карту репозитория: разобрать AST, ранжировать символы \
графом и отдать сжатую карту под бюджет токенов. Работает с любым репозиторием: \
Rust, TS/JS через oxc, Python; остальные расширения — как плоский текст.";

/// Общий для map/search/callers (и честно отвергаемый index'ом) флаг поля
/// `no_cache`: смысл у него один — «чужое дерево» (крейт из cargo registry,
/// соседний репозиторий), у которого нет своего каталога `.codemap` и которому
/// его создавать нельзя. С кешем (`refresh`) связывать нечего: полная
/// пересборка при каждом вызове, зато ноль операций записи на диск.
/// (Поля объявляются в каждой структуре: макрос в позиции полей не работает.)

#[derive(Debug, Args, Clone, Default)]
pub struct IndexArgs {
    /// Корень проекта (по умолчанию текущий каталог).
    #[arg(long, value_name = "PATH")]
    pub path: Option<PathBuf>,
    /// Вычислить ответ в памяти: не читать кеш и ничего не записывать на диск
    /// (для чужих деревьев: крейтов из cargo registry, соседних репозиториев).
    ///
    /// Для `index` флаг валиден только чтобы получить отказ с объяснением:
    /// операция существует ради записи кеша на диск.
    #[arg(long)]
    pub no_cache: bool,
}

#[derive(Debug, Args, Clone, Default)]
pub struct MapArgs {
    /// Корень проекта (по умолчанию текущий каталог).
    #[arg(long, value_name = "PATH")]
    pub path: Option<PathBuf>,
    /// Бюджет токенов карты.
    #[arg(long, value_name = "N", default_value_t = DEFAULT_TOKENS)]
    pub tokens: u32,
    /// Файлы, ранжируемые выше остальных (как chat-файлы у Aider).
    #[arg(long, value_name = "PATH")]
    pub file: Vec<String>,
    /// Показать в stderr разбор концентрации ранга по файлам.
    ///
    /// Отдельный флаг, а не всегдашняя диагностика: карта — это ответ агенту,
    /// и постоянный шум в stderr мешал бы разбирать вызов инструмента. В
    /// stdout флаг ничего не пишет — это канал карты (и JSON-RPC в MCP-режиме).
    #[arg(long)]
    pub explain: bool,
    /// Вычислить ответ в памяти: не читать кеш и ничего не записывать на диск
    /// (для чужих деревьев: крейтов из cargo registry, соседних репозиториев).
    #[arg(long)]
    pub no_cache: bool,
}

#[derive(Debug, Args, Clone, Default)]
pub struct SearchArgs {
    /// Корень проекта (по умолчанию текущий каталог).
    #[arg(long, value_name = "PATH")]
    pub path: Option<PathBuf>,
    /// Что ищем.
    #[arg(value_name = "QUERY")]
    pub query: String,
    /// Сколько результатов вернуть.
    #[arg(long, value_name = "N", default_value_t = 10)]
    pub limit: usize,
    /// Вычислить ответ в памяти: не читать кеш и ничего не записывать на диск
    /// (для чужих деревьев: крейтов из cargo registry, соседних репозиториев).
    #[arg(long)]
    pub no_cache: bool,
}

#[derive(Debug, Args, Clone, Default)]
pub struct CallersArgs {
    /// Корень проекта (по умолчанию текущий каталог).
    #[arg(long, value_name = "PATH")]
    pub path: Option<PathBuf>,
    /// Имя символа.
    #[arg(value_name = "SYMBOL")]
    pub symbol: String,
    /// Вычислить ответ в памяти: не читать кеш и ничего не записывать на диск
    /// (для чужих деревьев: крейтов из cargo registry, соседних репозиториев).
    #[arg(long)]
    pub no_cache: bool,
}

/// Аргументы операции «скелет файла».
///
/// Индекс не нужен: операция разбирает ровно один файл и ничего не пишет на
/// диск. Это делает её пригодной для чужих деревьев без флага `--no-cache` —
/// кеша там всё равно нет и создавать его нельзя.
#[derive(Debug, Args, Clone, Default)]
pub struct SkeletonArgs {
    /// Корень проекта (по умолчанию текущий каталог).
    #[arg(long, value_name = "PATH")]
    pub path: Option<PathBuf>,
    /// Файл, скелет которого показать (относительно корня).
    #[arg(value_name = "FILE")]
    pub file: String,
}

/// Аргументы операции «тело символа».
#[derive(Debug, Args, Clone, Default)]
pub struct BodyArgs {
    /// Корень проекта (по умолчанию текущий каталог).
    #[arg(long, value_name = "PATH")]
    pub path: Option<PathBuf>,
    /// Файл, в котором ищем символ (относительно корня).
    #[arg(value_name = "FILE")]
    pub file: String,
    /// Имя символа.
    #[arg(value_name = "SYMBOL")]
    pub symbol: String,
}

#[derive(Debug, Args, Clone, Default)]
pub struct McpArgs {
    /// Корень проекта (по умолчанию текущий каталог).
    #[arg(long, value_name = "PATH")]
    pub path: Option<PathBuf>,
    /// Бюджет токенов карты по умолчанию для вызовов `codemap_map`.
    #[arg(long, value_name = "N", default_value_t = DEFAULT_TOKENS)]
    pub tokens: u32,
}

/// Разобранные и приведённые к общему виду настройки операции.
///
/// MCP-режим держит один такой объект на весь сеанс и переиспользует его для
/// всех запросов — корень проекта не может «уехать» между вызовами.
#[derive(Debug, Clone)]
pub struct Cli {
    /// Корень проекта.
    pub root: PathBuf,
    /// Бюджет токенов карты по умолчанию.
    pub tokens: u32,
    /// Не использовать дисковый кеш: считать всё в памяти (см. [`store::compute_only`]).
    ///
    /// MCP-режим флага не имеет: корень MCP-сервера — «свой» проект, кеш там
    /// уместен всегда.
    pub no_cache: bool,
}

impl Cli {
    /// Настройки для тестов: пустой корень, операции обязаны честно сообщить
    /// об отсутствии индекса, а не выдумать результат.
    pub fn for_tests() -> Cli {
        Cli {
            root: PathBuf::from("."),
            tokens: DEFAULT_TOKENS,
            no_cache: false,
        }
    }

    /// Корень проекта, каким его увидит пользователь.
    fn display_root(&self) -> String {
        self.root.display().to_string()
    }

    /// Построить индекс и сохранить его на диск.
    pub fn index(&self, _path: Option<&str>) -> Result<String, String> {
        let (index, stats) = store::build(&self.root, scan::walk(&self.root)?)?;
        store::save(&self.root, &index)?;
        // `.codemap` обязан не появляться в `git status` вызывающего: это
        // артефакт инструмента, а не содержимое проекта.
        if let Err(error) = store::exclude_from_git(&self.root) {
            log::warn!("codemap: не удалось скрыть .codemap из git: {error}");
        }
        Ok(format!(
            "Проиндексировано: {} файлов ({} разобрано, {} из кеша, {} не удалось, \
             {} двоичных пропущено, {} плоских), {} тегов, {} токенов\nИндекс: {}",
            stats.scanned,
            stats.parsed,
            stats.reused,
            stats.failed,
            stats.binary,
            index.flat_count(),
            index.tags.len(),
            index.total_tokens(),
            store::index_path(&self.root).display()
        ))
    }

    /// Показать карту проекта.
    ///
    /// Индекс строится, если его нет: заставлять агента сначала вызывать
    /// `index`, чтобы затем прочитать карту, — это два вызова там, где хватает
    /// одного. Явный `index` нужен, когда агент знает, что репозиторий менялся
    /// часто, и хочет контролировать момент перестроения.
    ///
    /// `focus` — файлы, которые ранжируются выше остальных. Параметр не
    /// декоративный: без него [`graph::build`] работает без персонализации, и
    /// карта всегда отвечала на вопрос «что вообще есть в проекте», а не «что
    /// нужно именно здесь».
    pub fn map(&self, tokens: u32, focus: &[String], explain: bool) -> Result<String, String> {
        let index = self.load_or_build()?;
        let graph = graph::build(&index.tags, focus, &[]);
        let ranked = rank::rank_definitions(&graph, &index.tags);
        log::info!(
            "codemap: тегов {} | рёбер {} | ранжировано {} | файлов {} | фокус {}",
            index.tags.len(),
            graph.edges.len(),
            ranked.len(),
            index.files.len(),
            focus.len()
        );
        let (text, used) = render::render_map(&self.root, &index, &ranked, tokens);
        report_concentration(&ranked, explain);
        if text.is_empty() {
            return Err(format!(
                "Карта пуста: в {} не найдено ни одного определения. \
                 Возможно, корень указан неверно.",
                self.display_root()
            ));
        }
        log::info!("codemap: карта {} токенов при бюджете {}", used, tokens);
        Ok(text)
    }

    /// Найти символы и текст.
    pub fn search(&self, query: &str, limit: usize) -> Result<String, String> {
        let index = self.load_or_build()?;
        let hits = search::search(&index, query, limit);
        if hits.is_empty() {
            return Ok(format!("Ничего не найдено по запросу «{query}»."));
        }
        let mut out = format!("Найдено по запросу «{query}»:\n");
        for hit in hits {
            out.push_str(&format!("  {}:{}  [{}]  {}\n", hit.rel_path, hit.line, hit.kind, hit.snippet));
        }
        Ok(out)
    }

    /// Показать обратные ссылки на символ.
    pub fn callers(&self, symbol: &str) -> Result<String, String> {
        let index = self.load_or_build()?;
        let callers = search::callers(&index, symbol);
        if callers.is_empty() {
            return Ok(format!("Символ «{symbol}» нигде не используется."));
        }
        let mut out = format!("Вызовы «{symbol}»:\n");
        for caller in callers {
            out.push_str(&format!("  {}:{}\n", caller.rel_path, caller.line));
        }
        Ok(out)
    }

    /// Показать скелет одного файла: импорты и сигнатуры с номерами строк.
    ///
    /// Индекс не строится и на диск ничего не пишется: операция разбирает
    /// ровно один файл. Для агента это самый дешёвый способ понять, что в
    /// файле есть, не тратя контекст на тела функций.
    pub fn skeleton(&self, file: &str) -> Result<String, lookup::LookupError> {
        let view = lookup::FileView::open(&self.root, file)?;
        Ok(skeleton::skeleton(&view))
    }

    /// Показать тело одного символа файла.
    pub fn body(&self, file: &str, symbol: &str) -> Result<String, lookup::LookupError> {
        let view = lookup::FileView::open(&self.root, file)?;
        body::body(&view, symbol)
    }

    /// Взять индекс, соответствующий текущему состоянию каталога.
    ///
    /// Индекс достраивается, если его нет, и **пересобирается, если он разошёлся с
    /// диском**: удалённый файл обязан исчезнуть из карты, а добавленный — появиться
    /// без ручного `codemap index`. Проверка стоит одного обхода метаданных, поэтому
    /// держать карту нарочно устаревшей было бы уже не экономией, а просто
    /// неправдой.
    ///
    /// В режиме `--no-cache` диск вообще не участвует в хранении: полный
    /// scan+parse в памяти при каждом вызове — плата за запрет создавать что-либо
    /// в чужом корне ([`store::compute_only`]).
    fn load_or_build(&self) -> Result<super::model::Index, String> {
        match self.no_cache {
            true => store::compute_only(&self.root),
            false => store::refresh(&self.root),
        }
    }
}

/// Точка входа инструмента: разбор аргументов и выбор режима.
pub fn run(argv: Vec<String>) -> Exit {
    let rest: &[String] = argv.get(2..).unwrap_or(&[]);
    // clap требует, чтобы argv[0] был именем программы: без него первый
    // аргумент (`index`, `map`, ...) принимается за имя программы и все
    // последующие флаги объявляются лишними. Тест-обёртка ниже повторяет
    // ровно это построение — иначе тесты проверяли бы не тот разбор, который
    // really выполняется в main.
    let mut parser_argv: Vec<String> = vec!["agent-tools".to_string()];
    parser_argv.extend_from_slice(rest);
    let parsed = match CodemapParser::try_parse_from(&parser_argv) {
        Ok(parsed) => parsed,
        Err(error) => {
            // clap печатает справку сам; дублировать её не нужно.
            eprint!("{error}");
            return Exit::Input;
        }
    };

    match parsed.command {
        CodemapSub::Index(args) => {
            if args.no_cache {
                // Честное объяснение вместо generic-отказа clap: смысл операции
                // прямо противоположен флагу, и подменять его молча нельзя.
                eprintln!(
                    "codemap index: флаг --no-cache несовместим: операция существует, \
                     чтобы построить и сохранить кеш на диске. Для карты чужого дерева \
                     без записей используйте map/search/callers с --no-cache."
                );
                return Exit::Input;
            }
            let cli = Cli { root: root_of(args.path), tokens: DEFAULT_TOKENS, no_cache: false };
            finish(cli.index(None))
        }
        CodemapSub::Map(args) => {
            let cli = Cli { root: root_of(args.path), tokens: args.tokens, no_cache: args.no_cache };
            finish(cli.map(args.tokens, &args.file, args.explain))
        }
        CodemapSub::Search(args) => {
            let cli = Cli { root: root_of(args.path), tokens: DEFAULT_TOKENS, no_cache: args.no_cache };
            finish(cli.search(&args.query, args.limit))
        }
        CodemapSub::Callers(args) => {
            let cli = Cli { root: root_of(args.path), tokens: DEFAULT_TOKENS, no_cache: args.no_cache };
            finish(cli.callers(&args.symbol))
        }
        CodemapSub::Skeleton(args) => {
            let cli = Cli { root: root_of(args.path), tokens: DEFAULT_TOKENS, no_cache: true };
            finish_lookup(cli.skeleton(&args.file))
        }
        CodemapSub::Body(args) => {
            let cli = Cli { root: root_of(args.path), tokens: DEFAULT_TOKENS, no_cache: true };
            finish_lookup(cli.body(&args.file, &args.symbol))
        }
        CodemapSub::Mcp(args) => {
            let cli = Cli { root: root_of(args.path), tokens: args.tokens, no_cache: false };
            mcp::serve(cli)
        }
    }
}

/// Разобрать концентрацию ранга по файлам и показать её там, где это уместно.
///
/// Доля топ-файлов в сумме рангов — прямой признак того, что PageRank схлопнул
/// граф в одну вершину: на king_orch_3 один файл держал 56% суммы, и карта
/// показывала только его. Поэтому диагностика всегда идёт в лог на уровне
/// `info` (то есть видна при `AGENT_TOOLS_LOG=debug`), а по требованию
/// `--explain` дублируется прямо в stderr.
///
/// В stdout она не попадает **никогда**: stdout карты — это результат, который
/// разбирает вызывающий, а в MCP-режиме это JSON-RPC.
fn report_concentration(ranked: &[rank::RankedDefinition], explain: bool) {
    let mut share: Vec<(&str, f64)> = ranked
        .iter()
        .fold(std::collections::HashMap::new(), |mut acc, d| {
            *acc.entry(d.rel_path.as_str()).or_insert(0.0) += d.rank;
            acc
        })
        .into_iter()
        .collect();
    share.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let total: f64 = share.iter().map(|(_, v)| *v).sum();
    let top: Vec<String> = share
        .iter()
        .take(5)
        .map(|(p, v)| format!("{p}={:.1}%", 100.0 * v / total.max(1e-9)))
        .collect();
    let line = format!("концент ранга — {}", top.join(", "));
    log::info!("codemap: {line}");
    if explain {
        eprintln!("codemap: {line}");
    }
}

/// Напечатать результат операции и вернуть код процесса.
fn finish(result: Result<String, String>) -> Exit {
    match result {
        Ok(text) => {
            println!("{text}");
            Exit::Ok
        }
        Err(reason) => {
            // В CLI это единственный канал: ошибка — тоже результат работы.
            eprintln!("codemap: {reason}");
            Exit::Internal
        }
    }
}

/// Код возврата операций разведки (`skeleton`, `body`).
///
/// Два класса отказа разведены намеренно: «файла нет» и «символа нет» —
/// ошибка запроса (агент указал не то, код 2), а «файл есть, но не читается
/// или не разобран» — отказ исполнения (код 4).
fn finish_lookup(result: Result<String, lookup::LookupError>) -> Exit {
    match result {
        Ok(text) => {
            println!("{text}");
            Exit::Ok
        }
        Err(lookup::LookupError::Input(reason)) => {
            eprintln!("codemap: {reason}");
            Exit::Input
        }
        Err(lookup::LookupError::Failed(reason)) => {
            eprintln!("codemap: {reason}");
            Exit::Internal
        }
    }
}

/// Корневой путь: явный путь либо текущий каталог.
fn root_of(path: Option<PathBuf>) -> PathBuf {
    path.unwrap_or_else(|| PathBuf::from("."))
}

/// Проверить, что корень существует и это каталог.
///
/// Вынесено отдельно, чтобы проверка была одна на CLI и MCP: сейчас её нет
/// ни там, ни там, и ошибка «нет такого каталога» приходит изнутри обхода.
pub fn validate_root(root: &Path) -> Result<(), String> {
    if !root.exists() {
        return Err(format!("Путь не существует: {}", root.display()));
    }
    if !root.is_dir() {
        return Err(format!("Путь не является каталогом: {}", root.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(items: &[&str]) -> CodemapSub {
        // Имя программы + подкоманда: ровно то, что делает `run`.
        let mut argv: Vec<String> = vec!["agent-tools".to_string()];
        argv.extend(items.iter().map(|item| item.to_string()));
        CodemapParser::try_parse_from(&argv).expect("разбор").command
    }

    /// Регрессия: `run` отбрасывал первые два argv и передавал clap остаток,
    /// из-за чего `codemap index --path X` падал с «unexpected argument» —
    /// а тест этого не ловил, потому что строил argv иначе.
    #[test]
    fn real_argv_layout_is_accepted() {
        let mut argv: Vec<String> = vec!["agent-tools".into(), "codemap".into()];
        argv.extend(["index".into(), "--path".into(), ".".into()]);
        let rest: &[String] = argv.get(2..).unwrap();

        let mut parser_argv: Vec<String> = vec!["agent-tools".to_string()];
        parser_argv.extend_from_slice(rest);
        assert!(
            CodemapParser::try_parse_from(&parser_argv).is_ok(),
            "раскладка реального argv обязана разбираться"
        );
    }

    #[test]
    fn parses_index_with_explicit_root() {
        match parse(&["index", "--path", "D:/repo"]) {
            CodemapSub::Index(args) => assert_eq!(args.path.as_deref(), Some(Path::new("D:/repo"))),
            other => panic!("ожидался index, получен {other:?}"),
        }
    }

    #[test]
    fn parses_map_with_token_budget() {
        match parse(&["map", "--tokens", "512"]) {
            CodemapSub::Map(args) => assert_eq!(args.tokens, 512),
            other => panic!("ожидался map, получен {other:?}"),
        }
    }

    #[test]
    fn map_defaults_to_measured_budget() {
        match parse(&["map"]) {
            CodemapSub::Map(args) => assert_eq!(args.tokens, 2048),
            other => panic!("ожидался map, получен {other:?}"),
        }
    }

    #[test]
    fn repeated_file_flags_accumulate() {
        match parse(&["map", "--file", "a.rs", "--file", "b.rs"]) {
            CodemapSub::Map(args) => assert_eq!(args.file, vec!["a.rs", "b.rs"]),
            other => panic!("ожидался map, получен {other:?}"),
        }
    }

    /// Без явного `--explain` диагностика концентрации в stderr не пишется:
    /// карта и её разбор — разные вещи.
    #[test]
    fn explain_is_off_by_default_and_parses_when_asked() {
        match parse(&["map"]) {
            CodemapSub::Map(args) => assert!(!args.explain),
            other => panic!("ожидался map, получен {other:?}"),
        }
        match parse(&["map", "--explain"]) {
            CodemapSub::Map(args) => assert!(args.explain),
            other => panic!("ожидался map, получен {other:?}"),
        }
    }

    /// Фокус обязан менять карту — иначе `--file` объявлен, а не работает.
/// Проверяется сквозным путём `Cli::map`, а не вызовом `graph::build`: чинить
/// пришлось именно проводку (флаг разбирался и никуда не передавался), и тест
/// на внутренний вызов такой поломки не увидел бы.
///
/// Фикстура — цепочка `f0 ← f1 ← … ← f9`, где каждая следующая ссылается на
/// предыдущую. Без фокуса ранг убывает от `f0` к `f9`; фокус на `f5` обязан
/// поднять его выше прежнего места. Сравниваются позиции одного и того же файла,
/// а не «какой файл первый»: порядок первого зависит от того, насколько сильно
/// разошлись ранги, и проверять его здесь нечего.
#[test]
fn gate12_focus_flag_moves_the_focused_file_up_the_map() {
    let root = std::env::temp_dir().join("codemap_focus_map");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("временный каталог");
    for i in 0..10usize {
        let mut source = format!(
            "pub fn sym{i}_first() -> u32 {{\n    {i}\n}}\n\npub fn sym{i}_second() -> u32 {{\n    {}\n}}\n",
            i * 2
        );
        if i > 0 {
            source.push_str(&format!(
                "\npub fn sym{i}_third() -> u32 {{\n    sym{}_first()\n}}\n",
                i - 1
            ));
        }
        std::fs::write(root.join(format!("f{i}.rs")), source).expect("исходник");
    }

    let cli = Cli {
        root: root.clone(),
        tokens: 2_048,
        no_cache: false,
    };
    let plain = cli.map(2_048, &[], false).expect("карта без фокуса");
    let focused = cli
        .map(2_048, &["f5.rs".to_string()], false)
        .expect("карта с фокусом");

    let at = |text: &str, name: &str| {
        text.find(&format!("{name}:"))
            .unwrap_or_else(|| panic!("файла {name} нет в карте:\n{text}"))
    };
    assert_ne!(plain, focused, "--file обязан менять карту, а не быть прочитанным флагом");
    assert!(
        at(&focused, "f5.rs") < at(&plain, "f5.rs"),
        "фокусный файл обязан подняться в карте:\n{focused}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

    #[test]
    fn search_requires_query_positionally() {
        match parse(&["search", "handle_payment", "--limit", "3"]) {
            CodemapSub::Search(args) => {
                assert_eq!(args.query, "handle_payment");
                assert_eq!(args.limit, 3);
            }
            other => panic!("ожидался search, получен {other:?}"),
        }
    }

    #[test]
    fn mcp_subcommand_parses() {
        assert!(matches!(parse(&["mcp"]), CodemapSub::Mcp(_)));
    }

    /// Флаг `--no-cache` обязан разбираться на всех операциях чтения карты:
    /// это единое имя для «чужого дерева» на четырёх командах сразу.
    #[test]
    fn no_cache_flag_parses_on_every_read_command() {
        match parse(&["map", "--no-cache"]) {
            CodemapSub::Map(args) => assert!(args.no_cache),
            other => panic!("ожидался map, получен {other:?}"),
        }
        match parse(&["search", "handle_payment", "--no-cache"]) {
            CodemapSub::Search(args) => assert!(args.no_cache),
            other => panic!("ожидался search, получен {other:?}"),
        }
        match parse(&["callers", "handle_payment", "--no-cache"]) {
            CodemapSub::Callers(args) => assert!(args.no_cache),
            other => panic!("ожидался callers, получен {other:?}"),
        }
        match parse(&["index", "--no-cache"]) {
            CodemapSub::Index(args) => assert!(args.no_cache),
            other => panic!("ожидался index, получен {other:?}"),
        }
    }

    /// Без флага поведение прежнее: поле по умолчанию выключено.
    #[test]
    fn no_cache_is_off_by_default() {
        match parse(&["map"]) {
            CodemapSub::Map(args) => assert!(!args.no_cache),
            other => panic!("ожидался map, получен {other:?}"),
        }
    }

    /// `index` существует ради записи кеша на диск: флаг с ним несовместим и
    /// обязан быть отвергнут кодом ввода с объяснением, а не брошен clap'ом
    /// generic-текстом про «неизвестный аргумент».
    #[test]
    fn index_with_no_cache_is_rejected_as_input_error() {
        let exit = run(vec![
            "agent-tools".into(),
            "codemap".into(),
            "index".into(),
            "--no-cache".into(),
        ]);
        assert_eq!(exit, Exit::Input);
    }

    /// Карта чужого дерева в no-cache режиме отвечает, и на диске ничего
    /// не появляется: ни каталога кеша, ни правок exclude в `.git`.
    ///
    /// Исходник обязан содержать и определение, и ссылку: карта ранжирует
    /// определения по связям, и файл без единой ссылки картой не показывается
    /// вовсе (см. [`rank::rank_definitions`]) — это независимое от no-cache
    /// поведение, и тест не должен путать его с отказом флага.
    #[test]
    fn map_no_cache_answers_and_writes_nothing() {
        let root = std::env::temp_dir().join("codemap_cli_no_cache");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".git")).expect("имитация чужого репозитория");
        std::fs::create_dir_all(root.join("src")).expect("каталог");
        std::fs::write(
            root.join("src/lib.rs"),
            "pub fn plain_math() -> u32 { plain_inner() }\n\
             pub fn plain_inner() -> u32 { 3 }\n",
        )
        .expect("исходник");

        let cli = Cli {
            root: root.clone(),
            tokens: 1_024,
            no_cache: true,
        };
        let map = cli.map(1_024, &[], false).expect("карта в памяти");
        assert!(
            map.contains("plain_math"),
            "символ чужого дерева обязан попасть в карту:\n{map}"
        );
        assert!(!store::index_dir(&root).exists(), "каталог кеша запрещён");
        assert!(
            !root.join(".git/info/exclude").exists(),
            "exclude чужого корня запрещён"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unknown_subcommand_is_rejected() {
        let argv: Vec<String> = ["codemap", "nope"].iter().map(|s| s.to_string()).collect();
        assert!(CodemapParser::try_parse_from(&argv).is_err());
    }

    #[test]
    fn missing_query_is_rejected_rather_than_defaulted() {
        let argv: Vec<String> = ["codemap", "search"].iter().map(|s| s.to_string()).collect();
        assert!(
            CodemapParser::try_parse_from(&argv).is_err(),
            "пустой запрос обязан быть ошибкой ввода, а не пустым поиском"
        );
    }

    #[test]
    fn root_defaults_to_current_directory() {
        assert_eq!(root_of(None), PathBuf::from("."));
    }

    #[test]
    fn validate_root_accepts_existing_directory() {
        assert!(validate_root(Path::new(".")).is_ok());
    }

    #[test]
    fn validate_root_reports_missing_and_non_directory() {
        assert!(validate_root(Path::new("definitely/missing/path")).is_err());
        assert!(validate_root(Path::new("Cargo.toml")).is_err());
    }

    #[test]
    fn for_tests_has_known_budget() {
        assert_eq!(Cli::for_tests().tokens, DEFAULT_TOKENS);
    }
}