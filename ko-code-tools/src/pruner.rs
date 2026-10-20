//! Чистка вывода тестов: PASS-шум — вон, имена упавших и их вывод — вон.
//!
//! ## Зачем
//!
//! Вывод `cargo test` на реальном проекте — это сотни строк вида
//! `test some::module::case ... ok`, и именно они съедают контекст агента,
//! тогда как полезная часть (сводка, имена упавших, стектрейсы) — в конце.
//! Прежняя обрезка резала ХВОСТ, то есть выкидывала ровно то, что нужно.
//!
//! ## Правило строгости
//!
//! Удаляются ТОЛЬКО строки, опознанные как PASS-шум по шаблону конкретного
//! семейства команды. Всё, что не опознано, остаётся: лучше лишняя строка в
//! ответе, чем потерянный стектрейс.
//!
//! ## Серединная обрезка
//!
//! Блок упавшего теста может быть огромным (лог команды, дамп). Тогда он
//! режется ПОСЕРЕДИНЕ: голова и хвост, между ними маркер. Хвост здесь важнее
//! начала — в конце стектрейс, а в начале — заголовок блока.

/// Семейство команды: от него зависит, какие строки считаются PASS-шумом.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// `cargo test` — формат `test имя ... ok`.
    Cargo,
    /// `npm test` на vitest — формат `✓ имя файла`.
    Vitest,
    /// Не тесты (сборка, линтер, typecheck) — вывод не чистится.
    None,
}

/// Максимальный размер одного блока упавшего теста (в строках).
///
/// Блок длиннее — режется посередине, короткий остаётся целиком.
pub const MAX_SEGMENT_LINES: usize = 120;

/// Маркер пропущенных строк при серединной обрезке.
pub const CUT_MARKER: &str = "[обрезано N строк]";

/// Почистить вывод команды по семейству.
pub fn prune(family: Family, output: &str) -> String {
    match family {
        Family::None => output.to_string(),
        Family::Cargo => prune_cargo(output),
        Family::Vitest => prune_vitest(output),
    }
}

/// Строка с её принадлежностью к блоку упавшего теста.
struct Line {
    text: String,
    /// Строка внутри блока вывода упавшего теста (её можно резать посередине).
    in_fail_block: bool,
}

/// Собрать строки обратно, применив серединную обрезку к блокам упавших тестов.
fn assemble(lines: Vec<Line>) -> String {
    let mut out = String::new();
    let mut index = 0usize;
    while index < lines.len() {
        if !lines[index].in_fail_block {
            out.push_str(&lines[index].text);
            out.push('\n');
            index += 1;
            continue;
        }
        // Блок упавшего теста: собираем подряд идущие такие строки.
        let start = index;
        while index < lines.len() && lines[index].in_fail_block {
            index += 1;
        }
        let block: Vec<&Line> = lines[start..index].iter().collect();
        if block.len() > MAX_SEGMENT_LINES {
            let head = MAX_SEGMENT_LINES / 2;
            let tail = MAX_SEGMENT_LINES - head;
            for line in &block[..head] {
                out.push_str(&line.text);
                out.push('\n');
            }
            let cut = block.len() - MAX_SEGMENT_LINES;
            out.push_str(&CUT_MARKER.replace('N', &cut.to_string()));
            out.push('\n');
            for line in &block[block.len() - tail..] {
                out.push_str(&line.text);
                out.push('\n');
            }
        } else {
            for line in block {
                out.push_str(&line.text);
                out.push('\n');
            }
        }
    }
    out
}

/// `cargo test`: PASS-шум — строки `test имя ... ok`.
fn prune_cargo(output: &str) -> String {
    let mut lines: Vec<Line> = Vec::new();
    let mut in_fail_block = false;
    for raw in output.lines() {
        let trimmed = raw.trim_start();
        let is_pass = trimmed.starts_with("test ") && trimmed.ends_with(" ... ok");
        if is_pass {
            // Строка PASS-шума: молча выкидываем. Имя теста из неё не нужно —
            // упавшие тесты перечисляются отдельным блоком.
            continue;
        }
        if trimmed.starts_with("---- ") && trimmed.ends_with(" ----") {
            in_fail_block = true;
        } else if trimmed.starts_with("failures:")
            || trimmed.starts_with("test result:")
            || trimmed.starts_with("running ")
        {
            in_fail_block = false;
        }
        lines.push(Line {
            text: raw.to_string(),
            in_fail_block,
        });
    }
    assemble(lines)
}

/// `npm test` на vitest: PASS-шум — строки, начинающиеся с `✓`/`✔`.
fn prune_vitest(output: &str) -> String {
    let mut lines: Vec<Line> = Vec::new();
    let mut in_fail_block = false;
    for raw in output.lines() {
        let trimmed = raw.trim_start();
        if trimmed.starts_with('✓') || trimmed.starts_with('✔') {
            continue;
        }
        if trimmed.starts_with("FAIL ") {
            in_fail_block = true;
        } else if trimmed.starts_with("Test Files")
            || trimmed.starts_with("Tests ")
            || trimmed.starts_with("❯")
        {
            in_fail_block = false;
        }
        lines.push(Line {
            text: raw.to_string(),
            in_fail_block,
        });
    }
    assemble(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARGO_OUTPUT: &str = "\
   Compiling ko-code-tools v0.1.0
    Finished test profile [unoptimized] target(s) in 1.19s
     Running unittests src/lib.rs

running 3 tests
test alpha::case_one ... ok
test alpha::case_two ... ok
test beta::failing_case ... FAILED

failures:

---- beta::failing_case stdout ----
thread 'beta::failing_case' panicked at src/lib.rs:42:9:
assertion failed: expected 1, got 2
note: run with `RUST_BACKTRACE=1` ...

failures:
    beta::failing_case

test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
";

    const VITEST_OUTPUT: &str = "\
 ✓ test/user.test.ts (2 tests) 12ms
 ❯ test/parser.test.ts (1 test) 8ms
   × parser parses nested braces 8ms
     → expected 3 received 2

 Test Files  1 failed | 1 passed (2)
      Tests  1 failed | 2 passed (3)
";

    #[test]
    fn cargo_pass_noise_is_removed_and_summary_stays() {
        let pruned = prune(Family::Cargo, CARGO_OUTPUT);
        assert!(!pruned.contains("case_one ... ok"), "PASS-шум удалён: {pruned}");
        assert!(!pruned.contains("case_two ... ok"), "PASS-шум удалён: {pruned}");
        assert!(
            pruned.contains("test result: FAILED. 2 passed; 1 failed"),
            "сводка осталась: {pruned}"
        );
        assert!(
            pruned.contains("2 passed; 1 failed"),
            "счётчики пройденных/упавших на месте: {pruned}"
        );
    }

    #[test]
    fn cargo_failing_test_name_and_trace_stay() {
        let pruned = prune(Family::Cargo, CARGO_OUTPUT);
        assert!(
            pruned.contains("test beta::failing_case ... FAILED"),
            "имя упавшего теста осталось: {pruned}"
        );
        assert!(
            pruned.contains("---- beta::failing_case stdout ----"),
            "блок вывода упавшего остался: {pruned}"
        );
        assert!(
            pruned.contains("assertion failed: expected 1, got 2"),
            "текст ошибки остался: {pruned}"
        );
    }

    #[test]
    fn unknown_lines_are_never_dropped() {
        let pruned = prune(Family::Cargo, CARGO_OUTPUT);
        assert!(pruned.contains("Compiling ko-code-tools"), "сборка осталась: {pruned}");
        assert!(pruned.contains("running 3 tests"), "счётчик тестов остался: {pruned}");
    }

    #[test]
    fn vitest_pass_noise_is_removed_and_failure_stays() {
        let pruned = prune(Family::Vitest, VITEST_OUTPUT);
        assert!(!pruned.contains("✓ test/user.test.ts"), "PASS-шум удалён: {pruned}");
        assert!(
            pruned.contains("× parser parses nested braces"),
            "упавший тест остался: {pruned}"
        );
        assert!(
            pruned.contains("expected 3 received 2"),
            "сообщение об ошибке осталось: {pruned}"
        );
        assert!(
            pruned.contains("Tests  1 failed | 2 passed (3)"),
            "сводка осталась: {pruned}"
        );
    }

    #[test]
    fn oversized_fail_block_is_cut_in_the_middle_keeping_the_tail() {
        // Хвост блока (стектрейс) обязан остаться: он в конце.
        let mut output = String::from("---- beta::failing_case stdout ----\n");
        for i in 0..400 {
            output.push_str(&format!("строка шума {i}\n"));
        }
        output.push_str("thread 'beta' panicked at src/lib.rs:42:9:\nassertion failed: expected 1, got 2\n");
        let pruned = prune(Family::Cargo, &output);
        assert!(
            pruned.contains(CUT_MARKER.replace('N', &"").as_str())
                || pruned.contains("[обрезано"),
            "маркер обрезки есть: {pruned}"
        );
        assert!(
            pruned.contains("assertion failed: expected 1, got 2"),
            "хвост блока (стектрейс) сохранён: {pruned}"
        );
        assert!(
            !pruned.contains("строка шума 200"),
            "середина блока вырезана: {pruned}"
        );
        assert!(
            pruned.contains("строка шума 399"),
            "хвост блока сохранён: {pruned}"
        );
        let lines = pruned.lines().count();
        assert!(lines <= MAX_SEGMENT_LINES + 8, "блок ужат до лимита: {lines}");
    }

    #[test]
    fn family_none_keeps_everything() {
        let pruned = prune(Family::None, CARGO_OUTPUT);
        assert_eq!(pruned, CARGO_OUTPUT, "сборка/линт не чистится");
    }
}
