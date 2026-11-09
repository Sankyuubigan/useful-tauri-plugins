//! Общий тип тегов для всех бэкендов парсинга.
//!
//! Это точка, где различия языков умирают. `syn`, `oxc` и `rustpython` отдают
//! один и тот же `Vec<Tag>`, а граф, ранжирование, рендер и поиск работают
//! поверх него одним кодом. Правила ранжирования существуют в одном экземпляре,
//! а не по копии на язык.
//!
//! Теги отдаются «сырыми»: бэкенд не решает, важна ли ссылка. Отбором
//! определений, с которыми ссылка что-то связывает, занимается [`super::graph`]
//! — так же, как в Aider, где пересечение `defines` и `references` считается
//! на уровне репозитория, а не файла.

use serde::{Deserialize, Serialize};

/// Что тег означает: определение символа или ссылка на него.
///
/// Порядок вариантов используется при сортировке тегов: `Def` идёт раньше `Ref`,
/// поэтому при равном файле и строке определение оказывается раньше ссылки.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TagKind {
    /// Символ объявлен здесь. `line` — строка, на которой написано имя.
    Def,
    /// Символ упомянут здесь. `line` — строка вхождения.
    Ref,
}

/// Одна найденная позиция символа в файле.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    /// Путь файла относительно корня индекса, всегда со `/` вместо `\`.
    pub rel_path: String,
    /// Имя символа ровно так, как оно написано в коде.
    pub name: String,
    /// Определение или ссылка.
    pub kind: TagKind,
    /// Номер строки, 1-based — как его показывают редакторы.
    pub line: u32,
}

impl Tag {
    /// Тег-определение. Короткий конструктор: бэкенды создают их пачками.
    pub fn def(rel_path: &str, name: &str, line: u32) -> Self {
        Self {
            rel_path: rel_path.to_string(),
            name: name.to_string(),
            kind: TagKind::Def,
            line,
        }
    }

    /// Тег-ссылка.
    pub fn reference(rel_path: &str, name: &str, line: u32) -> Self {
        Self {
            rel_path: rel_path.to_string(),
            name: name.to_string(),
            kind: TagKind::Ref,
            line,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_set_kind() {
        let d = Tag::def("a.rs", "foo", 3);
        let r = Tag::reference("a.rs", "foo", 9);
        assert_eq!(d.kind, TagKind::Def);
        assert_eq!(r.kind, TagKind::Ref);
        assert_eq!((d.line, r.line), (3, 9));
    }

    #[test]
    fn paths_are_owned_not_borrowed() {
        let path = String::from("src/main.rs");
        let tag = Tag::def(&path, "main", 1);
        drop(path);
        assert_eq!(tag.rel_path, "src/main.rs");
    }
}