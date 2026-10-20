//! `code` — доступ к ядру кодо-тулов (файлы, поиск, shell) из CLI agent-tools.
//!
//! Ядро реализаций — крейт `ko-code-tools` (SSOT, тот же, что использует
//! хост King Orch): тула `read_file`, `read_many_files`, `write_file`,
//! `edit_file`, `grep`, `glob`, `list_directory`, `bash`, `run_tests`.
//!
//! Два режима вызова:
//! - CLI ([`cli`]): `agent-tools code call <тул> --args '<json>'`;
//! - MCP ([`mcp`]): `agent-tools code mcp` — JSON-RPC построчно по stdin/stdout.
//!
//! Политика записи: авто-запись внутри корня (`--root`, по умолчанию текущий
//! каталог), запрет вне — дефолтный хук ядра [`AllowWithinRoot`]. Разграничение
//! по сессиям/плашкам — зона ответственности хоста, CLI его не дублирует.

pub mod cli;
pub mod mcp;

pub use cli::run;
