//! Граф связей между файлами, как в Aider.
//!
//! Формулы перенесены из `aider/repomap.py` без изменений; здесь они собраны в
//! одном месте, чтобы правила ранжирования существовали в единственном
//! экземпляре, а не по копии на язык.
//!
//! ## Узлы и рёбра
//!
//! Узел — файл. Ребро `referencer → definer`: файл, где символ **упомянут**,
//! ссылается на файл, где он **объявлен**. Так сеть зависимостей получается
//! без анализа потоков управления: импорт и вызов дают одно и то же ребро.
//!
//! ## Почему вес ребра — `mul · √num_refs`
//!
//! `√num_refs` гасит частые упоминания: сто вызовов `assert!` не должны
//! перетянуть ранг на файл с тестами сильнее, чем один вызов в ядре.
//!
//! `mul` — множители важности, пороги взяты из Aider и объяснены в
//! [`importance`] дословно: они отражают то, что мы знаем о коде наверняка, а не
//! то, что хочется получить.

use std::collections::{HashMap, HashSet};

use super::tags::{Tag, TagKind};

/// Один файл с определениями и ссылками.
#[derive(Debug, Clone, Default)]
pub struct SymbolMap {
    /// Множество имён, определённых в файле. Множество, а не список: одно
    /// имя может быть объявлено дважды, и это не должно удваивать вес.
    pub defines: HashSet<String>,
    /// Сколько раз имя упомянуто в файле.
    pub references: HashMap<String, u32>,
}

/// Граф перед ранжированием.
#[derive(Debug, Clone, Default)]
pub struct Graph {
    /// Файлы, участвующие в графе.
    pub files: Vec<String>,
    /// Рёбра `(откуда, куда, вес)`. Пара может повторяться разными символами,
    /// поэтому это список, а не отображение.
    pub edges: Vec<(String, String, f64)>,
    /// Персонализация: во сколько раз поднимается ранг файла.
    pub personalization: HashMap<String, f64>,
}

/// Собрать граф по тегам индекса.
pub fn build(tags: &[Tag], chat_files: &[String], mentioned: &[String]) -> Graph {
    let mut by_file: HashMap<String, SymbolMap> = HashMap::new();
    // Порядок обхода фиксируется сортировкой: иначе порядок каталоговой
    // системы менял бы и граф, и его ранжирование.
    let mut ordered: Vec<&Tag> = tags.iter().collect();
    ordered.sort_by(|a, b| (&a.rel_path, a.kind, a.line).cmp(&(&b.rel_path, b.kind, b.line)));

    for tag in &ordered {
        let entry = by_file.entry(tag.rel_path.clone()).or_default();
        match tag.kind {
            TagKind::Def => {
                entry.defines.insert(tag.name.clone());
            }
            TagKind::Ref => {
                *entry.references.entry(tag.name.clone()).or_insert(0) += 1;
            }
        }
    }

    let mut files: Vec<String> = by_file.keys().cloned().collect();
    files.sort();

    let chat: HashSet<&String> = chat_files.iter().collect();
    let mentioned: HashSet<&String> = mentioned.iter().collect();

    let mut graph = Graph {
        files: files.clone(),
        edges: Vec::new(),
        personalization: HashMap::new(),
    };

    // Персонализация: 100/файлов на файл из чата и на упомянутый файл.
    // Константа 100 взята из Aider: она задаёт только масштаб, потому что
    // PageRank нормирует сумму.
    let personalize = 100.0 / files.len().max(1) as f64;
    for file in &files {
        let mut weight = 0.0;
        if chat.contains(file) {
            weight += personalize;
        }
        if mentioned.contains(file) {
            // `max`, а не `+`: файл одновременно из чата и упомянутый не
            // должен удваивать свой вес.
            weight = weight.max(personalize);
        }
        if weight > 0.0 {
            graph.personalization.insert(file.clone(), weight);
        }
    }

    // Проход по именам в стабильном порядке: итоговый граф обязан быть
    // одинаковым при двух запусках на одном дереве (ГЕЙТ 3).
    let mut names: HashSet<&String> = HashSet::new();
    for map in by_file.values() {
        names.extend(map.defines.iter());
    }
    let mut sorted_names: Vec<&String> = names.into_iter().collect();
    sorted_names.sort();

    for name in sorted_names {
        // Порядок обоих списков фиксируется сортировкой. Без неё рёбра
        // добавлялись бы в порядке обхода `HashMap`, который между прогонами
        // меняется, — и один и тот же проект давал бы разный граф (ГЕЙТ 3).
        let mut definers: Vec<&String> = by_file
            .iter()
            .filter(|(_, map)| map.defines.contains(name))
            .map(|(path, _)| path)
            .collect();
        definers.sort();
        if definers.is_empty() {
            continue;
        }

        let mul = importance(name, definers.len(), mentioned.contains(name));

        let mut referencers: Vec<(&String, u32)> = by_file
            .iter()
            .filter_map(|(path, map)| Some((path, map.references.get(name).copied()?)))
            .filter(|(_, count)| *count > 0)
            .collect();
        referencers.sort();

        let had_referencers = !referencers.is_empty();
        for (referencer, count) in referencers {
            let mut use_mul = mul;
            if chat.contains(referencer) {
                use_mul *= 50.0;
            }
            let weight = use_mul * (count as f64).sqrt();
            for definer in &definers {
                graph
                    .edges
                    .push((referencer.clone(), (*definer).clone(), weight));
            }
        }

        // Определение без единой ссылки не попадёт ни в одно ребро и выпадет
        // из графа целиком — вместе с файлом, где оно живёт. Самопетля малого
        // веса возвращает файл в граф, не выдумывая для него связей.
        //
        // Ранжирование самопетлю игнорирует (см. `super::rank`): её задача —
        // удержать файл, а не поднять его над центром проекта.
        if !had_referencers {
            for definer in &definers {
                graph
                    .edges
                    .push(((*definer).clone(), (*definer).clone(), 0.1));
            }
        }
    }

    graph
}

/// Множитель важности имени.
///
/// Пороги из Aider. Каждый отвечает на конкретный вопрос, а не «на всякий
/// случай»:
///
/// - ×10 за «длинное» имя — многословное имя (`get_user_by_id`) почти всегда
///   про публичную функцию, однобуквенное — про локальную деталь;
/// - ×0.1 за ведущее подчёркивание — приватное по соглашению, снаружи не нужно;
/// - ×0.1 при более чем пяти определениях — имя общее (`new`, `run`, `test`),
///   связать его с одним конкретным местом бессмысленно.
fn importance(name: &str, definer_count: usize, mentioned: bool) -> f64 {
    let mut mul = 1.0f64;

    let is_snake = name.contains('_');
    let is_kebab = name.contains('-');
    let is_camel = name.chars().any(char::is_uppercase) && name.chars().any(char::is_lowercase);
    if (is_snake || is_kebab || is_camel) && name.chars().count() >= 8 {
        mul *= 10.0;
    }
    if name.starts_with('_') {
        mul *= 0.1;
    }
    if definer_count > 5 {
        mul *= 0.1;
    }
    if mentioned {
        mul *= 10.0;
    }
    mul
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(path: &str, name: &str) -> Tag {
        Tag::def(path, name, 1)
    }

    fn reference(path: &str, name: &str) -> Tag {
        Tag::reference(path, name, 1)
    }

    #[test]
    fn edge_points_from_mentioning_file_to_defining_file() {
        let tags = vec![
            def("a.rs", "alpha"),
            def("b.rs", "beta"),
            reference("b.rs", "alpha"),
        ];
        let graph = build(&tags, &[], &[]);
        assert!(
            graph.edges.contains(&("b.rs".to_string(), "a.rs".to_string(), 1.0)),
            "b.rs упоминает alpha -> ребро должно идти в a.rs: {:?}",
            graph.edges
        );
    }

    /// Ссылка внутри одного файла — законное ребро: рекурсия это тоже связь.
/// Запрещено другое — выдумывать самопетлю, когда ссылки не было вовсе.
    #[test]
    fn reference_inside_one_file_is_a_real_edge_not_a_fabricated_loop() {
        let tags = vec![def("a.rs", "alpha"), reference("a.rs", "alpha")];
        let graph = build(&tags, &[], &[]);
        let loop_edge = graph
            .edges
            .iter()
            .find(|(from, to, _)| from == "a.rs" && to == "a.rs")
            .expect("вызов собственной функции — законное ребро");
        assert_eq!(
            loop_edge.2, 1.0,
            "настоящая ссылка должна весить как обычное ребро, а не как служебная самопетля 0.1"
        );
    }

    #[test]
    fn definition_without_references_keeps_a_weak_self_loop() {
        let tags = vec![def("a.rs", "lonely"), def("b.rs", "busy")];
        let graph = build(&tags, &[], &[]);
        let loop_edge = graph
            .edges
            .iter()
            .find(|(a, b, _)| a == "a.rs" && b == "a.rs")
            .expect("определение без ссылок должно остаться в графе");
        assert_eq!(loop_edge.2, 0.1, "вес самопетли обязан быть малым");
    }

    #[test]
    fn frequent_mentions_are_damped_by_sqrt() {
        // Девять упоминаний в ОДНОМ файле, а не девять файлов по одному:
        // затухание относится к частоте, а не к числу файлов.
        let mut all = vec![def("a.rs", "alpha")];
        all.extend((0..9).map(|_| reference("b.rs", "alpha")));
        let graph = build(&all, &[], &[]);
        let weight = graph
            .edges
            .iter()
            .find(|(from, to, _)| from == "b.rs" && to == "a.rs")
            .expect("ребро есть")
            .2;
        assert!((weight - 3.0).abs() < 1e-9, "√9 = 3, получено {weight}");
    }

    #[test]
    fn long_meaningful_name_outranks_private_short_one() {
        // `get_user_by_id` — многословное публичное имя; `_t` — приватная
        // деталь. Разница обязана быть кратной 100 (10 за имя, 0.1 за `_`).
        let mul_public = importance("get_user_by_id", 1, false);
        let mul_private = importance("_t", 1, false);
        assert!((mul_public / mul_private - 100.0).abs() < 1e-9);
    }

    #[test]
    fn name_defined_in_many_places_is_discounted() {
        let single = importance("handle_request", 1, false);
        let shared = importance("handle_request", 6, false);
        assert!((single / shared - 10.0).abs() < 1e-9);
    }

    #[test]
    fn mentioned_name_is_boosted() {
        assert!((importance("alpha", 1, true) / importance("alpha", 1, false) - 10.0).abs() < 1e-9);
    }

    /// ГЕЙТ 3: один и тот же вход обязан давать один и тот же граф.
    #[test]
    fn gate3_graph_is_deterministic() {
        let mut tags = Vec::new();
        for i in 0..40 {
            tags.push(def(&format!("f{i}.rs"), "alpha"));
            tags.push(reference(&format!("g{i}.rs"), "alpha"));
            tags.push(def(&format!("g{i}.rs"), "alpha"));
        }
        let first = build(&tags, &[], &[]);
        let second = build(&tags, &[], &[]);
        assert_eq!(first.edges.len(), second.edges.len());
        assert_eq!(first.edges, second.edges, "порядок рёбер обязан совпадать");
        assert_eq!(first.personalization, second.personalization);
    }

    #[test]
    fn chat_files_get_personalization() {
        let tags = vec![def("a.rs", "alpha"), def("b.rs", "beta")];
        let graph = build(&tags, &["a.rs".to_string()], &[]);
        assert!(graph.personalization.contains_key("a.rs"));
        assert!(!graph.personalization.contains_key("b.rs"));
    }

    #[test]
    fn mentioned_and_chat_file_is_not_double_counted() {
        let tags = vec![def("a.rs", "alpha"), def("b.rs", "beta")];
        let both = build(&tags, &["a.rs".to_string()], &["a.rs".to_string()]);
        let chat_only = build(&tags, &["a.rs".to_string()], &[]);
        assert_eq!(
            both.personalization.get("a.rs"),
            chat_only.personalization.get("a.rs"),
            "файл из чата и упомянутый не должен удваивать вес"
        );
    }

    #[test]
    fn empty_index_produces_empty_graph() {
        let graph = build(&[], &[], &[]);
        assert!(graph.files.is_empty());
        assert!(graph.edges.is_empty());
    }
}