//! `codemap` — карта репозитория: AST → лексика → граф.
//!
//! Универсальный инструмент для ИИ-агентов кодеров. Принимает путь к любой
//! папке, строит индекс и отдаёт сжатую карту проекта, поиск по символам и
//! обратные ссылки.
//!
//! ## Что уже сделано и что ещё предстоит
//!
//! Слой парсинга ([`tags`], [`lang`], [`rust_lang`], [`ts_lang`], [`py_lang`])
//! и плоский запасной путь ([`flat_lang`]) готовы. Поверх них построены граф
//! ([`graph`], [`rank`]), рендер под бюджет ([`render`]), поиск ([`search`]),
//! хранение ([`store`]) и оба режима вызова — CLI ([`cli`]) и MCP ([`mcp`]).
//!
//! ## Почему три бэкенда, а не один
//!
//! Tree-sitter и ast-grep отклонены: обе ветки требуют Си-компилятора —
//! 28 грамматик плюс рантайм у ast-grep, у `tree-sitter-rust` вообще нет фичи,
//! отключающей сборку. WASM-грамматики отклонены отдельно: они собираются как
//! emscripten side-модули и требуют общего линейного пространства памяти с
//! рантаймом, чего не умеют ни `wasmi`, ни `wasmtime`.
//!
//! Взамен каждый бэкенд отдаёт один и тот же [`tags::Tag`], поэтому различия
//! языков не расходятся дальше этой границы: правила ранжирования и рендера
//! существуют в одном экземпляре.

pub mod body;
pub mod clock;
pub mod cli;

pub use cli::run;

pub mod flat_lang;
pub mod graph;
pub mod inventory;
pub mod lang;
pub mod lookup;
pub mod mcp;
pub mod model;
pub mod parse;
pub mod py_lang;
pub mod rank;
pub mod render;
pub mod rust_lang;
pub mod scan;
pub mod skeleton;
pub mod search;
pub mod store;
pub mod syntax;
pub mod tags;
pub mod text;
pub mod tokens;
pub mod ts_lang;