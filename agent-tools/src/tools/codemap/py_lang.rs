//! Python: `rustpython-parser` плюс собственный обход AST.
//!
//! ## Почему не `ruff_python_parser`
//!
//! Ruff точнее, но тянет `stacker`, у которого **обязательный** build-dep на
//! `cc`: на Windows он компилирует `src/arch/windows.c`. Это ровно тот класс
//! C-зависимостей, ради которого весь инструмент собирается на чистом Rust.
//!
//! ## Почему обход свой, а не семантический анализ
//!
//! Полная таблица символов Python здесь не нужна: карте кода важно, какие
//! имена определены и где они упоминаются, а не какого типа переменная.
//! Разрешение имён появилось бы ценой C-зависимости, поэтому ссылки собираются
//! по именам — тот же приём backfill, что и в Aider, и в `rust_lang` для Rust.
//! Ложные совпадения отсекаются в [`super::graph`], где имена пересекаются с
//! множеством определений.

use std::convert::Infallible;

use rustpython_ast::fold::{fold_expr, fold_stmt, Fold};
use rustpython_ast::text_size::{TextRange, TextSize};
use rustpython_ast::{Expr, ExprContext, Ranged, Stmt};
use rustpython_parser::{ast, Parse};

use super::tags::Tag;
use super::text::LineIndex;

/// Распаковать результат с типом ошибки `Infallible`.
///
/// `Infallible` не населён, поэтому ветка `Err` недостижима по типам, а не по
/// договорённости: ошибиться на ровном месте обхода невозможно.
fn infallible<T>(result: Result<T, Infallible>) -> T {
    match result {
        Ok(value) => value,
        Err(never) => match never {},
    }
}

/// Разобрать исходник Python-файла и собрать его теги.
///
/// Синтаксическая ошибка здесь фатальна в отличие от TS: у `rustpython` нет
/// восстановления AST. Файл при этом не теряется — ошибка доходит до вызывающего
/// кода, который отправляет его в плоский индекс.
pub fn tags(rel_path: &str, source: &str) -> Result<Vec<Tag>, String> {
    let program = ast::Suite::parse(source, rel_path)
        .map_err(|e| format!("{rel_path}: не разобран как Python: {e}"))?;

    let lines = LineIndex::new(source);
    let mut collector = Collector {
        rel_path,
        lines: &lines,
        out: Vec::new(),
    };
    // Обход начинается с каждого верхнеуровневого оператора. Важно вызвать
    // именно МЕТОД трейта, а не свободную функцию: `fold_stmt` как функция
    // разбирает переданный узел на варианты и уходит вглубь, минуя
    // переопределение `Collector::fold_stmt`. Со свободной функцией верхний
    // уровень молча потерял бы все определения.
    for stmt in &program {
        infallible(collector.fold_stmt(stmt.clone()));
    }
    Ok(collector.out)
}

struct Collector<'a> {
    rel_path: &'a str,
    lines: &'a LineIndex,
    out: Vec<Tag>,
}

impl Collector<'_> {
    /// Отметить упоминанием имя, которое импорт приносит в проект.
    ///
    /// Берётся **исходное** имя, а не локальный псевдоним: псевдоним
    /// (`import numpy as np`) не объявлен больше нигде, поэтому ссылка на него
    /// не создала бы ни одного ребра графа. Ссылка должна называть то, что
    /// действительно определено в другом файле.
    ///
    /// Точка отбрасывается до последнего сегмента: `import a.b.c` приносит
    /// модуль `a.b.c`, и в качестве символа он называется `c`.
    fn push_bound(&mut self, bound: &str, line: u32) {
        let name = bound
            .rsplit('.')
            .next()
            .unwrap_or(bound)
            .trim_end_matches('*')
            .to_string();
        if name.is_empty() || name == "*" {
            return;
        }
        self.out.push(Tag::reference(self.rel_path, &name, line));
    }
}

impl Fold<TextRange> for Collector<'_> {
    type TargetU = TextRange;
    type Error = Infallible;
    type UserContext = ();

    fn will_map_user(&mut self, _user: &TextRange) -> Self::UserContext {}

    fn map_user(
        &mut self,
        user: TextRange,
        _context: Self::UserContext,
    ) -> Result<TextRange, Infallible> {
        Ok(user)
    }

    fn fold_stmt(&mut self, node: Stmt<TextRange>) -> Result<Stmt<TextRange>, Infallible> {
        let name = match &node {
            Stmt::FunctionDef(def) => Some(def.name.as_str()),
            Stmt::AsyncFunctionDef(def) => Some(def.name.as_str()),
            Stmt::ClassDef(def) => Some(def.name.as_str()),
            _ => None,
        };
        if let Some(name) = name {
            let line = self.lines.line_of(line_of(node.range().start()));
            self.out.push(Tag::def(self.rel_path, name, line));
        }

        // Импорт — это упоминание, а не объявление. Обход выражений его не
        // видит: псевдонимы в `from … import …` лежат в `Alias`, а не в
        // `Expr::Name`. Без этого кросс-модульных зависимостей Python не было
        // в графе вовсе, и файл с импортами выглядел независимым от проекта.
        match &node {
            Stmt::Import(import) => {
                let line = self.lines.line_of(line_of(node.range().start()));
                for alias in &import.names {
                    self.push_bound(&alias.name, line);
                }
            }
            Stmt::ImportFrom(from) => {
                let line = self.lines.line_of(line_of(node.range().start()));
                for alias in &from.names {
                    self.push_bound(&alias.name, line);
                }
            }
            _ => {}
        }

        // Рекурсия дальше: без неё вложенные функции и методы выпадали бы из
        // карты, а с ними — тело класса.
        fold_stmt(self, node)
    }

    fn fold_expr(&mut self, node: Expr<TextRange>) -> Result<Expr<TextRange>, Infallible> {
        match &node {
            // Присваивание и объявление — это не чтение. Их исключение
            // соответствует семантике `oxc`, где запись и чтение различаются,
            // и не даёт локальной переменной выглядеть использованием одноимённого
            // символа уровня репозитория.
            Expr::Name(name) => {
                if name.ctx != ExprContext::Store {
                    let line = self.lines.line_of(line_of(name.range().start()));
                    self.out.push(Tag::reference(self.rel_path, &name.id, line));
                }
            }
            // `obj.method()`: имя атрибута — тоже упоминание, и без него в
            // графе не было бы связей между методами и местом вызова.
            Expr::Attribute(attribute) => {
                let line = self.lines.line_of(line_of(attribute.range().start()));
                self.out
                    .push(Tag::reference(self.rel_path, &attribute.attr, line));
            }
            _ => {}
        }
        fold_expr(self, node)
    }
}

fn line_of(offset: TextSize) -> u32 {
    u32::from(offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::codemap::tags::TagKind;

    /// Нумерация строк значима для тестов, пустые строки не удалять.
    ///
    /// ```text
    ///  1 def alpha(x):
    ///  2     return x
    ///  3
    ///  4
    ///  5 class Beta:
    ///  6     def run(self, value):
    ///  7         return alpha(value)
    ///  8
    ///  9
    /// 10 def gamma():
    /// 11     return Beta().run(1)
    /// 12
    /// 13
    /// 14 GAMMA = gamma
    /// ```
    const SOURCE: &str = "\
def alpha(x):
    return x


class Beta:
    def run(self, value):
        return alpha(value)


def gamma():
    return Beta().run(1)


GAMMA = gamma
";

    fn parse() -> Vec<Tag> {
        tags("probe.py", SOURCE).expect("фикстура обязана разбираться")
    }

    fn def_line(tags: &[Tag], name: &str) -> u32 {
        tags.iter()
            .find(|t| t.kind == TagKind::Def && t.name == name)
            .unwrap_or_else(|| panic!("нет определения {name}"))
            .line
    }

    fn ref_lines(tags: &[Tag], name: &str) -> Vec<u32> {
        tags.iter()
            .filter(|t| t.kind == TagKind::Ref && t.name == name)
            .map(|t| t.line)
            .collect()
    }

    /// ГЕЙТ 2, бэкенд Python: и определения, и ссылки обязаны находиться.
    #[test]
    fn py_backend_yields_both_defs_and_refs() {
        let tags = parse();
        let defs = tags.iter().filter(|t| t.kind == TagKind::Def).count();
        let refs = tags.iter().filter(|t| t.kind == TagKind::Ref).count();
        assert!(defs > 0, "определения обязаны находиться");
        assert!(refs > 0, "ссылки обязаны находиться");
    }

    #[test]
    fn collects_module_and_nested_definitions() {
        let tags = parse();
        assert_eq!(def_line(&tags, "alpha"), 1);
        assert_eq!(def_line(&tags, "Beta"), 5);
        // Метод внутри класса — отдельное определение, а не часть класса.
        assert_eq!(def_line(&tags, "run"), 6, "метод обязан попасть в карту");
        assert_eq!(def_line(&tags, "gamma"), 10);
    }

    #[test]
    fn collects_name_and_attribute_references() {
        let tags = parse();
        assert_eq!(ref_lines(&tags, "alpha"), vec![7], "вызов alpha(value) — строка 7");
        assert!(
            ref_lines(&tags, "run").contains(&11),
            "вызов .run(1) должен дать ссылку на метод, {:?}",
            ref_lines(&tags, "run")
        );
        assert_eq!(ref_lines(&tags, "gamma"), vec![14]);
    }

    #[test]
    fn assignment_is_not_counted_as_a_read() {
        let tags = parse();
        // `return x` — чтение, `GAMMA = gamma` — запись. Символа `GAMMA` в
        // исходнике больше нигде нет, поэтому ссылок на него быть не должно.
        assert_eq!(ref_lines(&tags, "GAMMA"), Vec::<u32>::new());
    }

    #[test]
    fn syntax_error_is_an_error_not_a_panic() {
        let err = tags("broken.py", "def oops(:\n    pass\n")
            .expect_err("битый исходник обязан дать ошибку");
        assert!(
            err.contains("broken.py"),
            "ошибка обязана называть файл, иначе агент не поймёт, что чинить: {err}"
        );
    }

    #[test]
    fn empty_module_is_not_an_error() {
        let tags = tags("empty.py", "").expect("пустой файл — не ошибка");
        assert!(tags.is_empty());
    }

/// Р2: импорт в Python — упоминание, а не определение. Псевдонимы лежат в
    /// `Alias`, обход выражений их не видит, и без отдельного прохода граф не
    /// знал бы ни об одной кросс-модульной зависимости.
    ///
    /// Ссылка даётся на **исходное** имя: псевдоним не объявлен больше нигде и
    /// ребра графа не создал бы.
    #[test]
    fn imports_are_references_not_definitions() {
        let source = "from helpers import make_case, build_case as bc\nimport os.path\nimport numpy as np\n\n\ndef use():\n    return make_case()\n";
        let tags = tags("probe_imports.py", source).expect("фикстура обязана разбираться");
        for name in ["make_case", "build_case", "path", "numpy"] {
            assert!(
                !tags.iter().any(|t| t.kind == TagKind::Def && t.name == name),
                "импорт {name} не должен быть определением"
            );
            assert!(
                tags.iter().any(|t| t.kind == TagKind::Ref && t.name == name),
                "импорт {name} обязан дать ссылку"
            );
        }
        assert!(
            !tags.iter().any(|t| t.name == "bc"),
            "локальный псевдоним ребра не создаёт и в карту не годится"
        );
    }

    #[test]
    fn star_import_binds_nothing() {
        let source = "from helpers import *\n";
        let tags = tags("probe_star.py", source).expect("фикстура обязана разбираться");
        assert!(tags.iter().all(|t| t.name != "*"), "{tags:?}");
    }
}