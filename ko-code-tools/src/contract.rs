//! Контракт ядра: трейт тула, контекст исполнения, ошибки, реестр и утилиты.
//!
//! Каждый тул ядра — реализация [`CodeTool`]: имя, описание, JSON-схема, флаг
//! read-only и исполнение. Контекст [`ToolCtx`] даёт тулу рабочую директорию и
//! хук авторизации записи; политика записи (авто-зона, плашка, запрет) —
//! ответственность хоста, который реализует [`WriteAuthorizer`].

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::{fs, search, shell};

/// Ошибка тула. Все ветки завершаются честным сообщением (правило 2.2: тишина = ложь).
#[derive(Debug)]
pub enum ToolError {
    /// Неверные/недостающие аргументы вызова.
    Usage(String),
    /// Файл/директория не найдены.
    NotFound(String),
    /// Запрещено (вне корня / отклонено пользователем / запрещённая операция).
    Forbidden(String),
    /// Ошибка ввода/вывода.
    Io(String),
    /// Превышен таймаут.
    Timeout(String),
    /// Правка отклонена проверкой качества кода: новая синтаксическая ошибка.
    ///
    /// Отдельный класс нужен, чтобы отличать «правка сломала код» от «правка
    /// не выполнилась по другой причине»: в первом случае запись откатывается,
    /// во втором — остаётся.
    Lint(String),
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolError::Usage(m) => write!(f, "Неверные аргументы: {}", m),
            ToolError::NotFound(m) => write!(f, "Не найдено: {}", m),
            ToolError::Forbidden(m) => write!(f, "Запрещено: {}", m),
            ToolError::Io(m) => write!(f, "Ошибка ввода/вывода: {}", m),
            ToolError::Timeout(m) => write!(f, "Таймаут: {}", m),
            ToolError::Lint(m) => write!(f, "Правка отклонена проверкой качества кода: {}", m),
        }
    }
}

impl std::error::Error for ToolError {}

/// Хук авторизации записи (инверсия зависимостей, core rules §2.8).
///
/// Ядро не знает про плашки/UI/сессии хоста — решение «можно ли писать сюда»
/// принимает хост. Дефолтная реализация для CLI/тестов — [`AllowWithinRoot`].
pub trait WriteAuthorizer: Send + Sync {
    /// Разрешить ли запись по абсолютному пути от имени тула `tool`.
    fn authorize(&self, abs_path: &Path, tool: &str) -> Result<(), ToolError>;
}

/// Одна проблема, найденная проверкой качества кода.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintProblem {
    /// Номер строки (1-based), где проблема.
    pub line: u32,
    /// Текст проблемы.
    pub message: String,
}

/// Хук проверки качества кода после записи (инверсия зависимостей).
///
/// Ядро не знает про rust-analyzer, LSP и прочие серверы: «есть ли в изменённом
/// файле новая синтаксическая ошибка» решает хост, а ядро только спрашивает.
///
/// ## Контракт
///
/// - [`LintGate::snapshot`] — снимок состояния файла ДО записи (baseline);
/// - [`LintGate::check`] — проверка ПОСЛЕ записи: вернуть ТОЛЬКО новые
///   проблемы относительно baseline.
///
/// ## Три исхода (различать обязательно)
///
/// - `Ok(Some(проблемы))` — проверка выполнена; список новых проблем (пустой —
///   новых ошибок нет);
/// - `Ok(None)` — проверка НЕ выполнена (например, сервер проверки не
///   установлен). Это не «ошибок нет»: ядро сообщает об этом в ответе тула,
///   чтобы агент знал, что код не проверен;
/// - `Err(причина)` — проверка сорвалась с ошибкой.
pub trait LintGate: Send + Sync {
    /// Снимок состояния файла до записи.
    fn snapshot(&self, path: &Path) -> Result<Option<Vec<LintProblem>>, String>;

    /// Проверка после записи: новые проблемы относительно baseline.
    fn check(
        &self,
        path: &Path,
        baseline: &[LintProblem],
    ) -> Result<Option<Vec<LintProblem>>, String>;
}

/// Контекст исполнения тула: база путей чтения + хук записи.
pub struct ToolCtx<'a> {
    /// База для резолва относительных путей и чтения (рабочая директория).
    pub workspace_root: &'a Path,
    authorizer: &'a dyn WriteAuthorizer,
    lint_gate: Option<&'a dyn LintGate>,
}

impl<'a> std::fmt::Debug for ToolCtx<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolCtx")
            .field("workspace_root", &self.workspace_root)
            .field("lint_gate", &self.lint_gate.is_some())
            .finish_non_exhaustive()
    }
}

impl<'a> ToolCtx<'a> {
    pub fn new(workspace_root: &'a Path, authorizer: &'a dyn WriteAuthorizer) -> Self {
        Self {
            workspace_root,
            authorizer,
            lint_gate: None,
        }
    }

    /// Подключить проверку качества кода после записи.
    ///
    /// Без хука гейт просто не выполняется — это НЕ тихий пропуск: отсутствие
    /// гейта объявлено в описании тула, и CLI-режим честно о нём говорит.
    pub fn with_lint_gate(mut self, gate: &'a dyn LintGate) -> Self {
        self.lint_gate = Some(gate);
        self
    }

    /// Хук проверки качества, если он подключён.
    pub fn lint_gate(&self) -> Option<&'a dyn LintGate> {
        self.lint_gate
    }

    /// Централизованная проверка права записи по абсолютному пути — через хук хоста.
    pub fn authorize_write(&self, abs: &Path, tool: &str) -> Result<(), ToolError> {
        self.authorizer.authorize(abs, tool)
    }
}

/// Дефолтный авторизатор (CLI-режим, тесты): авто-запись только внутри корня.
///
/// Поведение зеркалит боевое правило хоста: внутри корня (включая новый файл —
/// родительский каталог внутри корня) разрешено; вне — честный запрет.
pub struct AllowWithinRoot {
    root: PathBuf,
}

impl AllowWithinRoot {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl WriteAuthorizer for AllowWithinRoot {
    fn authorize(&self, abs_path: &Path, _tool: &str) -> Result<(), ToolError> {
        if is_within_root(&self.root, abs_path) {
            return Ok(());
        }
        // Новый файл: сам путь может ещё не существовать — проверяем родителя.
        if let Some(parent) = abs_path.parent() {
            if is_within_root(&self.root, parent) {
                return Ok(());
            }
        }
        Err(ToolError::Forbidden(format!(
            "запись вне рабочей области '{}' запрещена (путь: {})",
            self.root.display(),
            abs_path.display()
        )))
    }
}

/// Контракт тула ядра. Хосты оборачивают его в свои трейты без изменения
/// имён/описаний/схем (паритет проверяется гейтом catalogs).
pub trait CodeTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    /// JSON-Schema параметров (формат `inputSchema` в промпте).
    fn parameters(&self) -> Value;
    /// `true` — тул ничего не меняет на диске (авто-доступ без подтверждения).
    fn is_readonly(&self) -> bool;
    fn execute(&self, args: &Value, ctx: &ToolCtx) -> Result<String, ToolError>;
}

/// SSOT-реестр: ВСЕ кодо-тулы ядра. Порядок обязан совпадать с порядком
/// реестра хоста (гейт каталога), чтобы выдача схем не менялась.
pub fn all_code_tools() -> Vec<Box<dyn CodeTool>> {
    vec![
        Box::new(fs::ReadFile),
        Box::new(fs::ReadManyFiles),
        Box::new(fs::WriteFile),
        Box::new(fs::EditFile),
        Box::new(search::Grep),
        Box::new(search::Glob),
        Box::new(search::ListDirectory),
        Box::new(shell::Bash),
        Box::new(shell::RunTests),
    ]
}

/// Схемы тулов для промпта/MCP: список `(имя, {description, inputSchema})`.
///
/// `include_write` = полный набор; иначе read-only-база, расширенная
/// `explicit`-именами мутаторов (аналог мета-набора `code_read` хоста).
pub fn schema_entries(include_write: bool, explicit: &[String]) -> Vec<(String, Value)> {
    all_code_tools()
        .into_iter()
        .filter(|tool| {
            let name = tool.name().to_string();
            explicit.contains(&name) || include_write || tool.is_readonly()
        })
        .map(|tool| {
            (
                tool.name().to_string(),
                serde_json::json!({
                    "description": tool.description(),
                    "inputSchema": tool.parameters(),
                }),
            )
        })
        .collect()
}

/// Разрешить путь (абсолютный или относительный корня) в абсолютный.
pub fn resolve_path(root: &Path, path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    }
}

/// Проверка: путь находится ВНУТРИ корня (или равен ему). Для read-тулов.
pub fn is_within_root(root: &Path, path: &Path) -> bool {
    let root_abs = normalize_canonical(root);
    let path_abs = normalize_canonical(path);
    path_abs.starts_with(&root_abs)
}

/// canonicalize с очисткой Windows-префикса `\\?\` (иначе `starts_with` ломается).
fn normalize_canonical(p: &Path) -> PathBuf {
    let c = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let s = c.to_string_lossy().to_string();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s).to_string();
    PathBuf::from(s)
}

/// Обрезать длинный текст до лимита с пометкой (для читаемых выводов тулов).
pub fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        text.to_string()
    } else {
        let head: String = text.chars().take(limit).collect();
        format!("{}…\n[вывод обрезан до {} символов]", head, limit)
    }
}

/// Взять первые `max_bytes` байт строки БЕЗ разрыва UTF-8 (граница по
/// `char_indices`). Для безопасной обрезки URL/логов/артефактов.
pub fn take_utf8_start(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = 0usize;
    for (idx, _) in s.char_indices() {
        if idx > max_bytes {
            break;
        }
        end = idx;
    }
    // end — граница последнего символа, полностью помещающегося в лимит.
    s[..end].to_string()
}

#[cfg(test)]
pub(crate) mod test_util {
    //! Общие тестовые утилиты ядра: синтетические папки и тестовый авторизатор
    //! (авто-запись только внутри корня — аналог боевой политики).

    use std::fs;
    use std::path::{Path, PathBuf};

    use super::AllowWithinRoot;

    /// Синтетическая папка: создаётся заново, гарантированно пуста.
    pub fn tmpdir(prefix: &str, name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("koct_{}_{}_{}", prefix, name, std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// Дефолтный авторизатор теста: авто-запись внутри `root`, запрет вне.
    /// Живёт в переменной теста (`let auth = auth_for(&d);`) — ToolCtx держит
    /// ссылку, поэтому объявлять auth нужно ДО и НЕ перемещать.
    pub fn auth_for(root: &Path) -> AllowWithinRoot {
        AllowWithinRoot::new(root.to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_path_handles_absolute_and_relative() {
        let root = Path::new("C:/proj");
        assert_eq!(resolve_path(root, "a/b.ts"), PathBuf::from("C:/proj/a/b.ts"));
        assert_eq!(resolve_path(root, "D:/other/c.ts"), PathBuf::from("D:/other/c.ts"));
    }

    #[test]
    fn truncate_marks_cut() {
        let t = truncate("abcdef", 4);
        assert!(t.contains("[вывод обрезан"));
        assert_eq!(truncate("abc", 10), "abc");
    }

    #[test]
    fn take_utf8_start_is_byte_budget_safe() {
        // Кириллица 'ю' = 2 байта: срез по байтам без char-границы не паникует.
        assert_eq!(take_utf8_start("ююю", 3), "ю"); // лимит 3 байта → 1 символ
        assert_eq!(take_utf8_start("ююю", 4), "юю");
        assert_eq!(take_utf8_start("ююю", 1), "");
        assert_eq!(take_utf8_start("hello world", 99), "hello world");
        assert_eq!(take_utf8_start("hello", 3), "hel");
        assert_eq!(take_utf8_start("", 5), "");
    }

    #[test]
    fn registry_has_no_duplicate_names() {
        let names: Vec<String> = all_code_tools().iter().map(|t| t.name().to_string()).collect();
        let unique: std::collections::HashSet<&String> = names.iter().collect();
        assert_eq!(names.len(), unique.len(), "дубли имён тулов в реестре ядра");
    }

    #[test]
    fn schema_filter_readonly_base_and_explicit() {
        let read_only: std::collections::HashSet<String> =
            schema_entries(false, &[]).into_iter().map(|(n, _)| n).collect();
        assert!(read_only.contains("read_file"));
        assert!(!read_only.contains("write_file"), "мутатор не входит в read-базу");

        let explicit: std::collections::HashSet<String> = schema_entries(
            false,
            &["bash".to_string()],
        )
        .into_iter()
        .map(|(n, _)| n)
        .collect();
        assert!(explicit.contains("bash"), "explicit добавляет мутатор");
        assert!(explicit.contains("read_file"), "readonly-база остаётся");
        assert!(!explicit.contains("write_file"), "write_file не добавлен");

        let full: std::collections::HashSet<String> =
            schema_entries(true, &[]).into_iter().map(|(n, _)| n).collect();
        assert!(full.contains("write_file"));
    }

    #[test]
    fn allow_within_root_denies_outside() {
        let root = test_util::tmpdir("contract", "allow");
        let other = test_util::tmpdir("contract", "other");
        let auth = AllowWithinRoot::new(root.clone());

        assert!(auth.authorize(&root.join("a.txt"), "write_file").is_ok());
        // Новый файл: родительская папка внутри корня — разрешено.
        assert!(auth.authorize(&root.join("sub/b.txt"), "write_file").is_ok());
        let err = auth.authorize(&other.join("c.txt"), "write_file").unwrap_err();
        assert!(matches!(err, ToolError::Forbidden(_)));
    }
}
