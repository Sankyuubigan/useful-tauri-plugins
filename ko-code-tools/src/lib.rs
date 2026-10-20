//! Дверь крейта: ядро кодо-тулов (SSOT).
//!
//! Реализация тулов кодинга существует ровно ОДИН раз — здесь. Хосты (King Orch)
//! получают тонкие адаптеры поверх [`contract::CodeTool`], CLI-набор
//! `agent-tools` — прямой интерфейс через сабкоманду `code`. Специфика хоста
//! (плашки разрешений, UI, сессии) не просачивается в ядро: право записи
//! решается хуком [`contract::WriteAuthorizer`] (инверсия зависимостей,
//! core rules §2.8).
//!
//! Тяжёлые AST-парсеры (syn, oxc, rustpython) в этом крейте ЗАПРЕЩЕНЫ — они
//! живут только в CLI-крейте `agent-tools` (см. codemap).

pub mod contract;
pub mod fs;
pub mod fuzzy;
pub mod pruner;
pub mod search;
pub mod shell;

pub use contract::{
    AllowWithinRoot, CodeTool, LintGate, LintProblem, ToolCtx, ToolError, WriteAuthorizer,
    all_code_tools, is_within_root, resolve_path, schema_entries, take_utf8_start, truncate,
};
