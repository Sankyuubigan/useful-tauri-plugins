//! Lint-gate по синтаксическому статусу файла — замена снятого с вооружения
//! LSP-гейта хоста (см. `agent-tools/README.md`, «Диагностика»).
//!
//! Подключается в [`super::cli::Cli::run_tool`] через `ToolCtx::with_lint_gate`:
//! ядро [`ko_code_tools`] снимает baseline проблем ДО записи и спрашивает
//! ПОСЛЕ записи. Источник проблем — тот же слой [`crate::tools::codemap::syntax`],
//! что у карты кода и команды `code check`: тот же парсер, тот же вопрос,
//! одинаковый ответ — иначе гейт врал бы про файлы, которые карта считает
//! рабочими (ключевой случай — TS с восстановлением AST).
//!
//! ## Семантика исходов (см. контракт `LintGate`)
//!
//! - `Ok(Some(проблемы))` — проверка выполнена. Проблема считается новой,
//!   только если её пара (строка, текст) не встречалась в baseline: ошибка,
//!   которая была и осталась, не блокирует правку.
//! - `Err(причина)` — проверить честно не можем: язык не поддерживается,
//!   файл был нечитаем/бинарным. Ядро печатает причину агенту и НЕ откатывает
//!   запись, но и не выставляет её как проверенную.
//! - `Ok(None)` здесь не используется: «сервер проверки недоступен» для
//!   синтакс-гейта не существует — у отказа всегда есть конкретная причина,
//!   и `Err` передаёт её без вранья об отсутствующем сервере.

use std::path::{Path, PathBuf};

use ko_code_tools::{LintGate, LintProblem};

use crate::tools::codemap::lookup::LookupError;
use crate::tools::codemap::syntax::{SyntaxStatus, inspect_file};

/// Гейт синтаксической проверки ядровых записей.
pub struct SyntaxGate {
    root: PathBuf,
}

impl SyntaxGate {
    /// Гейт работает в том же корне, что и CLI: ядро передаёт абсолютный
    /// путь проверяемого файла, слой [`inspect_file`] сверяет его с корнем.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Статус файла, который сейчас лежит на диске по этому пути.
    ///
    /// `Ok(…)` — файл точно разобрали и он существует; `Err(reason)` —
    /// проверку теории прогнать нельзя (и причина — не «нужен сервер»).
    fn inspect(&self, path: &Path) -> Result<Result<SyntaxStatus, LookupError>, String> {
        // ВАЖНО: ядро уже произвело запись к моменту `check`; `snapshot` же
        // гоняют ДО записи. Считывать один слой и давить исключение в
        // конкретную причину — здесь месте для честности.
        let file = path.to_string_lossy().into_owned();
        Ok(inspect_file(&self.root, &file))
    }
}

impl LintGate for SyntaxGate {
    /// Снимок проблем ДО записи.
    ///
    /// - файла нет — норма для `write_file` с новым именем: baseline пуст;
    /// - файл есть и разобрался — его текущие синтаксические проблемы;
    /// - язык не поддержан / файл нечитаем / вне корня — `Err` с причиной:
    ///   правка пройдёт, но агент узнает, что код не проверен.
    fn snapshot(&self, path: &Path) -> Result<Option<Vec<LintProblem>>, String> {
        match self.inspect(path)? {
            Err(LookupError::Missing(_)) => Ok(Some(Vec::new())),
            Err(reason) => Err(format!("baseline не снят: {}", reason.reason())),
            Ok(SyntaxStatus::Unsupported) => Err(
                "синтакс-проверка не поддерживается для этого расширения — правка пройдёт без проверки"
                    .to_string(),
            ),
            Ok(SyntaxStatus::Valid) => Ok(Some(Vec::new())),
            Ok(SyntaxStatus::Invalid { line, message }) => {
                Ok(Some(vec![LintProblem { line, message }]))
            }
        }
    }

    /// Проверка ПОСЛЕ записи: вернуть только НОВЫЕ проблемы.
    ///
    /// Проблема «уже была» — если её точная пара (строка, текст) есть в
    /// baseline. Всё прочее — новая синтаксическая ошибка, из-за которой
    /// ядро откатит запись и покажет окружение строк.
    fn check(
        &self,
        path: &Path,
        baseline: &[LintProblem],
    ) -> Result<Option<Vec<LintProblem>>, String> {
        let status = match self.inspect(path)? {
            Err(reason) => return Err(format!("после записи файл не проверен: {}", reason.reason())),
            Ok(status) => status,
        };
        match status {
            SyntaxStatus::Valid => Ok(Some(Vec::new())),
            SyntaxStatus::Unsupported => Err(
                "синтакс-проверка не поддерживается для этого расширения — правка пройдёт без проверки"
                    .to_string(),
            ),
            SyntaxStatus::Invalid { line, message } => {
                let existing = baseline
                    .iter()
                    .any(|problem| problem.line == line && problem.message == message);
                if existing {
                    // Та же ошибка на той же строке — не новая; запись хранится.
                    Ok(Some(Vec::new()))
                } else {
                    Ok(Some(vec![LintProblem { line, message }]))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmpdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("agent_tools_gate_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(root: &Path, rel: &str, content: &str) {
        if let Some(parent) = root.join(rel).parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(root.join(rel), content).unwrap();
    }

    /// Baseline для нового файла — пустой список, не отказ: иначе любая
    /// свежая запись уходила бы с «проверка не выполнена» вместо проверки.
    #[test]
    fn snapshot_of_missing_file_is_empty_baseline() {
        let dir = tmpdir("new_file");
        let gate = SyntaxGate::new(dir.clone());
        assert_eq!(gate.snapshot(&dir.join("fresh.rs")), Ok(Some(Vec::new())));
        let _ = fs::remove_dir_all(&dir);
    }

    /// Baseline валидного файла пуст; сломанного — содержит его ошибку,
    /// чтобы та же ошибка после правки не считалась новой.
    #[test]
    fn snapshot_captures_existing_problems() {
        let dir = tmpdir("existing");
        write(&dir, "ok.rs", "fn main() {}\n");
        let gate = SyntaxGate::new(dir.clone());
        assert_eq!(gate.snapshot(&dir.join("ok.rs")), Ok(Some(Vec::new())));

        write(&dir, "bad.rs", "fn broken( {}\n");
        let problems = gate.snapshot(&dir.join("bad.rs")).unwrap().unwrap();
        assert_eq!(problems.len(), 1, "уже сломанный файл имеет baseline");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Новая ошибка после записи откатывается: ядро получит непустой список.
    #[test]
    fn check_flags_new_breakage() {
        let dir = tmpdir("new_breakage");
        write(&dir, "a.rs", "fn main() {}\n");

        // Имитация записи: агент испортил файл.
        write(&dir, "a.rs", "fn main( {}\n");
        let gate = SyntaxGate::new(dir.clone());
        let problems = gate.check(&dir.join("a.rs"), &[]).unwrap().unwrap();
        assert_eq!(problems.len(), 1, "новая ошибка должна лишить запись покоя");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Ошибка, которая была и осталась, — не «новая»: правка не блокируется.
    #[test]
    fn check_passes_through_persistent_error() {
        let dir = tmpdir("persistent");
        let broken = "fn broken( {}\n";
        write(&dir, "a.rs", broken);
        let gate = SyntaxGate::new(dir.clone());
        let baseline = gate.snapshot(&dir.join("a.rs")).unwrap().unwrap();
        assert!(!baseline.is_empty(), "baseline для сломанного файла непуст");

        // Агент переписал файл, ошибка как была, так и осталась:
        // пара (строка, текст) совпала со снимком — запись не блокируется.
        write(&dir, "a.rs", broken);
        let problems = gate.check(&dir.join("a.rs"), &baseline).unwrap().unwrap();
        assert!(
            problems.is_empty(),
            "существующая ошибка не блокирует правку: {problems:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// Валидный результат после записи — пустой список «новых».
    #[test]
    fn check_reports_clean_write() {
        let dir = tmpdir("clean");
        write(&dir, "b.rs", "fn main() {}\n");
        let gate = SyntaxGate::new(dir.clone());
        let problems = gate.check(&dir.join("b.rs"), &[]).unwrap().unwrap();
        assert!(problems.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    /// Неподдерживаемый язык — честный отказ, а не «порядок» и не «сервер».
    #[test]
    fn unsupported_language_reports_honest_reason() {
        let dir = tmpdir("unsupported");
        write(&dir, "note.md", "# заголовок\n");
        let gate = SyntaxGate::new(dir.clone());
        let error = gate.snapshot(&dir.join("note.md")).unwrap_err();
        assert!(
            error.contains("не поддерживается"),
            "причина обязана говорить о языке, а не о сервере проверки: {error}"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
