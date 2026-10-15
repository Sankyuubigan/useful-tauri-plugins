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
    let file_ranks = pagerank(graph);
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
            for (name, count) in names {
                let Some(places) = defined_at.get(name) else { continue };
                if !places.contains_key(*definer) {
                    continue;
                }
                // Множитель `count` сохранён намеренно: повторное упоминание
                // имени в файле — это и есть интерес к этому определению,
                // и исходное поведение это учитывало. Умножение здесь, а не
                // перебор тегов, даёт то же число без квадратичной работы.
                *weights.entry((*definer, *name)).or_insert(0.0) += weight * (*count as f64);
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
}