//! Rust-файлы: определения через `syn`, ссылки — обходом идентификаторов.
//!
//! Почему ссылки собираются обходом, а не резолвером: `syn` не строит таблицу
//! символов и не разрешает имена. Ровно такой же приём применяет сам Aider для
//! языков без резолвера — backfill ссылок по именам, когда `name.reference.*`
//! недоступен (aider/repomap.py, блок «We saw defs, without any refs»).
//!
//! Ссылки отдаются «сырыми»: попадает каждое употребление пути, а связь с
//! определением устанавливает [`super::graph`] пересечением множеств имён.
//!
//! Ни `syn`, ни `proc-macro2` не имеют build-зависимостей на Си: граф
//! зависимостей проверен рекурсивно по crates.io API.

use syn::visit::Visit;

use super::tags::Tag;

/// Номер строки в формуле хранения тега.
///
/// Насыщение, а не усечение: файл с четырьмя миллиардами строк не бывает, но
/// молчаливо обёрнутый номер указал бы на другую строку — то есть солгал бы
/// агенту о том, где находится символ.
fn line_u32(line: usize) -> u32 {
    u32::try_from(line).unwrap_or(u32::MAX)
}

/// Разобрать исходник Rust-файла и собрать его теги.
///
/// Ошибка возвращается строкой, а не паникуется: агент обязан увидеть, какой
/// файл не разобран, а не получить падение процесса без указания причины.
pub fn tags(rel_path: &str, source: &str) -> Result<Vec<Tag>, String> {
    let file = syn::parse_file(source).map_err(|e| format!("{rel_path}: не разобран как Rust: {e}"))?;

    let mut defs = DefCollector {
        rel_path,
        out: Vec::new(),
    };
    defs.visit_file(&file);

    let mut refs = RefCollector {
        rel_path,
        out: Vec::new(),
    };
    refs.visit_file(&file);

    let mut out = defs.out;
    out.append(&mut refs.out);
    Ok(out)
}

/// Собирает объявления: функции, структуры, перечисления, трейты, типы,
/// константы, модули и методы внутри `impl`/`trait`.
///
/// Обход рекурсивный, поэтому вложенные функции и методы тоже попадают в
/// карту — иначе большая часть тела крупного файла осталась бы невидимой.
struct DefCollector<'p> {
    rel_path: &'p str,
    out: Vec<Tag>,
}

impl DefCollector<'_> {
    fn push(&mut self, name: String, line: usize) {
        self.out.push(Tag::def(self.rel_path, &name, line_u32(line)));
    }
}

impl<'ast> Visit<'ast> for DefCollector<'_> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let ident = &node.sig.ident;
        self.push(ident.to_string(), ident.span().start().line);
        syn::visit::visit_item_fn(self, node);
    }

    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        let ident = &node.ident;
        self.push(ident.to_string(), ident.span().start().line);
        syn::visit::visit_item_struct(self, node);
    }

    fn visit_item_enum(&mut self, node: &'ast syn::ItemEnum) {
        let ident = &node.ident;
        self.push(ident.to_string(), ident.span().start().line);
        syn::visit::visit_item_enum(self, node);
    }

    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        let ident = &node.ident;
        self.push(ident.to_string(), ident.span().start().line);
        syn::visit::visit_item_trait(self, node);
    }

    fn visit_item_type(&mut self, node: &'ast syn::ItemType) {
        let ident = &node.ident;
        self.push(ident.to_string(), ident.span().start().line);
        syn::visit::visit_item_type(self, node);
    }

    fn visit_item_const(&mut self, node: &'ast syn::ItemConst) {
        let ident = &node.ident;
        self.push(ident.to_string(), ident.span().start().line);
        syn::visit::visit_item_const(self, node);
    }

    fn visit_item_static(&mut self, node: &'ast syn::ItemStatic) {
        let ident = &node.ident;
        self.push(ident.to_string(), ident.span().start().line);
        syn::visit::visit_item_static(self, node);
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let ident = &node.ident;
        self.push(ident.to_string(), ident.span().start().line);
        syn::visit::visit_item_mod(self, node);
    }

    /// `impl` даёт определение по **самому типу**, а не по ключевому слову:
    /// `impl Beta` — это объявление `Beta`, а не объявление `impl`.
    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        if let syn::Type::Path(path) = &*node.self_ty {
            if let Some(segment) = path.path.segments.last() {
                let ident = &segment.ident;
                self.push(ident.to_string(), ident.span().start().line);
            }
        }
        syn::visit::visit_item_impl(self, node);
    }

    /// Метод внутри трейта — отдельное определение, а не часть трейта.
    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        let ident = &node.sig.ident;
        self.push(ident.to_string(), ident.span().start().line);
        syn::visit::visit_trait_item_fn(self, node);
    }

    /// Метод внутри `impl` — отдельное определение.
    ///
    /// Это `ImplItemFn`, а не `ItemFn`: без отдельного посетителя все методы
    /// классов выпадали бы из карты, то есть исчезал бы ровно тот код, ради
    /// которого карту и строят.
    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        let ident = &node.sig.ident;
        self.push(ident.to_string(), ident.span().start().line);
        syn::visit::visit_impl_item_fn(self, node);
    }
}

/// Собирает употребления: пути в выражениях (`Beta::new`) и вызовы методов.
///
/// `qself` пропускается: у `T::fmt` путь-сегмент `fmt` — это не самостоятельный
/// символ уровня репозитория, и в графе он дал бы ложное ребро.
struct RefCollector<'p> {
    rel_path: &'p str,
    out: Vec<Tag>,
}

impl RefCollector<'_> {
    fn push(&mut self, name: String, line: usize) {
        self.out.push(Tag::reference(self.rel_path, &name, line_u32(line)));
    }
}

impl<'ast> Visit<'ast> for RefCollector<'_> {
    fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
        if node.qself.is_none() {
            if let Some(segment) = node.path.segments.last() {
                let ident = &segment.ident;
                self.push(ident.to_string(), ident.span().start().line);
            }
        }
        syn::visit::visit_expr_path(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let method = &node.method;
        self.push(method.to_string(), method.span().start().line);
        syn::visit::visit_expr_method_call(self, node);
    }

    /// `use` — это объявление зависимости, и без него граф Rust-проекта пуст.
    ///
    /// Имена `use` — не `ExprPath`, поэтому обход выражений их не видит вовсе.
    /// На реальном репозитории это означало, что ни одна кросс-модульная
    /// зависимость не попадала в граф: файл, который импортирует чужой модуль,
    /// выглядел для PageRank как тот, кто ни от кого не зависит, и ядро проекта
    /// не поднималось в карте никогда.
    ///
    /// Берётся **последнее** имя в дереве `use` — то есть тот символ, который
    /// реально приходит извне (`use crate::a::b::run` → `run`). Промежуточные
    /// сегменты пути не берутся: модуль, объявленный через `pub mod foo;`,
    /// определением не является и связать его было бы не с чем. Локальные
    /// псевдонимы тоже не берутся — они нигде не объявлены и ребра не создадут.
    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        let mut names = Vec::new();
        collect_bound_names(&node.tree, &mut names);
        for ident in names {
            self.push(ident.to_string(), ident.span().start().line);
        }
        syn::visit::visit_item_use(self, node);
    }
}

/// Имена, которые дерево `use` связывает с местом объявления.
fn collect_bound_names<'t>(tree: &'t syn::UseTree, out: &mut Vec<&'t syn::Ident>) {
    match tree {
        syn::UseTree::Path(path) => collect_bound_names(&path.tree, out),
        syn::UseTree::Name(name) => out.push(&name.ident),
        syn::UseTree::Rename(rename) => out.push(&rename.ident),
        // `use foo::*` не привязывает ни одного имени.
        syn::UseTree::Glob(_) => {}
        syn::UseTree::Group(group) => {
            for item in &group.items {
                collect_bound_names(item, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::codemap::tags::TagKind;

    /// Нумерация строк здесь значима: тесты проверяют именно её, поэтому
    /// пустые строки между блоками удалять нельзя.
    ///
    /// ```text
    ///  1 use std::fmt;
    ///  2
    ///  3 fn alpha() -> u32 { 1 }
    ///  4
    ///  5 struct Beta;
    ///  6
    ///  7 impl Beta {
    ///  8     fn new() -> Self { Self }
    ///  9 }
    /// 10
    /// 11 fn gamma(x: u32) -> u32 {
    /// 12     let b = Beta::new();
    /// 13     if x > 0 { alpha() } else { b }
    /// 14 }
    /// ```
    const SOURCE: &str = "\
use std::fmt;

fn alpha() -> u32 { 1 }

struct Beta;

impl Beta {
    fn new() -> Self { Self }
}

fn gamma(x: u32) -> u32 {
    let b = Beta::new();
    if x > 0 { alpha() } else { b }
}
";

    fn parse() -> Vec<Tag> {
        tags("probe.rs", SOURCE).expect("фикстура обязана разбираться")
    }

    fn def_line(tags: &[Tag], name: &str) -> u32 {
        tags.iter()
            .find(|t| t.kind == TagKind::Def && t.name == name)
            .unwrap_or_else(|| panic!("нет определения {name}"))
            .line
    }

    fn ref_line(tags: &[Tag], name: &str) -> u32 {
        tags.iter()
            .find(|t| t.kind == TagKind::Ref && t.name == name)
            .unwrap_or_else(|| panic!("нет ссылки на {name}"))
            .line
    }

    /// ГЕЙТ 1. `proc-macro2` вне процедурного макроса работает в fallback-режиме
    /// и исторически не обязан давать номера строк. От этого зависят и рендер
    /// карты, и ссылки, поэтому проверяется первым и на настоящем файле.
    #[test]
    fn gate1_line_numbers_are_true() {
        let tags = parse();
        assert_eq!(def_line(&tags, "alpha"), 3);
        assert_eq!(def_line(&tags, "Beta"), 5);
        assert_eq!(def_line(&tags, "gamma"), 11);
        assert_eq!(ref_line(&tags, "alpha"), 13, "вызов alpha() находится на строке 13");
        assert_eq!(ref_line(&tags, "new"), 12, "вызов Beta::new() находится на строке 12");
    }

    #[test]
    fn nested_items_are_collected() {
        let tags = parse();
        // Метод внутри `impl` — отдельное определение на своей строке.
        assert_eq!(def_line(&tags, "new"), 8);
    }

    /// ГЕЙТ 2, часть 1: на фикстуре defs и refs оба непустые.
    #[test]
    fn rust_backend_yields_both_defs_and_refs() {
        let tags = parse();
        let defs = tags.iter().filter(|t| t.kind == TagKind::Def).count();
        let refs = tags.iter().filter(|t| t.kind == TagKind::Ref).count();
        assert!(defs > 0, "определения обязаны находиться");
        assert!(refs > 0, "ссылки обязаны находиться");
    }

    /// ГЕЙТ 2, часть 2: то же самое на настоящем файле репозитория, а не на
    /// фикстуре. Фикстура может оказаться удачной случайно.
    #[test]
    fn gate2_real_rust_file_yields_defs_and_refs() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("tools")
            .join("codemap")
            .join("tags.rs");
        let source = std::fs::read_to_string(&path).expect("прочитать реальный файл репозитория");
        let tags = tags("codemap/tags.rs", &source).expect("реальный файл обязан разобраться");
        let defs = tags.iter().filter(|t| t.kind == TagKind::Def).count();
        let refs = tags.iter().filter(|t| t.kind == TagKind::Ref).count();
        assert!(defs > 0, "реальный файл обязан дать определения");
        assert!(refs > 0, "реальный файл обязан дать ссылки");
    }

    #[test]
    fn broken_source_reports_error_without_panicking() {
        let err = tags("broken.rs", "fn ((( nope").expect_err("битый исходник обязан дать ошибку");
        assert!(
            err.contains("broken.rs"),
            "ошибка обязана называть файл, иначе агент не поймёт, что чинить: {err}"
        );
    }

    #[test]
    fn relative_path_is_preserved_verbatim() {
        let tags = parse();
        assert!(tags.iter().all(|t| t.rel_path == "probe.rs"));
    }

    /// Р2: `use` — упоминание, а не определение. Импорт сам по себе ничего не
    /// объявляет в этом репозитории, но называет символ из другого файла.
    #[test]
    fn use_statement_yields_a_reference_not_a_definition() {
        let tags = parse();
        assert!(
            !tags.iter().any(|t| t.kind == TagKind::Def && t.name == "fmt"),
            "импортный путь не должен считаться определением"
        );
        assert!(
            tags.iter().any(|t| t.kind == TagKind::Ref && t.name == "fmt"),
            "импорт обязан дать ссылку, иначе граф не знает о зависимости"
        );
    }

    /// Кросс-модульный импорт — это и есть зависимость между файлами. Без него
    /// граф Rust-проекта пуст: файлы выглядят независимыми, а ядро проекта не
    /// может подняться в карте.
    ///
    /// Берётся исходное имя (`use …::beta as gamma` → `beta`), а не локальный
    /// псевдоним: псевдоним не объявлен больше нигде и ребра бы не создал.
    #[test]
    fn cross_module_use_links_the_file_to_its_dependency() {
        let source = "use crate::domain::orchestrator::dispatch::route_message;\n\nfn entry() {\n    route_message()\n}\n";
        let tags = tags("api/chat.rs", source).expect("фикстура обязана разбираться");
        let at = tags
            .iter()
            .find(|t| t.kind == TagKind::Ref && t.name == "route_message")
            .expect("импорт должен дать ссылку на route_message");
        assert_eq!(at.line, 1, "ссылка стоит на строке самого импорта");
    }

    #[test]
    fn grouped_and_renamed_use_items_are_all_collected() {
        let source = "use crate::a::{alpha, beta as gamma, *};\nuse crate::b::delta;\n";
        let tags = tags("mod.rs", source).expect("фикстура обязана разбираться");
        let refs: Vec<&str> = tags
            .iter()
            .filter(|t| t.kind == TagKind::Ref)
            .map(|t| t.name.as_str())
            .collect();
        assert!(refs.contains(&"alpha"), "{refs:?}");
        assert!(refs.contains(&"beta"), "переименование берёт исходное имя: {refs:?}");
        assert!(refs.contains(&"delta"), "{refs:?}");
        assert!(!refs.contains(&"*"), "звёздочка именем не является: {refs:?}");
    }
}