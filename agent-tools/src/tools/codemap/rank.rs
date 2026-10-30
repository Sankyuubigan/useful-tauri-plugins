//! Персонализированный PageRank и раскладка ранга по определениям.
//!
//! Формулы портированы из `aider/repomap.py`. Реализация своя, потому что
//! `networkx` на Rust нет, а тащить `petgraph` ради тридцати строк итерации
//! незачем: граф здесь — это просто список рёбер.
//!
//! ## Почему степенное итерирование, а не решение системы
//!
//! Для PageRank неважна точность абсолютных значений — важна **порядок** файлов
//! по значимости. Итерация.power это даёт и даёт за 15–20 проходов на проектах
//! любого размера, где решение линейной системы стоило бы дороже и требовало
//! внешней библиотеки.

use std::collections::HashMap;

use super::graph::Graph;
use super::tags::{Tag, TagKind};

/// Коэффициент затухания PageRank.
const DAMPING: f64 = 0.85;

/// Сколько итераций достаточно для стабилизации порядка.
const ITERATIONS: usize = 20;

/// Насколько сумма рангов должна сойтись, чтобы итерации остановить.
const EPSILON: f64 = 1e-9;

/// Каталоги, считающиеся vendored/чужим кодом по имени компонента пути.
///
/// Список намеренно короткий: ложное понижение настоящего кода хуже, чем
/// оставшийся в карте чужой файл. «gen» и подобные слишком общие имена сюда
/// не попали именно поэтому.
const VENDOR_DIRS: &[&str] = &["vendor", "vendored", "third_party", "thirdparty", "external"];

/// На сколько множится ранг vendored-пути.
///
/// Не ноль, а малое число: vendored-файл, на который ссылается половина
/// проекта, всё-таки часть графа, и при большом бюджете карты он достоин
/// последнего места — а не исчезновения. Число подобрано на king_orch_3:
/// при 0.2 `astronomy.js` (импортируется одним файлом, но 16 упоминаний
/// имён с ×10 за длину) всё ещё возглавлял карту.
const VENDOR_FACTOR: f64 = 0.1;

/// Настройки ранжирования, общие для CLI и MCP.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RankOptions {
    /// Не понижать ранг vendored/сгенерированного кода.
    pub boost_vendor: bool,
    /// Gitignore-подобные шаблоны: подходящие пути не ранжируются вовсе.
    pub exclude: Vec<String>,
}

/// Vendored-ли путь.
///
/// Два признака: имя каталога в любой позиции пути из [`VENDOR_DIRS`] либо
/// файл объявления типов `.d.ts` — он описывает чужую библиотеку, даже когда
/// лежит рядом с кодом.
pub fn is_vendor_path(rel_path: &str) -> bool {
    let path = rel_path.replace('\\', "/");
    let file_name = path.rsplit('/').next().unwrap_or(path.as_str());
    if file_name.ends_with(".d.ts") {
        return true;
    }
    path.split('/').any(|component| VENDOR_DIRS.contains(&component))
}

/// Подходит ли путь хоть под один шаблон исключения.
fn is_excluded(rel_path: &str, patterns: &[String]) -> bool {
    patterns
        .iter()
        .any(|pattern| super::scan::matches(pattern, rel_path))
}

/// Ранг каждого файла.
pub fn pagerank(graph: &Graph) -> HashMap<String, f64> {
    let n = graph.files.len();
    if n == 0 {
        return HashMap::new();
    }

    // Список исходящих рёбер на каждый узел.
    //
    // Самопетли (вес 0.1 из графа) намеренно ИСКЛЮЧЕНЫ из распределения. Их
    // единственная задача — удержать файл в графе, когда на его символы никто
    // не ссылается. Если их учесть в перераспределении, изолированный файл
    // не теряет массу вообще и накапливает ранг выше настоящего центра
    // проекта: проверено арифметикой, `lonely.rs` обгонял цитируемый `core.rs`.
    let mut out_edges: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    let index: HashMap<&String, usize> = graph
        .files
        .iter()
        .enumerate()
        .map(|(i, f)| (f, i))
        .collect();
    for (from, to, weight) in &graph.edges {
        if from == to {
            continue;
        }
        if let (Some(&f), Some(&t)) = (index.get(from), index.get(to)) {
            out_edges[f].push((t, *weight));
        }
    }

    let base = 1.0 / n as f64;
    let mut ranks: Vec<f64> = vec![base; n];

    // Персонализация нормируется на единицу: PageRank складывает её с телепортом,
    // а не умножает на текущий ранг. Умножение здесь давало бы совсем другую
    // динамику — упомянутый файл тянулся бы к нулю, а не поднимался.
    let personal_total: f64 = graph.personalization.values().sum();
    let personal: Vec<f64> = graph
        .files
        .iter()
        .map(|file| {
            if personal_total > 0.0 {
                graph.personalization.get(file).copied().unwrap_or(0.0) / personal_total
            } else {
                0.0
            }
        })
        .collect();

    for _ in 0..ITERATIONS {
        let mut next = vec![0.0; n];

        // Висячие узлы (без исходящих рёбер) возвращают свой ранг в равномерную
        // часть. Иначе масса просто исчезала бы, и сумма рангов перестала бы
        // быть единицей — то есть ранги разных файлов сравнивались бы неверно.
        let mut dangling = 0.0;
        for i in 0..n {
            if out_edges[i].is_empty() {
                dangling += ranks[i];
            }
        }

        // Суммы весов считаются один раз на узел, а не на каждое ребро:
        // иначе внутренний цикл стал бы квадратичным по числу рёбер.
        let totals: Vec<f64> = out_edges
            .iter()
            .map(|edges| edges.iter().map(|(_, w)| *w).sum())
            .collect();

        for i in 0..n {
            let mut incoming = 0.0;
            for (j, edges) in out_edges.iter().enumerate() {
                if totals[j] <= 0.0 {
                    continue;
                }
                for (target, weight) in edges {
                    if *target == i {
                        incoming += ranks[j] * weight / totals[j];
                    }
                }
            }
            next[i] = base * (1.0 - DAMPING)
                + DAMPING * (incoming + dangling * base + personal[i]);
        }

        let delta: f64 = next.iter().zip(&ranks).map(|(a, b)| (a - b).abs()).sum();
        ranks = next;
        if delta < EPSILON {
            break;
        }
    }

    graph
        .files
        .iter()
        .enumerate()
        .map(|(i, file)| (file.clone(), ranks[i]))
        .collect()
}

/// Определение символа вместе с накопленным рангом.
#[derive(Debug, Clone, PartialEq)]
pub struct RankedDefinition {
    /// Путь файла.
    pub rel_path: String,
    /// Имя символа.
    pub name: String,
    /// Номер строки, на которой символ объявлен, 1-based.
    ///
    /// Поле не вспомогательное: без него рендер не может показать строку кода и
    /// карта выродилась бы в список имён — то есть в инвентарь.
    pub line: u32,
    /// Ранг пары (файл, символ).
    pub rank: f64,
}

/// Разложить ранг файлов по конкретным определениям.
///
/// Ранг файла делится между его определениями пропорционально весу исходящих
/// рёбер, по которым эти определения и были найдены. Именно этот шаг превращает
/// «важный файл» в «важные функции этого файла».
///
/// Ссылка сама по себе не говорит, к какому именно определению она ведёт:
/// обходы собирают имена, а не разрешённые цели. Поэтому вклад распределяется
/// по всем определениям одноимённого символа — иначе неоднозначное имя вроде
/// `new` поднимало бы случайный файл.
pub fn rank_definitions(graph: &Graph, tags: &[Tag]) -> Vec<RankedDefinition> {
    rank_definitions_opts(graph, tags, &RankOptions::default())
}

/// То же с настройками: даунвейт vendored-кода и исключение путей.
///
/// ## Почему самопетли не участвуют в накоплении весов
///
/// Ребро `файл → файл` честно существует (рекурсия и внутренние вызовы — связи,
/// см. `graph::build`), и в PageRank оно корректно игнорируется. Но шаг ниже —
/// «ранг файла × вес рёбер × число упоминаний» — по сырым рёбрам самопетлю
/// пропускал, и большой файл, использующий собственные символы сотни раз
/// (типичный vendored-код), накапливал вес, в разы превышающий вклад настоящих
/// внешних ссылок. На king_orch_3 из-за этого `astronomy.js` с его 348
/// внутренними упоминаниями возглавлял карту, оттесняя реальное ядро проекта.
/// Кросс-файловый интерес — единственный, который карта и обещает показывать;
/// «файл интересен самому себе» интересом не считается.
pub fn rank_definitions_opts(graph: &Graph, tags: &[Tag], opts: &RankOptions) -> Vec<RankedDefinition> {
    // Даунвейт vendored-кода применяется к рангу файла после PageRank: множитель
    // сохраняет порядок между обычными файлами и лишь сдвигает vendored вниз.
    // Важно, что это делается на копии: сам PageRank обязан оставаться
    // нормированной на единицу шкалой — на ней держатся тесты сравнимости.
    let file_ranks: HashMap<String, f64> = pagerank(graph)
        .into_iter()
        .map(|(file, rank)| {
            let rank = if opts.boost_vendor {
                rank
            } else if is_vendor_path(&file) {
                rank * VENDOR_FACTOR
            } else {
                rank
            };
            (file, rank)
        })
        .collect();
    let mut weights: HashMap<(&str, &str), f64> = HashMap::new();
    // Имя → файл → строки объявления. Строки нужны рендеру; если символ
    // объявлен в файле несколько раз, берётся первая — карта показывает одно
    // место, а не молча выбирает случайное.
    // --- Подготовка индексов -----------------------------------------------------
    //
    // Раньше здесь стоял вложенный цикл «рёбра × все теги»: на king_orch это
    // 22 246 × 42 496 ≈ 945 млн итераций, и карта строилась 12 секунд.
    // Стоимость росла как произведение размеров двух независимых списков —
    // то есть была квадратичной по объёму данных, и никакой порог её не
    // устранял: репозиторий вдвое больше — вчетверо медленнее.
    //
    // Теперь стоимость пропорциональна реальным совпадениям, а не их
    // потенциальному количеству: обе стороны перечисляются один раз и
    // соединяются по ключу.

    // Имя → строка объявления, для каждого файла.
    let mut defined_at: HashMap<&str, HashMap<&str, u32>> = HashMap::new();
    // Файл-референсер → сколько раз он ссылался на каждое имя.
    let mut refs_by_file: HashMap<&str, HashMap<&str, u32>> = HashMap::new();

    for tag in tags {
        match tag.kind {
            TagKind::Def => {
                defined_at
                    .entry(tag.name.as_str())
                    .or_default()
                    .entry(tag.rel_path.as_str())
                    .or_insert(tag.line);
            }
            TagKind::Ref => {
                *refs_by_file
                    .entry(tag.rel_path.as_str())
                    .or_default()
                    .entry(tag.name.as_str())
                    .or_insert(0) += 1;
            }
        }
    }

    // Файл → исходящие рёбра (куда этот файл ссылается и с каким весом).
    let mut edges_by_referencer: HashMap<&str, Vec<(&str, f64)>> = HashMap::new();
    for (referencer, definer, weight) in &graph.edges {
        edges_by_referencer
            .entry(referencer.as_str())
            .or_default()
            .push((definer.as_str(), *weight));
    }

    for (referencer, names) in &refs_by_file {
        // Работаем только по **ссылкам** из файла-референсера: именно они
        // объясняют, почему ребро вообще есть.
        //
        // Ошибка, которая здесь была: условие отбрасывало ссылки и пропускало
        // определения, из-за чего засчитывалась самопетля `b.rs:entry` вместо
        // настоящей связи `b.rs -> a.rs` по имени `core`.
        let Some(edges) = edges_by_referencer.get(referencer) else {
            continue;
        };
        for (definer, weight) in edges {
            // Самопетля не выражает внешнего интереса: см. докомментарий функции.
            if *definer == *referencer {
                continue;
            }
            for (name, _count) in names {
                let Some(places) = defined_at.get(name) else { continue };
                if !places.contains_key(*definer) {
                    continue;
                }
                // Частота упоминаний оплачивается РОВНО ОДИН РАЗ — в весе ребра
                // (`mul · √num_refs`, см. `graph::build`). Умножение на count
                // здесь давало бы второй учёт поверх первого: итоговый вклад
                // рос как count^1.5, и файл, использующий чужие символы сотню
                // раз (vendored-код, широкий импорт), перетягивал карту, хотя
                // `√num_refs` задуман именно против этого.
                *weights.entry((*definer, *name)).or_insert(0.0) += weight;
            }
        }
    }

    let mut ranked: Vec<RankedDefinition> = weights
        .into_iter()
        .filter_map(|((rel_path, name), weight)| {
            let rank = file_ranks.get(rel_path).copied().unwrap_or(0.0);
            // Строка обязана существовать: ключ веса возник именно из тега,
            // значит и объявление где-то есть.
            let line = defined_at.get(name)?.get(rel_path).copied().unwrap_or(1);
            Some(RankedDefinition {
                rel_path: rel_path.to_string(),
                name: name.to_string(),
                line,
                rank: rank * weight,
            })
        })
        // Исключённые пути не ранжируются вовсе, а не идут в хвосте: агент
        // попросил их не видеть, и «последняя строка карты» была бы нарушением.
        .filter(|definition| !is_excluded(&definition.rel_path, &opts.exclude))
        .collect();

    // Сортировка по рангу с обратным порядком — единственное место, где
    // определяется порядок карты. Сравнение по пути и имени при равных рангах
    // делает результат воспроизводимым.
    ranked.sort_by(|a, b| {
        b.rank
            .partial_cmp(&a.rank)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.rel_path.cmp(&b.rel_path))
            .then_with(|| a.name.cmp(&b.name))
    });
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::codemap::graph;
    use crate::tools::codemap::tags::Tag;

    fn def(path: &str, name: &str) -> Tag {
        Tag::def(path, name, 1)
    }

    fn reference(path: &str, name: &str) -> Tag {
        Tag::reference(path, name, 1)
    }

    /// Два файла, где `b.rs` вызывает `core` из `a.rs`.
    fn sample() -> (Graph, Vec<Tag>) {
        let tags = vec![
            def("a.rs", "core"),
            def("b.rs", "entry"),
            reference("b.rs", "core"),
        ];
        (graph::build(&tags, &[], &[]), tags)
    }

    #[test]
    fn empty_graph_yields_empty_ranks() {
        assert!(pagerank(&Graph::default()).is_empty());
    }

    #[test]
    fn every_file_receives_a_rank() {
        let (graph, _) = sample();
        let ranks = pagerank(&graph);
        assert_eq!(ranks.len(), 2, "ранг обязан быть у каждого файла");
        assert!(ranks.values().all(|r| *r > 0.0));
    }

    #[test]
    fn ranks_are_comparable_across_files() {
        // Ранги разных файлов обязаны быть одной шкалы: иначе сортировка карты
        // теряла бы смысл. Сумма нормируется на единицу.
        let (graph, _) = sample();
        let total: f64 = pagerank(&graph).values().sum();
        assert!((total - 1.0).abs() < 1e-6, "сумма рангов обязана быть 1, получено {total}");
    }

    #[test]
    fn a_file_cited_by_many_outranks_an_isolated_one() {
        let mut tags = vec![def("core.rs", "alpha"), def("lonely.rs", "beta")];
        for i in 0..5 {
            tags.push(def(&format!("m{i}.rs"), "entry"));
            tags.push(reference(&format!("m{i}.rs"), "alpha"));
        }
        let graph = graph::build(&tags, &[], &[]);
        let ranks = pagerank(&graph);
        assert!(
            ranks["core.rs"] > ranks["lonely.rs"],
            "цитируемый файл обязан быть выше изолированного: core={} lonely={}",
            ranks["core.rs"],
            ranks["lonely.rs"]
        );
    }

    /// Самопетля не должна давать файлу преимущество: её задача — удержать
    /// файл в графе, а не поднять его над реальным центром проекта.
    #[test]
    fn isolated_file_does_not_accumulate_rank_from_its_self_loop() {
        let mut tags = vec![def("core.rs", "alpha"), def("lonely.rs", "beta")];
        for i in 0..3 {
            tags.push(def(&format!("m{i}.rs"), "entry"));
            tags.push(reference(&format!("m{i}.rs"), "alpha"));
        }
        let graph = graph::build(&tags, &[], &[]);
        let ranks = pagerank(&graph);
        let average = 1.0 / graph.files.len() as f64;
        assert!(
            ranks["lonely.rs"] < average,
            "никем не упомянутый файл обязан быть ниже среднего {} (получено {})",
            average,
            ranks["lonely.rs"]
        );
    }

    #[test]
    fn personalization_lifts_mentioned_file() {
        let tags = vec![
            def("a.rs", "alpha"),
            def("b.rs", "beta"),
            def("c.rs", "gamma"),
        ];
        let plain = pagerank(&graph::build(&tags, &[], &[]));
        let focused = pagerank(&graph::build(&tags, &[], &["c.rs".to_string()]));
        assert!(
            focused["c.rs"] > plain["c.rs"],
            "упомянутый файл обязан подняться: {:?} против {:?}",
            focused["c.rs"],
            plain["c.rs"]
        );
    }

    #[test]
    fn recursion_does_not_diverges() {
        // Взаимные вызовы — обычное дело в коде. Итерация обязана сойтись.
        let tags = vec![
            def("a.rs", "ping"),
            def("b.rs", "pong"),
            reference("a.rs", "pong"),
            reference("b.rs", "ping"),
        ];
        let graph = graph::build(&tags, &[], &[]);
        let ranks = pagerank(&graph);
        let total: f64 = ranks.values().sum();
        assert!((total - 1.0).abs() < 1e-6, "рекурсия обязана сходиться, сумма {total}");
    }

    /// ГЕЙТ 3: два прогона на одном дереве обязаны дать один порядок.
    #[test]
    fn gate3_pagerank_is_deterministic() {
        // Имена различаются, иначе одно имя, определённое сразу в сорока
        // файлах, породило бы полный граф — такой случай искусственный, и
        // проверял бы не детерминизм, а арифметику на вырожденных данных.
        let mut tags = Vec::new();
        for i in 0..30 {
            tags.push(def(&format!("core{i}.rs"), &format!("shared_{i}")));
            tags.push(def(&format!("app{i}.rs"), &format!("entry_{i}")));
            tags.push(reference(&format!("app{i}.rs"), &format!("shared_{i}")));
            if i > 0 {
                tags.push(reference(&format!("app{i}.rs"), &format!("entry_{i}")));
            }
        }
        let graph = graph::build(&tags, &[], &[]);
        let first = rank_definitions(&graph, &tags);
        let second = rank_definitions(&graph, &tags);
        assert_eq!(first, second, "порядок определений обязан совпадать между прогонами");
    }

    /// Определение, к которому ведёт ребро из другого файла, обязано попасть
    /// в ранжирование и иметь положительный ранг.
    #[test]
    fn definitions_are_ranked_by_their_file_weight() {
        let (graph, tags) = sample();
        let ranked = rank_definitions(&graph, &tags);
        let names: Vec<String> = ranked
            .iter()
            .map(|d| format!("{}:{}", d.rel_path, d.name))
            .collect();
        assert!(!ranked.is_empty(), "определения обязаны ранжироваться");
        let core = ranked
            .iter()
            .find(|d| d.name == "core")
            .unwrap_or_else(|| {
                panic!(
                    "core есть в графе, но не в ранжировании: ranked={names:?} edges={:?}",
                    graph.edges
                )
            });
        assert!(core.rank > 0.0, "у core обязан быть положительный ранг");

        // Ранг течёт ПО рёбрам, то есть к тому, от чего зависят. Значит выше
        // обязан оказаться вызываемый `a.rs`, а не вызывающий `b.rs`: карта
        // должна показывать ядро проекта, а не тех, кто его зовёт.
        let ranks = pagerank(&graph);
        assert!(
            ranks["a.rs"] > ranks["b.rs"],
            "вызываемый файл обязан быть выше вызывающего: {:?}",
            ranks
        );
    }

    #[test]
    fn ranked_list_is_sorted_descending() {
        let (graph, tags) = sample();
        let ranked = rank_definitions(&graph, &tags);
        assert!(
            ranked.windows(2).all(|w| w[0].rank >= w[1].rank),
            "список обязан идти по убыванию: {:?}",
            ranked.iter().map(|d| (&d.rel_path, &d.name)).collect::<Vec<_>>()
        );
    }

    // --- Vendored-даунвейт и самопетли ---------------------------------------

    /// Файл, использующий только собственные символы, N раз подряд.
    fn vendor_tags(count: usize) -> Vec<Tag> {
        let mut tags = vec![def("vendor/lib.rs", "vendored_entry_point")];
        for _ in 0..count {
            tags.push(reference("vendor/lib.rs", "vendored_entry_point"));
        }
        tags
    }

    /// Внутренние упоминания не поднимают определение над кросс-файловым
    /// интересом. Реальный случай, который это ловит: `astronomy.js` на
    /// king_orch_3 возглавлял карту благодаря 348 упоминаниям собственных
    /// символов, оттесняя ядро, на которое реально ссылается проект.
    #[test]
    fn self_references_do_not_lift_definitions_into_the_map() {
        let mut tags = vec![
            def("core.rs", "core_entry_function"),
            def("app.rs", "app_entry"),
            reference("app.rs", "core_entry_function"),
        ];
        tags.extend(vendor_tags(400));

        let graph = graph::build(&tags, &[], &[]);
        let ranked = rank_definitions(&graph, &tags);
        assert!(
            ranked.iter().all(|d| d.name != "vendored_entry_point"),
            "определение, никем внешне не упомянутое, не попадает в ранжирование: {ranked:?}"
        );
        let core = ranked
            .iter()
            .find(|d| d.name == "core_entry_function")
            .expect("определение с внешней ссылкой обязано ранжироваться");
        assert!(core.rank > 0.0, "у core обязан быть положительный ранг: {core:?}");
    }

    /// Vendored-файл, на который реально ссылаются, остаётся в карте — но ниже
    /// равного ему обычного файла. Ноль был бы ложью: путь всё-таки в графе.
    #[test]
    fn referenced_vendor_file_stays_ranked_but_below_an_equal_plain_file() {
        // Две одинаковые цепочки: app → core и app_v → vendor. Разница только
        // в пути второго файла.
        let tags = vec![
            def("plain/lib.rs", "plain_symbol_long"),
            def("app.rs", "app_entry"),
            reference("app.rs", "plain_symbol_long"),
            def("vendor/lib2.rs", "vendor_symbol_long"),
            reference("app.rs", "vendor_symbol_long"),
        ];
        let graph = graph::build(&tags, &[], &[]);
        let ranked = rank_definitions(&graph, &tags);
        let plain = ranked
            .iter()
            .find(|d| d.name == "plain_symbol_long")
            .expect("обычный путь обязан ранжироваться");
        let vendor = ranked
            .iter()
            .find(|d| d.name == "vendor_symbol_long")
            .expect("упомянутый снаружи vendored-файл не исчезает из карты");
        assert!(
            vendor.rank < plain.rank,
            "vendored-путь обязан стоять ниже равного обычного: {vendor:?} против {plain:?}"
        );
        assert!(
            vendor.rank > 0.0,
            "даунвейт — множитель, а не вычёркивание: {vendor:?}"
        );
    }

    /// Флаг `--boost-vendor` возвращает прежнее поведение: без понижения
    /// vendored-путь и обычный равны при равных связях.
    #[test]
    fn boost_vendor_option_restores_the_plain_ranking() {
        let tags = vec![
            def("plain/lib.rs", "plain_symbol_long"),
            def("app.rs", "app_entry"),
            reference("app.rs", "plain_symbol_long"),
            def("vendor/lib2.rs", "vendor_symbol_long"),
            reference("app.rs", "vendor_symbol_long"),
        ];
        let graph = graph::build(&tags, &[], &[]);
        let opts = RankOptions { boost_vendor: true, exclude: Vec::new() };
        let ranked = rank_definitions_opts(&graph, &tags, &opts);
        let plain = ranked
            .iter()
            .find(|d| d.name == "plain_symbol_long")
            .expect("обычный путь обязан ранжироваться");
        let vendor = ranked
            .iter()
            .find(|d| d.name == "vendor_symbol_long")
            .expect("vendored-файл обязан ранжироваться");
        assert!(
            (vendor.rank - plain.rank).abs() < 1e-9,
            "без даунвейта равные связи дают равный ранг: {vendor:?} против {plain:?}"
        );
    }

    /// Исключённый путь не ранжируется вовсе: не в хвосте, не с нулём.
    #[test]
    fn excluded_paths_are_dropped_from_ranking() {
        let tags = vec![
            def("src/core.rs", "core_entry_function"),
            def("tests/helpers.rs", "helper_symbol_long"),
            def("app.rs", "app_entry"),
            reference("app.rs", "core_entry_function"),
            reference("app.rs", "helper_symbol_long"),
        ];
        let graph = graph::build(&tags, &[], &[]);
        let opts = RankOptions {
            boost_vendor: false,
            exclude: vec!["tests/**".to_string()],
        };
        let ranked = rank_definitions_opts(&graph, &tags, &opts);
        assert!(
            ranked.iter().all(|d| !d.rel_path.starts_with("tests/")),
            "исключённый путь обязан исчезнуть из ранжирования: {ranked:?}"
        );
        assert!(
            ranked.iter().any(|d| d.name == "core_entry_function"),
            "остальные пути ранжируются как прежде: {ranked:?}"
        );
    }

    /// Признак vendored-пути: каталог в любой позиции и `.d.ts`.
    #[test]
    fn vendor_paths_are_recognized_by_directory_and_extension() {
        assert!(is_vendor_path("vendor/lib.rs"));
        assert!(is_vendor_path("src/vendor/lib.rs"));
        assert!(is_vendor_path("third_party/lib.c"));
        assert!(is_vendor_path("types/astronomy.d.ts"));
        assert!(!is_vendor_path("src/main.rs"));
        assert!(!is_vendor_path("src/vendor.rs"), "имя ФАЙЛА не признак каталога");
        assert!(!is_vendor_path("src/vendors/lib.rs"), "признак — точное имя каталога");
    }
}