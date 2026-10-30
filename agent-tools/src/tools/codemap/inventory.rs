//! Сжатый инвентарь проекта: каталоги, их вес и сводка по игнорируемому.
//!
//! ## Зачем он нужен отдельно от кода
//!
//! Карта под бюджет 2048 токенов физически не может показать 361 файл: на
//! каждый файл остаётся несколько строк. Инвентарь отвечает на вопрос, который
//! список определений не отвечает вовсе — «из чего вообще состоит проект» — и
//! стоит копейки: строка каталога в два-три раза дешевле блока кода.
//!
//! ## Порядок каталогов — по архитектурному весу, а не по размеру
//!
//! Каталог сортируется по сумме рангов своих определений. Так каталог с
//! семидесятью тысячами строк фикстур (`tasks_for_test_llm/`) не вытесняет из
//! первых строк каталог, который реально держит архитектуру. Токены при этом
//! остаются в строке как характеристика размера — просто не как мера важности.
//!
//! ## Почему имена файлов выбираются по рангу, а не по размеру
//!
//! Список имён внутри каталога — это ответ на вопрос «что здесь главное».
//! Файл в 6 тысяч токенов, который никто не зовёт, главным не является; файл в
//! 1 тысячу токенов, который зовут отовсюду, — является. Порядок по рангу даёт
//! карте ту же выгоду, что и раздел кода, и обходится теми же данными, которые
//! [`super::render`] всё равно уже посчитал.

use std::cmp::Ordering;
use std::collections::HashMap;

use super::lang::Lang;
use super::model::Index;
use super::rank::RankedDefinition;
use super::tags::TagKind;
use super::tokens;

/// Доля бюджета карты, которую инвентарь вправе занимать.
///
/// Именно доля, а не фиксированное число токенов: при бюджете 32768 тот же
/// инвентарь должен вырасти вместе с картой, иначе он перестанет быть обзором
/// и останется подписью к трём каталогам.
pub const SHARE: f64 = 0.12;

/// Ниже этого бюджета секция не печатается вовсе.
///
/// Число не про «красиво»: заголовок и одна строка стоят дороже, чем половина
/// крошечного бюджета, и такая «секция» отнимала бы у кода место, не сказав
/// ничего. Пустая строка — честный ответ «инвентарь не помещается».
pub const MIN_BUDGET: u32 = 40;

/// Сколько имён файлов перечисляет одна строка каталога.
const NAMES_PER_DIR: usize = 5;

/// Минимальный полезный размер одной строки каталога.
///
/// Число отражает худший случай — короткий путь и одно имя, — и служит нижней
/// границей того, ради чего секция вообще печатается.
const MIN_DIRECTORY_TOKENS: u32 = 16;

/// Заголовок секции.
const HEADER: &str = "## 📁 Структура проекта\n";

/// Заголовок секции документации. С пустой строкой перед собой: секции карты
/// отделяются друг от друга, как отделяется инвентарь от шапки.
const DOCS_HEADER: &str = "\n## 📚 Документация\n";

/// Доля инвентарного бюджета, отведённая секции документации.
///
/// Доки откусывают от доли каталогов, а не от доли кода: оглавление — часть
/// обзора «из чего состоит проект». Если markdown-файлов в проекте нет, доля
/// целиком возвращается каталогам.
pub const DOCS_SHARE: f64 = 0.35;

/// Сколько заголовков одного документа показывается в строках оглавления.
const HEADINGS_PER_DOC: usize = 6;

/// Сумма рангов определений по каждому файлу.
///
/// Путь → вес. Файлы без ранжированных определений в карту не попадут, но в
/// инвентаре они всё равно нужны, поэтому отсутствие веса — это ноль, а не
/// пропуск.
pub fn file_weights(ranked: &[RankedDefinition]) -> HashMap<&str, f64> {
    let mut weights: HashMap<&str, f64> = HashMap::new();
    for definition in ranked {
        *weights.entry(definition.rel_path.as_str()).or_insert(0.0) += definition.rank;
    }
    weights
}

/// Построить секцию инвентаря либо пустую строку, если она не помещается.
pub fn section(index: &Index, weights: &HashMap<&str, f64>, budget: u32) -> String {
    if budget < MIN_BUDGET {
        return String::new();
    }

    let directories = directories(index, weights);

    // Сводка по игнорируемому резервируется ДО каталогов: она отвечает на
    // вопрос «что инструмент не смотрел», и молча исчезнуть при нехватке
    // места она не может — иначе карта утверждала бы, что проект устроен иначе.
    let ignored_line = index
        .ignored_summary()
        .is_empty()
        .then(String::new)
        .unwrap_or_else(|| format!("(игнорируется: {})\n", index.ignored_summary()));
    let ignored_cost = tokens::estimate(&ignored_line);
    let room = budget.saturating_sub(ignored_cost);

    // Секция без единого каталога — не секция, а пустая рамка: агент видит
    // заголовок, строку «ещё N каталогов» и ноль содержания. Дешевле честно не
    // печатать её вовсе: пустая строка означает «инвентарь не помещается».
    if directories.is_empty() || room < tokens::estimate(HEADER) + MIN_DIRECTORY_TOKENS {
        return String::new();
    }

    let mut kept: Vec<String> = Vec::new();
    let mut spent = 0u32;
    let mut dropped = 0usize;
    for directory in &directories {
        let line = directory.line();
        let cost = tokens::estimate(&line);
        if spent + cost <= room {
            spent += cost;
            kept.push(line);
        } else {
            dropped += 1;
        }
    }

    // Обрезанный список молча выглядел бы полным: агент решил бы, что в
    // проекте ровно столько каталогов, сколько напечатано. Если на строку с
    // признаком обрезки места не хватило, снимается последний каталог —
    // честная неполнота лучше незаметной.
    let mut trailer = String::new();
    if dropped > 0 {
        trailer = format!("… ещё {dropped} каталогов\n");
        while spent + tokens::estimate(&trailer) > room {
            match kept.pop() {
                Some(line) => spent -= tokens::estimate(&line),
                None => break,
            }
        }
    }

    let mut out = String::from(HEADER);
    for line in &kept {
        out.push_str(line);
    }
    out.push_str(&trailer);
    out.push_str(&ignored_line);
    out
}

/// Агрегат по одному каталогу.
struct Directory {
    /// Путь каталога относительно корня; `.` для файлов в корне.
    path: String,
    /// Сколько файлов индексировано внутри.
    files: u32,
    /// Сумма токенов по этим файлам.
    tokens: u64,
    /// Сумма рангов определений по этим файлам.
    weight: f64,
    /// Файлы каталога как `(ранг, токены, имя)`, уже отсортированные.
    ranked: Vec<(f64, u32, String)>,
}

impl Directory {
    /// Строка каталога для карты.
    ///
    /// Имена файлов идут после счётчиков, а не перед ними: при обрезке бюджета
    /// читателю важно сначала узнать, где он находится, и только потом — что
    /// внутри.
    fn line(&self) -> String {
        let shown: Vec<&str> = self
            .ranked
            .iter()
            .take(NAMES_PER_DIR)
            .map(|(_, _, name)| name.as_str())
            .collect();
        let mut line = format!(
            "{} — {} ф., ~{}",
            self.path,
            self.files,
            compact_tokens(self.tokens)
        );
        if !shown.is_empty() {
            line.push_str(": ");
            line.push_str(&shown.join(", "));
        }
        let hidden = self.ranked.len().saturating_sub(shown.len());
        if hidden > 0 {
            line.push_str(&format!(" …+{hidden}"));
        }
        line.push('\n');
        line
    }
}

/// Собрать каталоги, отсортированные по архитектурному весу.
fn directories(index: &Index, weights: &HashMap<&str, f64>) -> Vec<Directory> {
    let mut map: HashMap<String, Directory> = HashMap::new();
    for file in &index.files {
        let path = parent_of(&file.rel_path);
        let rank = weights.get(file.rel_path.as_str()).copied().unwrap_or(0.0);
        let directory = map
            .entry(path.clone())
            .or_insert_with(|| Directory {
                path,
                files: 0,
                tokens: 0,
                weight: 0.0,
                ranked: Vec::new(),
            });
        directory.files += 1;
        directory.tokens += file.tokens as u64;
        directory.weight += rank;
        directory
            .ranked
            .push((rank, file.tokens, file_name(&file.rel_path)));
    }

    let mut directories: Vec<Directory> = map.into_values().collect();
    for directory in &mut directories {
        // Файлы внутри каталога: сначала важные, потом крупные, потом по имени —
        // чтобы порядок не зависел от порядка обхода и был воспроизводим.
        directory.ranked.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(Ordering::Equal)
                .then_with(|| b.1.cmp(&a.1))
                .then_with(|| a.2.cmp(&b.2))
        });
    }
    directories.sort_by(|a, b| {
        b.weight
            .partial_cmp(&a.weight)
            .unwrap_or(Ordering::Equal)
            // Каталоги без символов (документация, фикстуры) веса не получили:
            // их порядок определяется размером, иначе они оказались бы хвостом
            // списка, хотя размер для них — единственный доступный сигнал.
            .then_with(|| b.tokens.cmp(&a.tokens))
            .then_with(|| a.path.cmp(&b.path))
    });
    directories
}

/// Каталог файла; корень обозначается точкой.
fn parent_of(rel_path: &str) -> String {
    match rel_path.rfind('/') {
        Some(at) => rel_path[..at].to_string(),
        None => ".".to_string(),
    }
}

/// Имя файла без пути каталогов.
///
/// Расширение остаётся: в одном каталоге могут лежать `service.ts` и
/// `service.py`, и без расширения строка инвентаря перестаёт различать их.
fn file_name(rel_path: &str) -> String {
    rel_path.rsplit('/').next().unwrap_or(rel_path).to_string()
}

/// Короткая запись числа токенов: `1.5M`, `48k`, `812`.
pub(crate) fn compact_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{}k", tokens / 1_000)
    } else {
        tokens.to_string()
    }
}

/// Секция документации: markdown-файлы с оглавлением из заголовков.
///
/// Код ранжируется связями, у документации связей нет — и без отдельной секции
/// дока не попадала бы в карту ни при каком бюджете, хотя для входа в чужой
/// проект `ARCHITECTURE.md` полезнее любого блока кода. Заголовки берутся из
/// индекса (их туда кладёт бэкенд Markdown), поэтому файлы здесь не читаются.
///
/// Порядок документов: фокусные (`--file`) выше остальных, далее по токенам.
/// Как и у каталогов, обрезка честная: признак «ещё N документов» обязателен.
pub fn docs_section(index: &Index, focus: &[String], budget: u32) -> String {
    if budget < MIN_BUDGET {
        return String::new();
    }

    let docs: Vec<&super::model::FileEntry> = index
        .files
        .iter()
        .filter(|file| file.lang == Lang::Markdown)
        .collect();
    if docs.is_empty() {
        return String::new();
    }

    // Заголовки уже в тегах индекса: группируем одним проходом, без чтения.
    let mut headings: HashMap<&str, Vec<(&str, u32)>> = HashMap::new();
    for tag in index.tags.iter().filter(|t| t.kind == TagKind::Def) {
        headings
            .entry(tag.rel_path.as_str())
            .or_default()
            .push((tag.name.as_str(), tag.line));
    }

    let focused: Vec<&String> = focus.iter().collect();
    let mut ordered: Vec<&&super::model::FileEntry> = docs.iter().collect();
    ordered.sort_by(|a, b| {
        let a_focus = focused.contains(&&a.rel_path);
        let b_focus = focused.contains(&&b.rel_path);
        b_focus
            .cmp(&a_focus)
            .then_with(|| b.tokens.cmp(&a.tokens))
            .then_with(|| a.rel_path.cmp(&b.rel_path))
    });

    let mut kept: Vec<String> = Vec::new();
    let mut spent = 0u32;
    let mut dropped = 0usize;
    for doc in &ordered {
        let Some(doc_headings) = headings.get(doc.rel_path.as_str()) else {
            // Документ без заголовков всё равно называется: агент должен
            // знать, что он есть, даже если оглавления у него нет.
            let line = format!("{} (~{}):\n", doc.rel_path, compact_tokens(doc.tokens as u64));
            let cost = tokens::estimate(&line);
            if spent + cost <= budget {
                spent += cost;
                kept.push(line);
            } else {
                dropped += 1;
            }
            continue;
        };

        let mut block = format!("{} (~{}):\n", doc.rel_path, compact_tokens(doc.tokens as u64));
        for (name, line) in doc_headings.iter().take(HEADINGS_PER_DOC) {
            block.push_str(&format!("{line:>5} | {name}\n"));
        }
        let hidden = doc_headings.len().saturating_sub(HEADINGS_PER_DOC);
        if hidden > 0 {
            block.push_str(&format!("      … ещё {hidden} разделов\n"));
        }
        let cost = tokens::estimate(&block);
        if spent + cost <= budget {
            spent += cost;
            kept.push(block);
        } else {
            dropped += 1;
        }
    }

    if kept.is_empty() {
        return String::new();
    }

    let mut out = String::from(DOCS_HEADER);
    for block in &kept {
        out.push_str(block);
    }
    if dropped > 0 {
        // Признак обрезки важнее последнего документа: без него список
        // выглядел бы полным.
        let trailer = format!("… ещё {dropped} документов\n");
        if spent + tokens::estimate(&trailer) <= budget {
            out.push_str(&trailer);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::codemap::lang::Lang;
    use crate::tools::codemap::model::{FileEntry, IgnoredDir, IgnoreReason};
    use crate::tools::codemap::rank::RankedDefinition;

    fn file(rel_path: &str, tokens: u32) -> FileEntry {
        FileEntry {
            rel_path: rel_path.into(),
            lang: Lang::Rust,
            bytes: tokens as u64 * 4,
            lines: 10,
            tokens,
            mtime_secs: 1,
            parsed: true,
        }
    }

    fn index(files: Vec<FileEntry>) -> Index {
        Index {
            version: super::super::model::FORMAT_VERSION,
            root: "repo".into(),
            built_at: "2026-10-15 00:00:00".into(),
            files,
            tags: Vec::new(),
            chunks: Vec::new(),
            ignored: vec![IgnoredDir {
                path: "node_modules".into(),
                reason: IgnoreReason::Build,
            }],
        }
    }

    fn ranked(rel_path: &str, name: &str, rank: f64) -> RankedDefinition {
        RankedDefinition {
            rel_path: rel_path.into(),
            name: name.into(),
            line: 1,
            rank,
        }
    }

    #[test]
    fn tiny_budget_yields_no_section_rather_than_a_stub() {
        let index = index(vec![file("src/a.rs", 100)]);
        let weights = file_weights(&[]);
        assert_eq!(section(&index, &weights, 10), "");
    }

    /// Пустая рамка хуже отсутствия секции: заголовок и «ещё 63 каталогов» без
    /// единого содержания выглядят как отказ инструмента. Если после резерва
    /// под сводку игнорируемого не остаётся места хотя бы на одну строку —
    /// секция не печатается вовсе.
    ///
    /// Фикстура повторяет реальный случай king_orch_3 на бюджете 512: длинная
    /// сводка игнорируемых каталогов съедала весь инвентарьный бюджет.
    #[test]
    fn budget_that_cannot_fit_a_single_directory_prints_nothing() {
        let mut index = index(vec![file("src/a.rs", 100)]);
        index.ignored = (0..18)
            .map(|i| IgnoredDir {
                path: format!("ignored_directory_{i}"),
                reason: IgnoreReason::Build,
            })
            .collect();
        let weights = file_weights(&[]);
        let ignored_cost = tokens::estimate(&format!(
            "(игнорируется: {})\n",
            index.ignored_summary()
        ));
        // Бюджет ровно такой, что после резерва под сводку не остаётся ничего.
        let budget = ignored_cost + tokens::estimate(HEADER) + MIN_DIRECTORY_TOKENS - 1;
        assert_eq!(section(&index, &weights, budget), "");

        // На токен больше — секция появляется и содержит каталог.
        let roomy = section(&index, &weights, budget + 1);
        assert!(roomy.contains("src — 1 ф."), "{roomy}");
    }

    #[test]
    fn section_lists_directories_with_counts_and_tokens() {
        let index = index(vec![
            file("src/a.rs", 1000),
            file("src/b.rs", 500),
            file("tools/c.rs", 20),
        ]);
        let text = section(&index, &file_weights(&[]), 400);
        assert!(text.starts_with(HEADER), "заголовок обязателен: {text}");
        // 1000 + 500 = 1500 токенов: `compact_tokens` округляет вниз до килотонен,
        // и в строке написано именно `~1k`, а не точное число.
        assert!(text.contains("src — 2 ф., ~1k: a.rs, b.rs"), "{text}");
        assert!(text.contains("tools — 1 ф., ~20: c.rs"), "{text}");
    }

    /// Крупные числа сжимаются: без этого строка каталога на 168 тысяч токенов
    /// съедала бы инвентарь целиком.
    #[test]
    fn token_counts_are_compact() {
        assert_eq!(compact_tokens(812), "812");
        assert_eq!(compact_tokens(1_500), "1k");
        assert_eq!(compact_tokens(1_680_000), "1.7M");
    }

    /// Порядок каталогов задаёт архитектура, а не размер: каталог из
    /// семидесяти тысяч токенов фикстур не обязан открывать инвентарь.
    #[test]
    fn directories_are_ordered_by_rank_not_by_size() {
        let index = index(vec![file("fixtures/big.jsonl", 200_000), file("src/core.rs", 900)]);
        let ranked = [ranked("src/core.rs", "run", 50.0)];
        let weights = file_weights(&ranked);
        let text = section(&index, &weights, 400);
        let core_at = text.find("src").expect("src есть");
        let fixtures_at = text.find("fixtures").expect("fixtures есть");
        assert!(core_at < fixtures_at, "важный каталог обязан идти первым: {text}");
    }

    /// Счётчик файлов пропущенного каталога не выдумывается: вместо него
    /// называется сам каталог, и агент знает, что он не смотрел.
    #[test]
    fn ignored_directories_are_named_without_invented_counts() {
        let index = index(vec![file("src/a.rs", 100)]);
        let text = section(&index, &file_weights(&[]), 400);
        assert!(text.contains("(игнорируется: node_modules)"), "{text}");
        assert!(!text.contains("[") , "счётчики не выдумываются: {text}");
    }

    #[test]
    fn budget_is_respected() {
        let index = index((0..200).map(|i| file(&format!("dir{i}/f{i}.rs"), 5000)).collect());
        let text = section(&index, &file_weights(&[]), 200);
        assert!(
            tokens::estimate(&text) as f64 <= 200.0 * 1.15,
            "инвентарь обязан укладываться в выделенный бюджет: {}",
            tokens::estimate(&text)
        );
        assert!(text.contains("ещё"), "обрезанный список обязан признать обрезку: {text}");
    }

    #[test]
    fn file_names_are_ordered_by_rank_inside_a_directory() {
        let index = index(vec![
            file("src/large_unused.rs", 90_000),
            file("src/important.rs", 800),
        ]);
        let ranked = [ranked("src/important.rs", "run", 90.0)];
        let weights = file_weights(&ranked);
        let text = section(&index, &weights, 400);
        let important_at = text.find("important.rs").expect("important.rs есть");
        let large_at = text.find("large_unused.rs").expect("large_unused.rs есть");
        assert!(important_at < large_at, "важный файл обязан идти первым: {text}");
    }

    #[test]
    fn file_name_drops_directories_but_keeps_extension() {
        assert_eq!(file_name("src/deep/mod.rs"), "mod.rs");
        assert_eq!(file_name(".gitignore"), ".gitignore");
        assert_eq!(file_name("README"), "README");
    }

    // --- Секция документации -------------------------------------------------

    use crate::tools::codemap::tags::Tag;

    fn md_file(rel_path: &str, tokens: u32) -> FileEntry {
        FileEntry {
            rel_path: rel_path.into(),
            lang: Lang::Markdown,
            bytes: tokens as u64 * 4,
            lines: 50,
            tokens,
            mtime_secs: 1,
            parsed: true,
        }
    }

    fn md_index(files: Vec<FileEntry>, tags: Vec<Tag>) -> Index {
        let mut index = index(files);
        index.tags = tags;
        index
    }

    #[test]
    fn docs_section_lists_documents_with_headings_from_the_index() {
        let index = md_index(
            vec![md_file("docs/ARCHITECTURE.md", 3000)],
            vec![
                Tag::def("docs/ARCHITECTURE.md", "Архитектура", 1),
                Tag::def("docs/ARCHITECTURE.md", "Слой домена", 45),
            ],
        );
        let text = docs_section(&index, &[], 400);
        assert!(text.starts_with(DOCS_HEADER), "заголовок секции обязателен: {text}");
        assert!(text.contains("docs/ARCHITECTURE.md (~3k):"), "{text}");
        assert!(text.contains("| Архитектура"), "{text}");
        assert!(text.contains("| Слой домена"), "{text}");
    }

    #[test]
    fn docs_section_is_empty_without_markdown_files() {
        let index = index(vec![file("src/a.rs", 100)]);
        assert_eq!(docs_section(&index, &[], 400), "");
    }

    #[test]
    fn focused_document_outranks_a_bigger_one() {
        let index = md_index(
            vec![md_file("docs/big.md", 9000), md_file("notes/small.md", 100)],
            vec![Tag::def("notes/small.md", "План", 1)],
        );
        let text = docs_section(&index, &["notes/small.md".to_string()], 400);
        let small_at = text.find("notes/small.md").expect("фокусный док есть");
        let big_at = text.find("docs/big.md").expect("большой док есть");
        assert!(
            small_at < big_at,
            "--file обязан поднимать документ выше крупного: {text}"
        );
    }

    #[test]
    fn docs_without_headings_are_named_but_not_invented() {
        let index = md_index(vec![md_file("README.md", 500)], vec![]);
        let text = docs_section(&index, &[], 400);
        assert!(text.contains("README.md (~500):"), "документ обязан быть назван: {text}");
        assert!(!text.contains("|"), "строк оглавления без заголовков не выдумывается: {text}");
    }

    #[test]
    fn docs_section_truncates_honestly() {
        let files: Vec<FileEntry> = (0..12)
            .map(|i| md_file(&format!("docs/doc{i}.md"), 1000 + i))
            .collect();
        let tags: Vec<Tag> = files
            .iter()
            .flat_map(|f| {
                (0..3).map(move |h| Tag::def(&f.rel_path, &format!("Раздел {h}"), h as u32 * 10 + 1))
            })
            .collect();
        let index = md_index(files, tags);
        let text = docs_section(&index, &[], 120);
        assert!(
            text.contains("ещё") && text.contains("документов"),
            "обрезанный список обязан признать обрезку: {text}"
        );
        assert!(
            tokens::estimate(&text) as f64 <= 120.0 * 1.15,
            "секция обязана укладываться в бюджет: {}",
            tokens::estimate(&text)
        );
    }

    /// Число заголовков одного документа ограничено: огромная оглавление
    /// одного файла не должно съедать секцию.
    #[test]
    fn heading_list_per_document_is_capped() {
        let tags: Vec<Tag> = (0..20)
            .map(|h| Tag::def("docs/big.md", &format!("Раздел номер {h}"), h as u32 * 5 + 1))
            .collect();
        let index = md_index(vec![md_file("docs/big.md", 4000)], tags);
        let text = docs_section(&index, &[], 400);
        assert!(
            text.contains("ещё 14 разделов"),
            "лишние заголовки обязаны быть посчитаны честно: {text}"
        );
        assert!(
            !text.contains("Раздел номер 19"),
            "заголовки сверх потолка не печатаются: {text}"
        );
    }
}