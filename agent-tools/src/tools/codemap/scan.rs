//! Обход каталога: что попадёт в индекс, а что нет.
//!
//! Решение о пропуске принимается **здесь и только здесь**. Бэкенды занимаются
//! разбором содержимого и не знают ничего про каталоги: иначе правила
//! исключений разъехались бы по трём бэкендам, а «почему этот файл не в
//! индексе» перестало бы иметь ответ.
//!
//! ## Что пропускается и почему
//!
//! - Каталоги сборки и зависимостей (`target`, `node_modules`, `dist`, …):
//!   там нет исходников проекта, только артефакты.
//! - `.codemap` — собственный индекс инструмента. Без исключения он попал бы в
//!   следующую же сборку и разросся.
//! - `.gitignore` репозитория: пользователь уже решил, что не является частью
//!   проекта. Повторное решение здесь было бы спором с ним без оснований.
//! - Файлы крупнее [`MAX_FILE_BYTES`]: разбор минимификата на несколько
//!   мегабайт съедает время агента, а карты из него всё равно не выйдет.
//!
//! ## Почему обход НЕ читает файлы
//!
//! Обход спрашивает только метаданные: размер и `mtime`. Содержимое читает
//! [`super::store::build`] — и только для файлов, у которых `mtime` разошёлся
//! с индексом.
//!
//! Это не оптимизация ради оптимизации, а условие, при котором вообще возможна
//! проверка свежести. Пока обход читает все файлы, свежесть индекса нельзя
//! проверить дёшево, а значит `map` обязан либо верить индексу на слово, либо
//! дорого перечитывать проект при каждом вызове. Оба варианта приводят к
//! одному: удалённый файл остаётся в карте, а новый в неё не попадает.
//!
//! ## Чего обход НЕ делает
//!
//! Не определяет бинарность по содержимому и не угадывает кодировку. Это делает
//! [`super::store::build`] при чтении: нечитаемый файл уходит в плоский индекс
//! с записью в лог — вместо того чтобы молча выпасть.
//!
//! ## Почему пропущенные каталоги записываются, а не просто не читаются
//!
//! Обход увидел `node_modules` и `tasks/` и не зашёл внутрь. Если об этом не
//! сказать, агент заключит, что таких папок в проекте нет, и полезет искать
//! модуль в каталоге, который инструмент даже не открывал. Поэтому каждый
//! пропуск попадает в [`Scan::ignored`] и всплывает в карте одной строкой.
//!
//! Счётчика файлов внутри пропущенного каталога сводка не содержит **сознательно**:
//! на реальном репозитории в `.gitignore`-каталогах лежат сотни тысяч файлов,
//! и подсчёт означал бы второй обход того, что обход решил не смотреть.

use std::fs;
use std::path::Path;

use super::lang::Lang;
use super::model::{FileEntry, IgnoredDir, IgnoreReason};

/// Каталоги, которые не содержат исходников проекта.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    ".git",
    ".svn",
    ".hg",
    ".idea",
    ".vscode",
    "__pycache__",
    ".venv",
    "venv",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    ".agents_workspace",
    ".codemap",
];

/// Имена файлов, которые не несут смысла для агента.
const SKIP_NAMES: &[&str] = &[
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "Cargo.lock",
    "poetry.lock",
    "uv.lock",
];

/// Порог размера файла для разбора, в байтах.
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// Результат обхода: что попало в индекс и что было увидено, но не раскрыто.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scan {
    /// Файлы индекса, отсортированные по пути.
    pub files: Vec<FileEntry>,
    /// Каталоги, в которые обход не заходил.
    ///
    /// Отдельное поле, а не пустое место среди `files`: агент обязан знать, что
    /// папка существует и её сознательно не смотрели. Молча пропущенный каталог
    /// выглядел бы как «его тут нет».
    pub ignored: Vec<IgnoredDir>,
}

/// Обойти каталог и собрать список файлов индекса.
///
/// Возвращает файлы, отсортированные по пути: порядок не зависит от
/// порядка чтения каталоговой системы, иначе один и тот же репозиторий давал
/// бы разные индексы на разных машинах.
pub fn walk(root: &Path) -> Result<Scan, String> {
    let mut ignore = GitIgnore::load(root);
    let mut scan = Scan::default();
    visit(root, root, &mut ignore, &mut scan)?;
    scan.files.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    scan.ignored.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(scan)
}

/// Рекурсивный обход одного каталога.
fn visit(dir: &Path, root: &Path, ignore: &mut GitIgnore, scan: &mut Scan) -> Result<(), String> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            // Каталог мог исчезнуть между обходом и чтением — это не повод
            // ронять всю сборку индекса из-за одного каталога.
            log::warn!("codemap: не прочитать {}: {e}", dir.display());
            return Ok(());
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                log::warn!("codemap: элемент каталога не прочитан в {}: {e}", dir.display());
                continue;
            }
        };
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let rel = relative(root, &path);

        let file_type = match entry.file_type() {
            Ok(kind) => kind,
            Err(e) => {
                log::warn!("codemap: тип {} не определён: {e}", path.display());
                continue;
            }
        };

        if file_type.is_dir() {
            if let Some(reason) = skip_reason(&name, &rel, ignore) {
                scan.ignored.push(IgnoredDir { path: rel, reason });
                continue;
            }
            visit(&path, root, ignore, scan)?;
            continue;
        }

        if !file_type.is_file() {
            continue;
        }
        if SKIP_NAMES.contains(&name.as_str()) || ignore.is_ignored(&rel, false) {
            continue;
        }
        match describe(&rel, &path) {
            Ok(Some(entry)) => scan.files.push(entry),
            Ok(None) => {}
            Err(reason) => log::warn!("codemap: {reason}"),
        }
    }
    Ok(())
}

/// Почему каталог не раскрывается, либо `None`, если раскрывать надо.
///
/// Список артефактов проверяется раньше `.gitignore`: для каталога вроде
/// `node_modules` причина «здесь только зависимости» полезнее, чем «пользователь
/// написал правило», и обе причины приводят к одному и тому же действию.
fn skip_reason(name: &str, rel: &str, ignore: &GitIgnore) -> Option<IgnoreReason> {
    if SKIP_DIRS.contains(&name) {
        return Some(IgnoreReason::Build);
    }
    if ignore.is_ignored(rel, true) {
        return Some(IgnoreReason::GitIgnore);
    }
    None
}

/// Собрать запись индекса для одного файла — **только по метаданным**.
///
/// Файл здесь НЕ читается. Обход спрашивает у файла ровно две вещи: размер и
/// дату изменения. Обе отдаёт `fs::metadata` без единого байта ввода-вывода.
///
/// Почему так важно: раньше обход читал каждый файл целиком (строки, токены),
/// и только потом `store::build` сравнивал `mtime` и для неизменившихся файлов
/// **выбрасывал напрасно прочитанное**. На репозитории в 361 файл это были
/// сотни мегабайт чтения впустую — «тёплый» индекс стоил 482 мс вместо
/// десятков. Теперь содержимое читает `store::build` и только там, где `mtime`
/// действительно разошёлся.
///
/// `lines` и `tokens` здесь нулевые: их проставляет
/// [`super::store::build`] для реально разобранного файла. Для неизменившегося
/// файла они приходят из кеша вместе с готовыми тегами.
///
/// `Ok(None)` — файл сознательно неиндексируем (слишком большой). Это не
/// ошибка и не повод для лога: правило размера известно заранее.
fn describe(rel_path: &str, path: &Path) -> Result<Option<FileEntry>, String> {
    let meta = fs::metadata(path).map_err(|e| format!("не получить свойства {}: {e}", path.display()))?;
    if meta.len() == 0 || meta.len() > MAX_FILE_BYTES {
        return Ok(None);
    }
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let extension = rel_path
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();

    Ok(Some(FileEntry {
        rel_path: rel_path.to_string(),
        lang: Lang::of_extension(&extension),
        bytes: meta.len(),
        lines: 0,
        tokens: 0,
        mtime_secs: mtime,
        parsed: false,
    }))
}

/// Путь относительно корня, всегда со `/` вместо `\`.
fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

/// Правила `.gitignore` корня.
pub struct GitIgnore {
    rules: Vec<Rule>,
}

struct Rule {
    pattern: String,
    /// Правило-исключение `!` — отменяет предыдущие.
    negated: bool,
    /// Правило относится только к каталогам.
    dir_only: bool,
}

impl GitIgnore {
    /// Прочитать `.gitignore` корня. Отсутствие файла — обычный случай,
    /// не ошибка.
    pub fn load(root: &Path) -> GitIgnore {
        let rules = fs::read_to_string(root.join(".gitignore"))
            .map(|text| parse_rules(&text))
            .unwrap_or_default();
        GitIgnore { rules }
    }

    /// Пусто ли правил — быстрая проверка на горячем пути.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Исключён ли путь.
    pub fn is_ignored(&self, rel_path: &str, is_dir: bool) -> bool {
        if self.rules.is_empty() {
            return false;
        }
        let mut ignored = false;
        for rule in &self.rules {
            if rule.dir_only && !is_dir {
                continue;
            }
            if matches(&rule.pattern, rel_path) {
                // Позднее правило выигрывает: так же работает и сам git.
                ignored = !rule.negated;
            }
        }
        ignored
    }
}

/// Разобрать текст `.gitignore`.
fn parse_rules(text: &str) -> Vec<Rule> {
    let mut rules = Vec::new();
    for raw in text.lines() {
        let mut line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let negated = line.starts_with('!');
        if negated {
            line = line.trim_start_matches('!');
        }
        let dir_only = line.ends_with('/');
        let pattern = line.trim_end_matches('/');
        if !pattern.is_empty() {
            rules.push(Rule {
                pattern: pattern.to_string(),
                negated,
                dir_only,
            });
        }
    }
    rules
}

/// Сопоставление одного шаблона с путём.
///
/// Поддерживается подмножество gitignore, достаточное для реальных проектов:
/// якорь в начале, `*` внутри компонента и `**` для «на любом уровне».
/// Полный синтаксис gitignore не воспроизводится — и это осознанно: лишний
/// файл в индексе лучше, чем молча выпавший файл, который агент потом не найдёт.
fn matches(pattern: &str, rel_path: &str) -> bool {
    let anchored = pattern.starts_with('/');
    let pattern = pattern.trim_start_matches('/');

    if pattern.contains("**") {
        return double_star_matches(pattern, rel_path);
    }

    let pattern_parts: Vec<&str> = pattern.split('/').collect();
    let path_parts: Vec<&str> = rel_path.split('/').collect();

    if anchored {
        // Якорь означает «от начала пути» — совпадение обязано быть полным.
        pattern_parts.len() == path_parts.len()
            && pattern_parts
                .iter()
                .zip(path_parts.iter())
                .all(|(p, q)| component_matches(p, q))
    } else if pattern_parts.len() == 1 {
        // Шаблон из одного компонента совпадает с любым компонентом пути.
        // Иначе правило `build` не исключало бы `src/build/out.rs`.
        path_parts
            .iter()
            .any(|q| component_matches(pattern_parts[0], q))
    } else {
        // Хвост пути: `src/main.rs` совпадает и с `a/src/main.rs`.
        if pattern_parts.len() > path_parts.len() {
            return false;
        }
        let offset = path_parts.len() - pattern_parts.len();
        pattern_parts
            .iter()
            .zip(path_parts.iter().skip(offset))
            .all(|(p, q)| component_matches(p, q))
    }
}

/// `**` — «на любом уровне вложенности».
fn double_star_matches(pattern: &str, rel_path: &str) -> bool {
    let path_parts: Vec<&str> = rel_path.split('/').collect();

    if let Some(rest) = pattern.strip_prefix("**/") {
        // `**/gen` означает «каталог gen на любом уровне», поэтому совпадение
        // ищется как хвост пути, а не полное совпадение.
        let pattern_parts: Vec<&str> = rest.split('/').collect();
        if pattern_parts.len() > path_parts.len() {
            return false;
        }
        return path_parts.windows(pattern_parts.len()).any(|window| {
            window
                .iter()
                .zip(pattern_parts.iter())
                .all(|(actual, expected)| component_matches(expected, actual))
        });
    }

    if let Some(prefix) = pattern.strip_suffix("/**") {
        // `build/**` — всё, что внутри `build`.
        let prefix = prefix.trim_end_matches('/');
        return rel_path.starts_with(&format!("{prefix}/"));
    }

    // `a/**/b` и прочие серединные формы: `**` внутри компонента равносилен
    // `*`, дальше работает обычное сопоставление по компонентам.
    matches(&pattern.replace("**", "*"), rel_path)
}

/// Совпадение одного компонента пути с шаблоном компонента.
///
/// `*` внутри компонента не переходит через разделитель, потому что компонент
/// разделителя уже не содержит.
fn component_matches(pattern: &str, text: &str) -> bool {
    if !pattern.contains('*') {
        return pattern == text;
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    let first = parts[0];
    let last = parts[parts.len() - 1];
    text.len() >= first.len() + last.len() && text.starts_with(first) && text.ends_with(last)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("codemap_scan_{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("временный каталог");
        dir
    }

    fn write(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().expect("есть родитель")).expect("каталог");
        fs::write(path, body).expect("запись файла");
    }

    #[test]
    fn finds_sources_and_skips_build_directories() {
        let root = temp_root("walk");
        write(&root, "src/main.rs", "fn main() {}\n");
        write(&root, "node_modules/lib/index.js", "module.exports = 1;\n");
        write(&root, "target/debug/build.rs", "fn build() {}\n");
        write(&root, ".codemap/index.json", "{}\n");

        let found = walk(&root).expect("обход").files;
        let paths: Vec<&str> = found.iter().map(|f| f.rel_path.as_str()).collect();
        assert_eq!(paths, vec!["src/main.rs"], "ожидался только исходник, получили {paths:?}");
        let _ = fs::remove_dir_all(&root);
    }

    /// Пропущенные каталоги обязаны быть названы, иначе агент решит, что их нет.
    #[test]
    fn skipped_directories_are_reported_not_silently_dropped() {
        let root = temp_root("ignored");
        write(&root, "node_modules/lib/index.js", "module.exports = 1;\n");
        write(&root, "src/main.rs", "fn main() {}\n");
        let scan = walk(&root).expect("обход");
        let paths: Vec<&str> = scan.ignored.iter().map(|d| d.path.as_str()).collect();
        assert_eq!(paths, vec!["node_modules"], "каталог обязан попасть в сводку");
        assert_eq!(scan.ignored[0].reason, IgnoreReason::Build);
        let _ = fs::remove_dir_all(&root);
    }

    /// Каталог, исключённый `.gitignore`, отличается от каталога артефактов:
    /// решение принял пользователь, а не инструмент.
    #[test]
    fn gitignored_directory_is_reported_as_such() {
        let root = temp_root("ignored_git");
        fs::write(root.join(".gitignore"), "/tasks\n").expect(".gitignore");
        write(&root, "tasks/plan.md", "# план\n");
        write(&root, "src/main.rs", "fn main() {}\n");
        let scan = walk(&root).expect("обход");
        assert!(
            scan.ignored.iter().any(|d| d.path == "tasks" && d.reason == IgnoreReason::GitIgnore),
            "каталог из .gitignore обязан быть виден с правильной причиной: {:?}",
            scan.ignored
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn respects_gitignore_including_negation() {
        let root = temp_root("ignore");
        fs::write(root.join(".gitignore"), "*.log\nbuild/\n!keep.log\n").expect(".gitignore");
        write(&root, "a.rs", "fn a() {}\n");
        write(&root, "debug.log", "noise\n");
        write(&root, "keep.log", "important\n");
        write(&root, "build/out.rs", "fn generated() {}\n");

        let paths: Vec<String> = walk(&root)
            .expect("обход")
            .files
            .into_iter()
            .map(|f| f.rel_path)
            .collect();
        assert!(paths.contains(&"a.rs".to_string()));
        assert!(!paths.contains(&"debug.log".to_string()), "*.log обязан исключаться");
        assert!(
            paths.contains(&"keep.log".to_string()),
            "правило с `!` обязано отменять исключение"
        );
        assert!(!paths.contains(&"build/out.rs".to_string()), "build/ обязан исключаться");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn double_star_matches_a_name_at_any_depth() {
        assert!(matches("**/gen", "src/gen"));
        assert!(matches("**/gen", "a/b/c/gen"));
        // Каталог `gen` исключает и всё, что внутри него, — как в git.
        assert!(matches("**/gen", "src/gen/deep.rs"));
        // Но не что-то с похожим именем.
        assert!(!matches("**/gen", "src/generate.rs"));
        assert!(matches("**/gen/*.rs", "src/gen/a.rs"));
    }

    #[test]
    fn trailing_double_star_covers_directory_contents() {
        assert!(matches("build/**", "build/a/b.rs"));
        assert!(!matches("build/**", "src/build/a.rs"));
    }

    #[test]
    fn plain_name_matches_any_component() {
        assert!(matches("target", "target"));
        assert!(matches("target", "a/b/target"));
        assert!(!matches("target", "a/b/targeted"));
    }

    #[test]
    fn anchored_pattern_does_not_match_suffix() {
        assert!(matches("/build", "build"));
        assert!(!matches("/build", "src/build"));
    }

    #[test]
    fn star_does_not_cross_directory_separator() {
        assert!(matches("*.rs", "main.rs"));
        assert!(!matches("src/*.rs", "src/deep/main.rs"));
    }

    #[test]
    fn comments_and_blank_lines_are_not_rules() {
        let rules = parse_rules("# комментарий\n\n*.tmp\n");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].pattern, "*.tmp");
    }

    #[test]
    fn empty_ignore_file_ignores_nothing() {
        let ignore = GitIgnore::load(Path::new("/путь/которого/нет"));
        assert!(ignore.is_empty());
        assert!(!ignore.is_ignored("a.rs", false));
    }

    /// Обход отдаёт МЕТАДАННЫЕ и ничего больше: размер, язык и дату изменения.
/// Размерность (строки, токены) проставляет разбор — иначе сверить индекс с
/// диском было бы нечем, а именно ради этого обход и не читает файлы.
#[test]
    fn walk_yields_metadata_without_reading_the_file() {
        let root = temp_root("entry");
        write(&root, "a.rs", "fn a() {}\nfn b() {}\n");
        let found = walk(&root).expect("обход").files;
        let entry = found.first().expect("один файл");
        assert_eq!(entry.lang, Lang::Rust);
        assert!(entry.bytes > 0, "размер берётся из метаданных");
        assert!(entry.mtime_secs > 0, "дата изменения обязана быть известна — по ней сверяется кеш");
        assert_eq!(entry.lines, 0, "обход не читает файл: строки проставит разбор");
        assert_eq!(entry.tokens, 0, "обход не читает файл: токены проставит разбор");
        assert!(!entry.parsed, "на обходе файл ещё не разобран");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn files_are_sorted_by_path() {
        let root = temp_root("sort");
        write(&root, "z.rs", "fn z() {}\n");
        write(&root, "a.rs", "fn a() {}\n");
        write(&root, "m/b.rs", "fn b() {}\n");
        let paths: Vec<String> = walk(&root).expect("обход").files.into_iter().map(|f| f.rel_path).collect();
        let mut sorted = paths.clone();
        sorted.sort();
        assert_eq!(paths, sorted, "обход обязан давать стабильный порядок");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn lockfiles_are_skipped() {
        let root = temp_root("locks");
        write(&root, "Cargo.lock", "[[package]]\n");
        write(&root, "a.rs", "fn a() {}\n");
        let paths: Vec<String> = walk(&root).expect("обход").files.into_iter().map(|f| f.rel_path).collect();
        assert_eq!(paths, vec!["a.rs".to_string()]);
        let _ = fs::remove_dir_all(&root);
    }
}