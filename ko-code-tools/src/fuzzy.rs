//! Нечёткое сопоставление фрагментов — третья попытка `edit_file`.
//!
//! ## Зачем
//!
//! Первые две попытки (`exact` и снятие префиксов номеров строк) ловят
//! «агент переписал фрагмент из вывода `read_file`». Третья ловит другую
//! реальность: тот же код, но с другими отступами или другими пробелами
//! (форматирование, табы против пробелов, перенос строки).
//!
//! ## Как устроено сравнение
//!
//! И образец, и файл приводятся к НОРМАЛИЗОВАННОМУ виду: отступы строк
//! отброшены, внутренние пробелы схлопнуты в один, пустые строки выброшены.
//! Совпадение ищется как подстрока нормализованного текста.
//!
//! Нормализация идёт с КАРТОЙ ПОЗИЦИЙ: для каждого байта нормализованного
//! текста известно, какому байту исходника он соответствует. Поэтому найденное
//! совпадение переводится обратно в диапазон исходного текста — и заменяется
//! ИМЕННО ЭТОТ диапазон, а не целые строки вокруг него. Иначе правка «let x = 1;»
//! в строке `let x = 1; let y = 2;` съела бы вторую половину строки.
//!
//! ## Правило строгости
//!
//! Нечёткое совпадение применяется **только если кандидат ровно один**.
//! Два кандидата — это неоднозначность, и разрешать её догадкой значит
//! править не то место: честный отказ с перечислением кандидатов полезнее
//! молчаливой правки не того фрагмента.
//!
//! ## Что здесь НЕ является порогом
//!
//! Минимальная длина образца — константа ядра ([`FUZZY_MIN_CONTENT_CHARS`]),
//! а не аргумент тула: порог, который задаёт модель, она же и будет угадывать.
//! Слишком короткий образец не размывается («одна скобка» совпадает с половиной
//! файла), поэтому fuzzy для него просто не применяется — возвращается обычная
//! ошибка «фрагмент не найден».

/// Минимальная длина нормализованного образца (в символах).
///
/// Образец короче — нечёткий поиск не запускается: совпадения такого
/// образца встречаются в файле десятки раз, и любой из них будет случайным.
pub const FUZZY_MIN_CONTENT_CHARS: usize = 8;

/// Найденный фрагмент: диапазон в исходнике и его текст.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Первая строка кандидата (1-based).
    pub start_line: usize,
    /// Последняя строка кандидата (1-based).
    pub end_line: usize,
    /// Начало диапазона в исходнике (байтовая граница символа).
    pub start_byte: usize,
    /// Конец диапазона в исходнике (байтовая граница символа, исключительно).
    pub end_byte: usize,
    /// Текст кандидата — для показа в ошибке неоднозначности.
    pub text: String,
}

/// Результат нечёткого поиска.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FuzzyMatch {
    /// Кандидат ровно один — правку можно применять.
    Unique(Candidate),
    /// Кандидатов несколько — выбирать нельзя.
    Ambiguous(Vec<Candidate>),
    /// Кандидатов нет.
    None,
}

/// Нормализованный текст вместе с картой «нормализованный байт → исходный байт».
struct Normalized {
    text: String,
    /// Для каждого байта `text` — соответствующий байт исходника.
    origins: Vec<usize>,
}

impl Normalized {
    /// Нормализовать строку с картой позиций.
    fn new(source: &str) -> Normalized {
        let mut text = String::new();
        let mut origins: Vec<usize> = Vec::new();
        let mut offset = 0usize;

        for line in source.split_inclusive('\n') {
            let body = line.strip_suffix('\n').unwrap_or(line);
            let trimmed = body.trim();
            if !trimmed.is_empty() {
                if !text.is_empty() {
                    // Разрыв между строками: его исходная позиция — конец
                    // предыдущей строки (сам перевод строки).
                    push_char(&mut text, &mut origins, '\n', offset.saturating_sub(1));
                }
                let indent_bytes = body.len() - body.trim_start().len();
                let mut last_was_space = false;
                for (index, ch) in trimmed.char_indices() {
                    let origin = offset + indent_bytes + index;
                    if ch.is_whitespace() {
                        if last_was_space {
                            continue;
                        }
                        last_was_space = true;
                        push_char(&mut text, &mut origins, ' ', origin);
                    } else {
                        last_was_space = false;
                        push_char(&mut text, &mut origins, ch, origin);
                    }
                }
            }
            offset += line.len();
        }

        Normalized { text, origins }
    }

    /// Исходное смещение для нормализованного байта.
    fn origin(&self, index: usize) -> usize {
        self.origins.get(index).copied().unwrap_or(0)
    }
}

/// Добавить символ в нормализованный текст и его карту позиций.
fn push_char(text: &mut String, origins: &mut Vec<usize>, ch: char, origin: usize) {
    text.push(ch);
    for k in 0..ch.len_utf8() {
        origins.push(origin + k);
    }
}

/// Найти образец в содержимом файла по нормализованному совпадению.
///
/// Возвращает все вхождения: решение «какое из них правильное» принимает
/// вызывающий, и только когда вхождение ровно одно.
pub fn find(content: &str, pattern: &str) -> FuzzyMatch {
    let wanted = Normalized::new(pattern);
    let wanted_text = wanted.text.trim();
    if wanted_text.is_empty() {
        return FuzzyMatch::None;
    }
    if wanted_text.chars().count() < FUZZY_MIN_CONTENT_CHARS {
        return FuzzyMatch::None;
    }

    let file = Normalized::new(content);
    let mut candidates: Vec<Candidate> = Vec::new();

    for (start, matched) in file.text.match_indices(wanted_text) {
        let len = matched.len();
        if len == 0 {
            continue;
        }
        let start_byte = file.origin(start);
        let end_byte = file.origin(start + len - 1) + 1;
        if end_byte > content.len() || start_byte >= end_byte {
            continue;
        }
        let start_line = line_of(content, start_byte);
        let end_line = line_of(content, end_byte.saturating_sub(1));
        candidates.push(Candidate {
            start_line,
            end_line,
            start_byte,
            end_byte,
            text: content[start_byte..end_byte].to_string(),
        });
    }

    match candidates.len() {
        0 => FuzzyMatch::None,
        1 => FuzzyMatch::Unique(candidates.remove(0)),
        _ => FuzzyMatch::Ambiguous(candidates),
    }
}

/// Номер строки (1-based) для байтового смещения в тексте.
fn line_of(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].matches('\n').count() + 1
}

/// Ширина отступа строки (число ведущих пробельных символов).
pub fn indent_width(line: &str) -> usize {
    line.chars().take_while(|c| c.is_whitespace()).count()
}

/// Пересчитать отступ замены под отступ найденного фрагмента.
///
/// `delta` — разница между отступом найденного кандидата и отступом образца
/// в том файле, откуда образец взят. Положительная — добавить пробелов,
/// отрицательная — снять (но не больше, чем есть).
///
/// **Первая строка не трогается.** Заменяемый диапазон начинается с первого
/// непробельного символа, поэтому отступ первой строки остаётся в файле и
/// добавлять его снова — значит задвоить его. А вот строки 2 и дальше
/// вставляются целиком: их отступ нужен заново, иначе многострочная замена
/// «съедет» на уровень образца.
pub fn reindent(new_text: &str, delta: isize) -> String {
    if delta == 0 {
        return new_text.to_string();
    }
    let mut out = String::with_capacity(new_text.len());
    for (index, line) in new_text.split_inclusive('\n').enumerate() {
        if index > 0 && line.is_empty() {
            continue;
        }
        let has_newline = line.ends_with('\n');
        let body = line.strip_suffix('\n').unwrap_or(line);
        if body.trim().is_empty() {
            out.push_str(line);
            continue;
        }
        let current = indent_width(body);
        let new_indent = if index == 0 {
            // Первая строка вставляется после отступа, который уже есть в файле.
            current
        } else if delta > 0 {
            current + delta as usize
        } else {
            current.saturating_sub(delta.unsigned_abs())
        };
        let rest = &body[current..];
        out.push_str(&" ".repeat(new_indent));
        out.push_str(rest);
        if has_newline {
            out.push('\n');
        }
    }
    out
}

/// Разница отступов кандидата и образца: столько пробелов добавить (или снять)
/// в замене, чтобы она легла в код с тем же отступом.
pub fn indent_delta(candidate_first_line: &str, pattern_first_line: &str) -> isize {
    indent_width(candidate_first_line) as isize - indent_width(pattern_first_line) as isize
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "\
fn main() {
    let x = 1;
    if x > 0 {
        println!(\"да\");
    }
}
";

    #[test]
    fn unique_match_is_found_despite_different_indent() {
        // Образец без отступа, в файле — с четырьмя пробелами.
        let pattern = "let x = 1;\nif x > 0 {\nprintln!(\"да\");\n}";
        match find(FILE, pattern) {
            FuzzyMatch::Unique(candidate) => {
                assert_eq!(candidate.start_line, 2, "первая строка кандидата");
                assert_eq!(candidate.end_line, 5, "последняя строка кандидата");
                assert!(candidate.text.contains("let x = 1;"), "текст кандидата: {}", candidate.text);
            }
            other => panic!("ожидался уникальный кандидат, получено {other:?}"),
        }
    }

    #[test]
    fn two_candidates_are_ambiguous_not_a_guess() {
        // Разные пробелы внутри строки: точный поиск не находит, нечёткий — два кандидата.
        let file = "fn a() { let v   =   1; }\nfn b() { let v   =   1; }\n";
        match find(file, "let v = 1;") {
            FuzzyMatch::Ambiguous(candidates) => {
                assert_eq!(candidates.len(), 2, "оба кандидата найдены");
                assert_eq!(candidates[0].start_line, 1);
                assert_eq!(candidates[1].start_line, 2);
            }
            other => panic!("ожидалась неоднозначность, получено {other:?}"),
        }
    }

    #[test]
    fn no_candidate_is_none() {
        assert_eq!(find(FILE, "совершенно другой код"), FuzzyMatch::None);
    }

    #[test]
    fn too_short_pattern_is_not_matched_at_all() {
        // Короткий образец совпадает с десятками мест — искать его нельзя.
        assert_eq!(find(FILE, "}"), FuzzyMatch::None);
        assert_eq!(find(FILE, "x"), FuzzyMatch::None);
    }

    #[test]
    fn internal_spaces_do_not_prevent_a_match() {
        let file = "let  value   =   42;\n";
        match find(file, "let value = 42;") {
            FuzzyMatch::Unique(candidate) => {
                assert_eq!(candidate.start_line, 1);
                assert_eq!(candidate.text, "let  value   =   42;", "диапазон покрывает исходный текст");
            }
            other => panic!("ожидался кандидат, получено {other:?}"),
        }
    }

    #[test]
    fn blank_lines_inside_the_pattern_are_ignored() {
        let file = "fn a() {\n\n    let v = 1;\n\n}\n";
        match find(file, "let v = 1;") {
            FuzzyMatch::Unique(candidate) => {
                assert_eq!(candidate.start_line, 3, "кандидат — одна строка");
                assert_eq!(candidate.end_line, 3);
            }
            other => panic!("ожидался кандидат, получено {other:?}"),
        }
    }

    #[test]
    fn match_inside_a_line_keeps_the_rest_of_the_line_out() {
        // Кандидат — часть строки: диапазон не должен съесть её остаток.
        let file = "fn a() { let v = 1; let w = 2; }\n";
        match find(file, "let v = 1;") {
            FuzzyMatch::Unique(candidate) => {
                assert_eq!(candidate.text, "let v = 1;", "только найденный фрагмент");
            }
            other => panic!("ожидался кандидат, получено {other:?}"),
        }
    }

    #[test]
    fn reindent_adds_and_removes_spaces() {
        // Первая строка не трогается (её отступ остаётся в файле), дальние —
        // сдвигаются на дельту.
        let added = reindent("let x = 1;\nlet y = 2;", 4);
        assert_eq!(added, "let x = 1;\n    let y = 2;");
        let removed = reindent("        let x = 1;", -4);
        assert_eq!(removed, "        let x = 1;");
        // Снять больше, чем есть, — нельзя: отступ не уходит в минус.
        let floor = reindent("  let x = 1;", -8);
        assert_eq!(floor, "  let x = 1;");
        // Нулевая дельта — текст без изменений.
        assert_eq!(reindent("let x = 1;", 0), "let x = 1;");
    }

    #[test]
    fn indent_delta_is_measured_between_candidate_and_pattern() {
        assert_eq!(indent_delta("    let x = 1;", "let x = 1;"), 4);
        assert_eq!(indent_delta("let x = 1;", "    let x = 1;"), -4);
        assert_eq!(indent_delta("let x = 1;", "let x = 1;"), 0);
    }
}
