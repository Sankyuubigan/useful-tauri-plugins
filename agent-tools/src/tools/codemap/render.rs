//! Рендер карты проекта и подгонка под бюджет токенов.
//!
//! ## Что показывает карта
//!
//! Три слоя, и каждый отвечает на свой вопрос:
//!
//! - **инвентарь** ([`super::inventory`]) — «из чего состоит проект»: каталоги,
//!   счётчики, токены, сводка по игнорируемому;
//! - **блоки кода** — «как устроен этот символ»: строки исходника вокруг важного
//!   определения, с элизией пропусков;
//! - **шапка** — цифры индекса, по которым видно, что карта вообще построена.
//!
//! ## Почему рендер круговой, а не жадный
//!
//! Жадный проход («весь файл, потом следующий») на реальном репозитории отдавал
//! весь бюджет одному файлу: на king_orch_3 карта на 2048 токена покрывала
//! **1 файл из 361**, и ядро проекта не было видно ни при каком бюджете. Причина
//! не в том, что один файл «важнее», — а в том, что у рендера не было правила
//! остановиться.
//!
//! Правило теперь одно: **каждый файл получает одну строку контекста за круг**,
//! и круги повторяются, пока хватает бюджета. Один круг — это «по символу из
//! каждого файла», и он даёт широту; следующие круги дают глубину тем файлам,
//! которые её выдерживают. Широта при этом не стоит ничего: правило одно на все
//! файлы и не смотрит на то, кто «главный».
//!
//! ## Потолок на файл
//!
//! Даже при круговом рендере файл с сотней определений способен вытеснить
//! остальных, когда бюджет велик. Поэтому у файла есть жёсткий потолок:
//! [`FILE_SHARE`] от бюджета, но не больше [`MAX_FILE_TOKENS`] — при очень
//! большом бюджете «15% от 32768» это почти пять тысяч токенов на один файл,
//! и карта снова стала бы пересказом одного модуля.
//!
//! Первое определение потолком не ограничено: файл, попавший в карту, обязан
//! показать хоть что-то, иначе строка каталога без единой строки кода выглядела
//! бы как «здесь ничего нет».

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::inventory;
use super::model::Index;
use super::rank::RankedDefinition;
use super::tags::Tag;
use super::tokens;

/// Потолок одного файла как доля от бюджета карты.
const FILE_SHARE: f64 = 0.15;

/// Абсолютный потолок одного файла в токенах.
const MAX_FILE_TOKENS: u32 = 250;

/// Насколько контекст расширяется вокруг строки определения.
///
/// Две строки до и одна после: этого хватает, чтобы увидеть `pub fn` вместе с
/// атрибутами и открывающую строку тела. Больше — уже чужой код.
const CONTEXT_BEFORE: usize = 2;
const CONTEXT_AFTER: usize = 1;

/// Значок пропуска строк, как в карте Aider.
const ELLIPSIS: &str = "...";

/// Построить карту проекта, уложив её в `budget_tokens`.
///
/// Возвращает готовый текст и его оценку в токенах.
pub fn render_map(
    root: &Path,
    index: &Index,
    ranked: &[RankedDefinition],
    focus: &[String],
    budget_tokens: u32,
) -> (String, u32) {
    if ranked.is_empty() {
        return (String::new(), 0);
    }

    let mut by_file: HashMap<&str, Vec<&RankedDefinition>> = HashMap::new();
    for definition in ranked {
        by_file
            .entry(&definition.rel_path)
            .or_default()
            .push(definition);
    }
    // Порядок файлов — по лучшему рангу определения в них, а не по алфавиту.
    // Иначе самый важный файл репозитория оказывался бы последним, и агент
    // начинал бы чтение карты с чужого файла.
    let mut files: Vec<&&str> = by_file.keys().collect();
    files.sort_by(|a, b| {
        best_rank(&by_file, b)
            .partial_cmp(&best_rank(&by_file, a))
            .unwrap_or(std::cmp::Ordering::Equal)
            // Равные ранги: путь, чтобы порядок был воспроизводим.
            .then_with(|| a.cmp(b))
    });

    // Документация откусывает от инвентарной доли, а не от кода: оглавление —
    // часть обзора. Без markdown-файлов секция пуста и каталогам возвращается
    // вся доля — поведение прежнее.
    let inventory_budget = (budget_tokens as f64 * inventory::SHARE) as u32;
    let docs_budget = (inventory_budget as f64 * inventory::DOCS_SHARE) as u32;
    let docs_text = inventory::docs_section(index, focus, docs_budget);
    let dirs_budget = inventory_budget.saturating_sub(tokens::estimate(&docs_text));
    let inventory_text = inventory::section(index, &inventory::file_weights(ranked), dirs_budget);

    let header = format!(
        "# Карта проекта: {}\n\n> {} | файлов: {} | разобрано: {} | символов: {} | токенов: ~{}\n\n",
        index.root,
        index.built_at,
        index.files.len(),
        index.parsed_count(),
        index.tags.iter().filter(|t| t.kind == super::tags::TagKind::Def).count(),
        index.total_tokens(),
    );

    // Код покупается на остаток после шапки и обзора: инвентарь с документацией
    // зарезервированы долей, а не «сколько вышло», иначе обзор забирал бы больше
    // трети карты в проектах с большим числом каталогов.
    let reserved = tokens::estimate(&header)
        + tokens::estimate(&inventory_text)
        + tokens::estimate(&docs_text);
    let mut budget_left = budget_tokens.saturating_sub(reserved);
    let cap = per_file_cap(budget_tokens);

    let mut sources = Sources::new(root.to_path_buf());
    let mut plans: Vec<FilePlan<'_>> = files
        .iter()
        .map(|path| FilePlan {
            path,
            definitions: by_file[*path].as_slice(),
            taken: 0,
            block: String::new(),
            tokens: 0,
            unreadable: false,
        })
        .collect();

    loop {
        let mut progressed = false;
        for plan in &mut plans {
            if plan.offer(&mut sources, cap, &mut budget_left) {
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }

    let mut out = header;
    out.push_str(&inventory_text);
    out.push_str(&docs_text);
    for plan in &plans {
        out.push_str(&plan.block);
    }
    let used = tokens::estimate(&out);
    (out, used)
}

/// Потолок одного файла при данном бюджете.
fn per_file_cap(budget_tokens: u32) -> u32 {
    let share = (budget_tokens as f64 * FILE_SHARE) as u32;
    share.min(MAX_FILE_TOKENS)
}

/// Лучший ранг среди определений файла.
fn best_rank(by_file: &HashMap<&str, Vec<&RankedDefinition>>, rel: &&str) -> f64 {
    by_file
        .get(*rel)
        .and_then(|list| {
            list.iter()
                .map(|d| d.rank)
                .fold(None, |acc, r| Some(acc.map_or(r, |best: f64| if r > best { r } else { best })))
        })
        .unwrap_or(0.0)
}

/// Состояние одного файла в карте.
struct FilePlan<'a> {
    /// Путь файла относительно корня.
    path: &'a str,
    /// Определения файла, отсортированные по убыванию ранга.
    definitions: &'a [&'a RankedDefinition],
    /// Сколько определений уже взято.
    taken: usize,
    /// Готовый блок карты для этого файла.
    block: String,
    /// Оценка блока в токенах.
    tokens: u32,
    /// Файл не прочитан: дальше tries бессмысленны.
    unreadable: bool,
}

impl FilePlan<'_> {
    /// Попытаться добавить следующее определение файла.
    ///
    /// Возвращает `true`, если что-то добавилось. Отказ по месту не считается
    /// ошибкой: круговой рендер обязан спрашивать все файлы подряд, иначе один
    /// дорогой блок остановил бы и дешёвые, которые в бюджет поместились бы.
    fn offer(&mut self, sources: &mut Sources, cap: u32, budget_left: &mut u32) -> bool {
        if self.unreadable || self.taken >= self.definitions.len() {
            return false;
        }
        if self.taken > 0 && self.tokens >= cap {
            return false;
        }
        let Some(lines) = sources.lines(self.path) else {
            self.unreadable = true;
            return false;
        };

        let wanted: Vec<&RankedDefinition> = self.definitions[..=self.taken].to_vec();
        // Заголовок файла — часть блока, а не отдельная строка сверху: блок
        // пересобирается целиком при каждом добавлении, и шапка обязана
        // пересчитываться вместе с телом, иначе карта лишилась бы названия
        // файла ровно в тот момент, когда он попал в бюджет.
        let mut block = String::from(self.path);
        block.push_str(":\n");
        block.push_str(&render_lines(lines, &wanted));
        let cost = tokens::estimate(&block);
        // Дельта, а не полная стоимость: блок пересобирается целиком при каждом
        // добавлении, а уже оплаченные строки повторно в бюджет не идут.
        let delta = cost.saturating_sub(self.tokens);
        if delta > *budget_left {
            return false;
        }

        self.block = block;
        self.tokens = cost;
        self.taken += 1;
        *budget_left -= delta;
        true
    }
}

/// Ленивый кеш исходников.
///
/// Файлы читаются только те, чей блок реально попал в карту. Раньше они читались
/// все, до подбора бюджета: на репозитории в сотни файлов это означало держать
/// в памяти исходники, из которых в карту попал один.
struct Sources {
    root: PathBuf,
    /// `None` — файл не удалось прочитать; повторно не читается.
    cache: HashMap<String, Option<Vec<String>>>,
}

impl Sources {
    fn new(root: PathBuf) -> Sources {
        Sources {
            root,
            cache: HashMap::new(),
        }
    }

    fn lines(&mut self, rel_path: &str) -> Option<&[String]> {
        let entry = self
            .cache
            .entry(rel_path.to_string())
            .or_insert_with(|| match std::fs::read_to_string(self.root.join(rel_path)) {
                Ok(source) => Some(source.lines().map(str::to_string).collect()),
                Err(error) => {
                    // Файл был в индексе, но исчез или стал нечитаемым. Молчать
                    // здесь нельзя: агент увидел бы каталог и решил, что кода
                    // в нём нет.
                    log::warn!("codemap: {rel_path} прочитан с потерями: {error}");
                    None
                }
            });
        entry.as_deref()
    }
}

/// Отрисовать строки вокруг отмеченных позиций с элизией пропусков.
fn render_lines(lines: &[String], wanted: &[&RankedDefinition]) -> String {
    let wanted: Vec<&RankedDefinition> = wanted.to_vec();
    let mut marked: Vec<u32> = wanted.iter().map(|d| d.line).collect();
    marked.sort_unstable();
    marked.dedup();

    // Склеиваем перекрывающиеся окна в диапазоны: иначе две соседние функции
    // показались бы двумя отдельными блоками с многоточием между ними.
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for line in &marked {
        let start = (*line as usize).saturating_sub(CONTEXT_BEFORE).max(1);
        let end = ((*line as usize) + CONTEXT_AFTER).min(lines.len().max(1));
        match ranges.last_mut() {
            Some(last) if start <= last.1 + 1 => last.1 = last.1.max(end),
            _ => ranges.push((start, end)),
        }
    }

    let mut out = String::new();
    let mut previous_end = 0usize;
    for (start, end) in ranges {
        if previous_end > 0 {
            out.push_str(ELLIPSIS);
            out.push('\n');
        }
        for number in start..=end {
            if number > lines.len() {
                break;
            }
            // Длинные строки (минифицированный код) обрезаются: одна строка в
            // 100 КБ символов съела бы весь бюджет карты целиком.
            let text: String = lines[number - 1].chars().take(100).collect();
            out.push_str(&format!("{number:>5} | {text}\n"));
        }
        previous_end = end;
    }
    out
}

/// Строки определения символа по всем тегам индекса.
///
/// Нужна внешним вызывающим (MCP и king_orch), чтобы показать агенту место
/// символа без повторного чтения файла.
pub fn locate<'a>(index: &'a Index, name: &str) -> Vec<&'a Tag> {
    index
        .tags
        .iter()
        .filter(|tag| tag.name == name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::codemap::model::FileEntry;
    use crate::tools::codemap::rank::RankedDefinition;

    fn temp_root(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("codemap_render_{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("временный каталог");
        dir
    }

    const SOURCE: &str = "\
// заголовочный комментарий
use std::fmt;

pub struct Service {
    field: u32,
}

impl Service {
    pub fn run(&self, input: u32) -> u32 {
        self.helper(input)
    }

    fn helper(&self, value: u32) -> u32 {
        value + 1
    }
}

pub fn entry() -> u32 {
    Service { field: 0 }.run(1)
}
";

    fn ranked() -> Vec<RankedDefinition> {
        vec![
            RankedDefinition {
                rel_path: "a.rs".into(),
                name: "entry".into(),
                line: 18,
                rank: 10.0,
            },
            RankedDefinition {
                rel_path: "a.rs".into(),
                name: "run".into(),
                line: 8,
                rank: 5.0,
            },
            RankedDefinition {
                rel_path: "a.rs".into(),
                name: "helper".into(),
                line: 12,
                rank: 1.0,
            },
        ]
    }

    fn index() -> Index {
        Index {
            version: crate::tools::codemap::model::FORMAT_VERSION,
            root: "repo".into(),
            built_at: "2026-10-15 00:00:00".into(),
            files: vec![FileEntry {
                rel_path: "a.rs".into(),
                lang: crate::tools::codemap::lang::Lang::Rust,
                bytes: 300,
                lines: 19,
                tokens: 80,
                mtime_secs: 1,
                parsed: true,
            }],
            tags: Vec::new(),
            chunks: Vec::new(),
            ignored: Vec::new(),
        }
    }

    #[test]
    fn map_shows_source_lines_not_just_names() {
        let root = temp_root("lines");
        std::fs::write(root.join("a.rs"), SOURCE).expect("исходник");
        let (text, _) = render_map(&root, &index(), &ranked(), &[], 10_000);
        assert!(text.contains("pub fn entry"), "карта обязана содержать строки кода");
        assert!(text.contains("a.rs:"), "карта обязана называть файл");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn gaps_are_elided() {
        let root = temp_root("ellipsis");
        std::fs::write(root.join("a.rs"), SOURCE).expect("исходник");
        let (text, _) = render_map(&root, &index(), &ranked(), &[], 10_000);
        assert!(text.contains(ELLIPSIS), "пропуски обязаны быть обозначены");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Внутри одного файла определения идут в порядке исходника, а не по рангу:
/// карта обязана читаться как код, а не как выжимка. Ранг определяет, какие
/// строки вообще попадут в карту и в каком порядке идут **файлы**.
    #[test]
    fn definitions_inside_a_file_keep_source_order() {
        let root = temp_root("order");
        std::fs::write(root.join("a.rs"), SOURCE).expect("исходник");
        let (text, _) = render_map(&root, &index(), &ranked(), &[], 10_000);
        let run_at = text.find("pub fn run").expect("run есть");
        let helper_at = text.find("fn helper").expect("helper есть");
        let entry_at = text.find("pub fn entry").expect("entry есть");
        assert!(run_at < helper_at, "порядок в файле обязан следовать исходнику");
        assert!(helper_at < entry_at);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Файлы идут по убыванию ранга: карта обязана начинаться с ядра проекта,
    /// а не с файла, который случайно стоит первым по алфавиту.
    #[test]
    fn files_are_ordered_by_rank() {
        let root = temp_root("fileorder");
        std::fs::write(root.join("a.rs"), SOURCE).expect("исходник");
        std::fs::write(root.join("z.rs"), SOURCE).expect("исходник");
        let ranked = vec![
            RankedDefinition {
                rel_path: "z.rs".into(),
                name: "run".into(),
                line: 8,
                rank: 9.0,
            },
            RankedDefinition {
                rel_path: "a.rs".into(),
                name: "run".into(),
                line: 8,
                rank: 1.0,
            },
        ];
        let (text, _) = render_map(&root, &index(), &ranked, &[], 10_000);
        let z_at = text.find("z.rs:").expect("z.rs есть в карте");
        let a_at = text.find("a.rs:").expect("a.rs есть в карте");
        assert!(
            z_at < a_at,
            "файл с большим рангом обязан идти первым, даже если по алфавиту он позже"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn long_lines_are_truncated() {
        let mut lines = vec!["x".repeat(5_000)];
        lines.extend(std::iter::repeat_n(String::new(), 3));
        let definition = RankedDefinition {
            rel_path: "a.rs".into(),
            name: "wide".into(),
            line: 1,
            rank: 1.0,
        };
        let rendered = render_lines(&lines, &[&definition]);
        let longest = rendered.lines().map(str::len).max().unwrap_or(0);
        assert!(longest < 200, "строка обязана быть обрезана: {longest}");
    }

    /// ГЕЙТ 4: при заданном бюджете карта обязана в него укладываться.
    #[test]
    fn gate4_map_fits_the_token_budget() {
        let root = temp_root("budget");
        std::fs::write(root.join("a.rs"), SOURCE).expect("исходник");
        let mut definitions = Vec::new();
        for i in 0..40 {
            definitions.push(RankedDefinition {
                rel_path: "a.rs".into(),
                name: format!("sym{i}"),
                line: (i as u32 % 15) + 1,
                rank: 100.0 - i as f64,
            });
        }
        let budget = 60u32;
        let (text, cost) = render_map(&root, &index(), &definitions, &[], budget);
        assert!(
            cost as f64 <= budget as f64 * 1.15 + 1.0,
            "карта обязана укладываться в бюджет {budget}, получилось {cost}"
        );
        assert!(!text.is_empty(), "карта не должна выродиться в пустую строку");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bigger_budget_yields_more_content() {
        let root = temp_root("budget2");
        std::fs::write(root.join("a.rs"), SOURCE).expect("исходник");
        let mut definitions = Vec::new();
        for i in 0..40 {
            definitions.push(RankedDefinition {
                rel_path: "a.rs".into(),
                name: format!("sym{i}"),
                line: (i as u32 % 15) + 1,
                rank: 100.0 - i as f64,
            });
        }
        let (small, _) = render_map(&root, &index(), &definitions, &[], 40);
        let (large, _) = render_map(&root, &index(), &definitions, &[], 4_000);
        assert!(
            large.len() > small.len(),
            "больший бюджет обязан дать больше кода: {} против {}",
            large.len(),
            small.len()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn empty_index_renders_empty_map() {
        let root = temp_root("empty");
        let (text, cost) = render_map(&root, &index(), &[], &[], 1_000);
        assert!(text.is_empty());
        assert_eq!(cost, 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Репозиторий из N файлов: каждый файл — две функции.
    fn many_files(root: &Path, count: usize) -> Vec<RankedDefinition> {
        let mut definitions = Vec::new();
        for i in 0..count {
            let rel = format!("mod{i}/file{i}.rs");
            let source = format!(
                "pub fn top_{i}() -> u32 {{\n    helper_{i}()\n}}\n\nfn helper_{i}() -> u32 {{\n    {i}\n}}\n\npub fn extra_{i}(v: u32) -> u32 {{\n    v + {i}\n}}\n"
            );
            let path = root.join(&rel);
            std::fs::create_dir_all(path.parent().expect("есть родитель")).expect("каталог");
            std::fs::write(&path, source).expect("исходник");
            for (n, rank) in [(0usize, 100.0 - i as f64), (1, 50.0 - i as f64), (5, 10.0)] {
                definitions.push(RankedDefinition {
                    rel_path: rel.clone(),
                    name: format!("sym{i}_{n}"),
                    line: (n as u32 * 4) + 1,
                    rank,
                });
            }
        }
        definitions
    }

    /// ГЕЙТ 8, локальная форма: карта обязана покрывать много разных файлов,
    /// а не вырождаться в один. Раньше жадный рендер отдавал весь бюджет
    /// первому файлу — здесь это и проверяется.
    #[test]
    fn gate8_map_covers_many_files_not_one() {
        let root = temp_root("breadth");
        let definitions = many_files(&root, 40);
        let (text, _) = render_map(&root, &index(), &definitions, &[], 2_048);
        let files: Vec<&str> = text
            .lines()
            .filter(|line| line.ends_with(".rs:"))
            .map(|line| &line[..line.len() - 1])
            .collect();
        assert!(
            files.len() >= 10,
            "карта обязана покрывать не меньше 10 файлов, покрыла {}: {:?}",
            files.len(),
            &files[..files.len().min(5)]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// ГЕЙТ 10: один файл не вправе съесть карту, сколько бы токенов ни было.
    #[test]
    fn gate10_no_file_exceeds_its_share_of_the_map() {
        let root = temp_root("cap");
        let definitions = many_files(&root, 12);
        let (text, used) = render_map(&root, &index(), &definitions, &[], 2_048);
        let blocks: Vec<&str> = text
            .split_inclusive(".rs:\n")
            .skip(1)
            .map(|tail| tail.split(".rs:").next().unwrap_or(tail))
            .collect();
        assert!(!blocks.is_empty(), "в карте обязан быть хотя бы один файл");
        for (i, block) in blocks.iter().enumerate() {
            let share = tokens::estimate(block) as f64 / used.max(1) as f64;
            assert!(
                share <= FILE_SHARE,
                "блок {i} занял {:.1}% карты при потолке {:.0}%",
                share * 100.0,
                FILE_SHARE * 100.0
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Первое определение файла потолком не ограничено: файл, дошедший до
    /// карты, обязан показать хоть что-то.
    #[test]
    fn a_file_that_made_it_into_the_map_shows_at_least_one_line() {
        let root = temp_root("firstline");
        let definitions = many_files(&root, 6);
        let (text, _) = render_map(&root, &index(), &definitions, &[], 4_000);
        assert!(text.contains(".rs:\n"), "у каждого блока обязано быть тело");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Файл, исчезнувший после индексации, обязан быть виден в логе, а не
    /// молча выпасть: иначе каталог в карте выглядел бы пустым по-настоящему.
    #[test]
    fn missing_source_file_is_not_silently_dropped() {
        let root = temp_root("missing_file");
        let definitions = vec![RankedDefinition {
            rel_path: "ghost.rs".into(),
            name: "ghost".into(),
            line: 1,
            rank: 10.0,
        }];
        let (text, used) = render_map(&root, &index(), &definitions, &[], 4_000);
        assert!(!text.contains("ghost.rs:"), "нечитаемый файл не должен давать блок");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Markdown-файлы попадают в карту секцией документации: без неё дока не
    /// попадала бы в карту ни при каком бюджете — у прозы нет связей, которые
    /// качают ранг. Заголовки берутся из тегов индекса, файлы не читаются.
    #[test]
    fn markdown_files_get_a_docs_section_with_headings() {
        let root = temp_root("docs");
        std::fs::write(root.join("a.rs"), SOURCE).expect("исходник");
        std::fs::write(
            root.join("ARCHITECTURE.md"),
            "# Архитектура\n\nТекст\n\n## Слои\n",
        )
        .expect("документ");

        let mut index = index();
        index.files.push(crate::tools::codemap::model::FileEntry {
            rel_path: "ARCHITECTURE.md".into(),
            lang: crate::tools::codemap::lang::Lang::Markdown,
            bytes: 40,
            lines: 5,
            tokens: 12,
            mtime_secs: 1,
            parsed: true,
        });
        index.tags = vec![
            Tag::def("ARCHITECTURE.md", "Архитектура", 1),
            Tag::def("ARCHITECTURE.md", "Слои", 5),
        ];

        let (text, _) = render_map(&root, &index, &ranked(), &[], 2_048);
        assert!(text.contains("## 📚 Документация"), "секция обязана появиться:\n{text}");
        assert!(text.contains("ARCHITECTURE.md"), "{text}");
        assert!(text.contains("| Архитектура"), "{text}");
        assert!(text.contains("| Слои"), "{text}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Без markdown-файлов секции нет и поведение прежнее: инвентарь получает
    /// всю свою долю.
    #[test]
    fn no_markdown_files_means_no_docs_section() {
        let root = temp_root("no_docs");
        std::fs::write(root.join("a.rs"), SOURCE).expect("исходник");
        let (text, _) = render_map(&root, &index(), &ranked(), &[], 2_048);
        assert!(!text.contains("Документация"), "секции без доков быть не должно:\n{text}");
        let _ = std::fs::remove_dir_all(&root);
    }
}