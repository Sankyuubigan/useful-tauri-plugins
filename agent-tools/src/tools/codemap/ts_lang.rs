//! TS / JS / TSX / JSX: `oxc_parser` для AST, `oxc_semantic` для символьной
//! таблицы.
//!
//! Это единственный из трёх бэкендов, который даёт **разрешённые** ссылки: у
//! `oxc` есть таблица символов и разрешение имён, поэтому ссылка приходит уже
//! связанной со своим определением, а не просто как вхождение имени.
//!
//! Ни `oxc_parser`, ни `oxc_semantic` не имеют build-зависимостей на Си: граф
//! зависимостей проверен рекурсивно по crates.io API, `cc` и `cmake` в нём
//! нет (в отличие от tree-sitter, где `cc` обязателен у каждой грамматики).

use std::collections::HashSet;

use oxc_allocator::Allocator;
use oxc_ast::AstKind;
use oxc_parser::Parser;
use oxc_semantic::{SemanticBuilder, SymbolFlags};
use oxc_span::{GetSpan, SourceType};

use super::lang::Lang;
use super::tags::Tag;
use super::text::LineIndex;

/// Разобрать исходник TS/JS-файла и собрать его теги.
///
/// Диагностики парсера не роняют файл, а попадают в лог: один синтаксический
/// сбой в середине репозитория не должен выбрасывать из карты весь файл. При
/// этом ошибка **логируется** — молча проглоченная диагностика была бы ложью
/// о качестве карты.
pub fn tags(rel_path: &str, source: &str, lang: Lang) -> Result<Vec<Tag>, String> {
    let source_type = source_type_of(lang, rel_path)?;

    // Порядок важен: `allocator` должен пережить и AST, и семантику, поэтому он
    // объявлен первым и умирает последним.
    let allocator = Allocator::default();
    let parse = Parser::new(&allocator, source, source_type).parse();

    if !parse.diagnostics.is_empty() {
        log::warn!(
            "codemap: {} — {} диагностик парсера, карта построена по восстановленному AST: {}",
            rel_path,
            parse.diagnostics.len(),
            parse.diagnostics[0]
        );
    }

    let built = SemanticBuilder::new_compiler()
        .with_build_nodes(true)
        .build(&parse.program);
    if !built.diagnostics.is_empty() {
        log::warn!(
            "codemap: {} — {} семантических диагностик: {}",
            rel_path,
            built.diagnostics.len(),
            built.diagnostics[0]
        );
    }
    let semantic = built.semantic;
    let lines = LineIndex::new(source);

    let mut out = Vec::new();
    let scoping = semantic.scoping();

    // Имена, для которых ссылки уже получены из таблицы символов. По ним
    // делается backfill для членов класса, иначе одна и та же ссылка была бы
    // посчитана дважды и завысила бы вес ребра.
    let mut resolved = HashSet::new();

    for symbol_id in scoping.symbol_ids() {
        let flags = scoping.symbol_flags(symbol_id);
        if !is_tracked(flags) {
            continue;
        }
        let name = scoping.symbol_name(symbol_id).to_string();
        if name.is_empty() || name == "arguments" {
            continue;
        }
        // Объявление — не то же самое, что отслеживаемый символ: импорт
        // отслеживается (его упоминания нужны графу), но объявлением не
        // является. Слить эти два понятия в одну проверку нельзя: тогда
        // вместе с «объявлением» теряются и ссылки на импортированное имя,
        // а это ровно те ссылки, из которых строится граф.
        if defines_here(flags) {
            let def_span = scoping.symbol_span(symbol_id);
            out.push(Tag::def(
                rel_path,
                &name,
                lines.line_of(def_span.start),
            ));
        }

        let mut has_any = false;
        for reference in semantic.symbol_references(symbol_id) {
            let span = semantic.reference_span(reference);
            out.push(Tag::reference(rel_path, &name, lines.line_of(span.start)));
            has_any = true;
        }
        if has_any {
            resolved.insert(name);
        }
    }

    // Члены класса в таблицу символов НЕ попадают — oxc держит их отдельно в
    // ClassTable. Без этого прохода все методы классов исчезли бы из карты, то
    // есть пропало бы ровно то, ради чего карта нужна: как устроен класс и кто
    // его вызывает.
    //
    // Поля класса сюда НЕ входят: `sessionId: string` — это состояние, а не
    // архитектура. Список полей в карте занимал столько же места, сколько
    // сигнатуры методов, и не отвечал ни на какой вопрос об устройстве кода.
    let mut members = HashSet::new();
    for node in semantic.nodes() {
        let AstKind::MethodDefinition(method) = node.kind() else {
            continue;
        };
        let Some(name) = method.key.static_name() else {
            continue;
        };
        let name = name.to_string();
        members.insert(name.clone());
        out.push(Tag::def(rel_path, &name, lines.line_of(method.span.start)));
    }

    // Ссылки на члены класса разрешение не даёт: свойство в `svc.run` — это
    // `IdentifierName`, а не `IdentifierReference`, и в таблицу ссылок оно не
    // попадает. Поэтому берём сами узлы `StaticMemberExpression`.
    //
    // Именно они, а не все `IdentifierName`: имя ключа метода и имён в
    // импортах тоже `IdentifierName`, и широкое сопоставление зачло бы
    // определение ссылкой на само себя.
    //
    // Это тот же приём backfill, что и в Aider для языков без резолвера.
    if !members.is_empty() {
        for node in semantic.nodes() {
            let AstKind::StaticMemberExpression(member) = node.kind() else {
                continue;
            };
            let name = member.property.name.as_str();
            if members.contains(name) && !resolved.contains(name) {
                out.push(Tag::reference(
                    rel_path,
                    name,
                    lines.line_of(member.property.span().start),
                ));
            }
        }
    }

    Ok(out)
}

/// Тип источника для oxc-парсера по расширению файла.
///
/// Доступен и слою проверки ([`super::syntax`]): там тот же парсер, и тип
/// источника обязан определяться одинаково, а не второй таблицей.
pub(crate) fn source_type_of(lang: Lang, rel_path: &str) -> Result<SourceType, String> {
    if lang != Lang::TsJs {
        return Err(format!("{rel_path}: ожидался TS/JS, получен {}", lang.name()));
    }
    let extension = rel_path
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    Ok(match extension.as_str() {
        "ts" => SourceType::ts(),
        "tsx" => SourceType::tsx(),
        "jsx" => SourceType::jsx(),
        "js" | "mjs" => SourceType::mjs(),
        "cjs" => SourceType::cjs(),
        "mts" => SourceType::ts(),
        "cts" => SourceType::cjs(),
        other => return Err(format!("{rel_path}: расширение .{other} не поддерживается")),
    })
}

/// Символ участвует в карте вообще — это не локальная переменная и не
/// параметр.
///
/// Исключены `FunctionScopedVariable` (это `var` **и параметры функций**) и
/// `CatchVariable`. Причина — не «шум», а отсутствие смысла: у параметра почти
/// нет ссылок, поэтому в графе он не получает веса и в карту не попадает, но
/// до ранжирования он занимает место в индексе и в графе.
///
/// Важно, что проверяется именно **пересечение пусто**: одиночная проверка
/// «равно ли флаги исключённому набору» пропустила бы параметр, потому что
/// `FunctionScopedVariable` != `FunctionScopedVariable | CatchVariable`.
///
/// Блочные `let`/`const` оставлены намеренно: среди них и переменные уровня
/// модуля, а отсевом невостребованных занимается PageRank и бюджет токенов —
/// ровно так же, как в Aider.
fn is_tracked(flags: SymbolFlags) -> bool {
    let local = SymbolFlags::FunctionScopedVariable | SymbolFlags::CatchVariable;
    !flags.is_empty() && flags.intersection(local).is_empty()
}

/// Символ объявлен **здесь, в этом репозитории** — то есть это не импорт.
///
/// `import { invoke }` — объявление в смысле языка, поэтому резолвер oxc
/// честно помечает его флагом `Import`, и обход обязан это учесть. Такое имя
/// ничего не определяет в проекте, но именно оно удерживало карту: на
/// king_orch_3 `src/controllers/chat.ts` начинался со списка из двадцати
/// импортов, и десяток «определений» в нём был чужой библиотекой.
///
/// Ссылки на импорт при этом сохраняются — см. [`is_tracked`]: точка импорта
/// законно упоминает символ, и без неё граф потерял бы ребро `файл → модуль`.
fn defines_here(flags: SymbolFlags) -> bool {
    let imported = SymbolFlags::Import | SymbolFlags::TypeImport;
    flags.intersection(imported).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::codemap::tags::TagKind;

    /// Нумерация строк значима для тестов, пустые строки не удалять.
    ///
    /// ```text
    ///  1 import { helper } from "./util";
    ///  2
    ///  3 export class Service {
    ///  4   run(input: string): number {
    ///  5     return helper(input);
    ///  6   }
    ///  7 }
    ///  8
    ///  9 const svc = new Service();
    /// 10
    /// 11 export function alpha(x: number): number {
    /// 12   const doubled = x * 2;
    /// 13   return doubled + svc.run("a");
    /// 14 }
    /// 15
    /// 16 export const beta = 7;
    /// ```
    const SOURCE: &str = r#"import { helper } from "./util";

export class Service {
  run(input: string): number {
    return helper(input);
  }
}

const svc = new Service();

export function alpha(x: number): number {
  const doubled = x * 2;
  return doubled + svc.run("a");
}

export const beta = 7;
"#;

    fn parse_ts() -> Vec<Tag> {
        tags("probe.ts", SOURCE, Lang::TsJs).expect("фикстура обязана разбираться")
    }

    fn def_line(tags: &[Tag], name: &str) -> u32 {
        tags.iter()
            .find(|t| t.kind == TagKind::Def && t.name == name)
            .unwrap_or_else(|| panic!("нет определения {name}"))
            .line
    }

    /// ГЕЙТ 2, бэкенд TS: и определения, и ссылки обязаны находиться.
    #[test]
    fn ts_backend_yields_both_defs_and_refs() {
        let tags = parse_ts();
        let defs = tags.iter().filter(|t| t.kind == TagKind::Def).count();
        let refs = tags.iter().filter(|t| t.kind == TagKind::Ref).count();
        assert!(defs > 0, "определения обязаны находиться");
        assert!(refs > 0, "ссылки обязаны находиться");
    }

    #[test]
    fn resolves_declarations_and_calls() {
        let tags = parse_ts();
        assert_eq!(def_line(&tags, "Service"), 3);
        assert_eq!(def_line(&tags, "run"), 4, "метод класса — отдельное определение");
        assert_eq!(def_line(&tags, "alpha"), 11);
        assert_eq!(def_line(&tags, "beta"), 16, "module-level const обязан попасть в карту");
        assert_eq!(def_line(&tags, "doubled"), 12, "блочный let сохраняется");

        // Метод не в таблице символов, ссылка на него добирается по имени —
        // ровно тот backfill, который применяет Aider для языков без резолвера.
        let run_ref = tags
            .iter()
            .find(|t| t.kind == TagKind::Ref && t.name == "run")
            .expect("вызов svc.run() обязан дать ссылку на метод");
        assert_eq!(run_ref.line, 13, "вызов svc.run() находится на строке 13");
    }

    #[test]
    fn resolved_references_are_not_counted_twice() {
        let tags = parse_ts();
        let doubled_refs = tags
            .iter()
            .filter(|t| t.kind == TagKind::Ref && t.name == "doubled")
            .count();
        // `doubled` разрешается таблицей символов, поэтому backfill по членам
        // класса не должен дублировать его ссылки. В исходнике одно обращение:
        // `const doubled = x * 2` — это объявление, а не ссылка.
        assert_eq!(doubled_refs, 1, "одно обращение к doubled в строке 13");
    }

    #[test]
    fn function_parameters_are_not_declarations() {
        let tags = parse_ts();
        // `input` и `x` — параметры функций: в графе у них нет смыслового веса.
        for parameter in ["input", "x"] {
            assert!(
                !tags.iter().any(|t| t.kind == TagKind::Def && t.name == parameter),
                "параметр {parameter} не должен попадать в определения"
            );
        }
    }

    #[test]
    fn tsx_is_not_parsed_as_plain_ts() {
        let source = "export const App = () => <div className=\"x\">hi</div>;\n";
        let tags = tags("probe.tsx", source, Lang::TsJs).expect("tsx обязан разбираться");
        assert_eq!(def_line(&tags, "App"), 1);
    }

    #[test]
    fn wrong_backend_is_an_error_not_a_silent_empty_result() {
        let err = tags("probe.ts", SOURCE, Lang::Rust)
            .expect_err("TS-файл не должен приниматься Rust-бэкендом");
        assert!(err.contains("probe.ts"), "ошибка обязана называть файл: {err}");
    }

    /// Синтаксическая ошибка не фатальна: файл обязан дойти до индекса, а
    /// диагностика — до лога. Пустой результат допустим и не считается ошибкой:
    /// тогда файл уходит в плоский индекс, и это проверяется отдельно.
    #[test]
    fn syntax_errors_do_not_turn_into_a_hard_error() {
        let broken = "function oops( { return; }\n";
        let tags = tags("broken.ts", broken, Lang::TsJs);
        assert!(
            tags.is_ok(),
            "синтаксическая ошибка обязана логироваться, а не ронять файл: {:?}",
            tags.err()
        );
    }

    /// Р2: импорт — не определение. Раньше `import { invoke }` попадал в карту
    /// как объявление, и верх карты состоял из чужих библиотек.
    #[test]
    fn imported_binding_is_not_a_definition() {
        let tags = parse_ts();
        assert!(
            !tags.iter().any(|t| t.kind == TagKind::Def && t.name == "helper"),
            "импортное имя обязано быть ссылкой, а не определением: {:?}",
            tags.iter().filter(|t| t.name == "helper").collect::<Vec<_>>()
        );
    }

    /// Ссылка на импортированное имя сохраняется: точка импорта — это
    /// законное упоминание символа, и без него граф теряет ребро в модуль.
    #[test]
    fn imported_name_is_still_referenced() {
        let tags = parse_ts();
        assert!(
            tags.iter().any(|t| t.kind == TagKind::Ref && t.name == "helper"),
            "импорт обязан остаться упоминанием, иначе пропадёт ребро графа"
        );
    }

    /// Р2: поле класса — состояние, а не архитектура. В карту идут заголовок
    /// класса и его методы, но не список полей.
    #[test]
    fn class_field_is_not_a_definition() {
        let source = "export class Session {\n  sessionId: string | null = null;\n  history: string[] = [];\n\n  run(v: string) {\n    return v;\n  }\n}\n";
        let tags = tags("fields.ts", source, Lang::TsJs).expect("фикстура обязана разбираться");
        for field in ["sessionId", "history"] {
            assert!(
                !tags.iter().any(|t| t.kind == TagKind::Def && t.name == field),
                "поле {field} не должно попадать в определения"
            );
        }
        // Заголовок класса и его метод остаются — иначе карта потеряла бы
        // ровно то, ради чего показывает класс.
        assert_eq!(def_line(&tags, "Session"), 1);
        assert_eq!(def_line(&tags, "run"), 5);
    }

    /// Тип-импорт (`import type`) — тот же случай, что и обычный: отдельного
    /// флага в графе у него нет, а в карте он занимал место так же.
    #[test]
    fn type_only_import_is_not_a_definition() {
        let source = "import type { Payload } from \"./types\";\n\nexport function alpha(): void {}\n";
        let tags = tags("types.ts", source, Lang::TsJs).expect("фикстура обязана разбираться");
        assert!(
            !tags.iter().any(|t| t.kind == TagKind::Def && t.name == "Payload"),
            "тип-импорт не должен быть определением"
        );
        assert_eq!(def_line(&tags, "alpha"), 3);
    }
}