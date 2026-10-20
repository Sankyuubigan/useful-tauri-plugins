//! Тело одного символа: от строки объявления до строки следующего объявления.
//!
//! Граница тела — не «по синтаксису», а по известным определениям файла: тело
//! символа заканчивается там, где начинается следующее объявление. Плюсы:
//! правило одинаковое для всех языков и не требует второго разбора. Минус
//! (честно названный в ответе): вложенное объявление обрезает тело раньше
//! времени — поэтому в ответе всегда указано, где тело закончилось.

use super::lookup::{FileView, LookupError};

/// Сколько символов перечислить в подсказке, когда символ не найден.
const MAX_HINT_SYMBOLS: usize = 20;

/// Собрать тело символа.
///
/// Ошибки двух классов: символа нет — это ошибка запроса (агент ошибся именем),
/// файл не читается — отказ исполнения. Различие важно для кода возврата.
pub fn body(view: &FileView, symbol: &str) -> Result<String, LookupError> {
    let name = symbol.trim();
    if name.is_empty() {
        return Err(LookupError::Input("не указано имя символа".to_string()));
    }

    let defs = view.definitions();
    let matching: Vec<&&super::tags::Tag> =
        defs.iter().filter(|tag| tag.name == name).collect();

    if matching.is_empty() {
        return Err(LookupError::Input(format!(
            "Символ «{name}» не найден в {}. Доступные символы: {}",
            view.rel_path,
            hint_symbols(&defs)
        )));
    }

    let start = matching[0].line;
    // Верхняя граница — следующее объявление файла. Если следующих нет, тело
    // идёт до конца файла.
    let end = defs
        .iter()
        .find(|tag| tag.line > start)
        .map(|tag| tag.line.saturating_sub(1))
        .unwrap_or_else(|| view.lines.len() as u32)
        .min(view.lines.len() as u32);

    let mut out = String::new();
    out.push_str(&format!(
        "{}: тело символа «{name}» (строки {start}–{end})\n",
        view.rel_path
    ));
    if matching.len() > 1 {
        let lines: Vec<String> = matching
            .iter()
            .map(|tag| tag.line.to_string())
            .collect();
        out.push_str(&format!(
            "Внимание: символ объявлен {} раз (строки {}); показано первое вхождение.\n",
            matching.len(),
            lines.join(", ")
        ));
    }
    for number in start..=end {
        if let Some(text) = view.line_text(number) {
            out.push_str(&format!("{number:>5} | {text}\n"));
        }
    }
    out.push_str(
        "\nГраница тела — начало следующего объявления файла; вложенное объявление \
         обрезает тело раньше. Для соседнего символа вызови body ещё раз.\n",
    );
    Ok(out)
}

/// Имена символов файла для подсказки «ищи здесь».
fn hint_symbols(defs: &[&super::tags::Tag]) -> String {
    if defs.is_empty() {
        return "файл не содержит определений".to_string();
    }
    let mut names: Vec<&str> = Vec::new();
    for tag in defs {
        if !names.contains(&tag.name.as_str()) {
            names.push(tag.name.as_str());
        }
        if names.len() == MAX_HINT_SYMBOLS {
            break;
        }
    }
    if names.len() == MAX_HINT_SYMBOLS {
        format!("{} и другие", names.join(", "))
    } else {
        names.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::codemap::lookup::FileView;

    const SOURCE: &str = "\
pub fn alpha() -> u32 {
    let value = 1;
    value + 1
}

pub fn beta() -> u32 {
    alpha()
}
";

    fn view(source: &str) -> FileView {
        // Каталог уникален на вызов: тесты идут параллельно, и общий каталог
        // означал бы, что один тест перезаписывает фикстуру другого.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let dir = std::env::temp_dir().join(format!(
            "codemap_body_{}_{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("каталог");
        let path = dir.join("sample.rs");
        std::fs::write(&path, source).expect("фикстура");
        let view = FileView::open(&dir, "sample.rs").expect("разбор");
        let _ = std::fs::remove_dir_all(&dir);
        view
    }

    #[test]
    fn body_contains_the_whole_function_and_stops_at_the_next_one() {
        let text = body(&view(SOURCE), "alpha").expect("тело найдено");
        assert!(text.contains("let value = 1;"), "тело целиком: {text}");
        assert!(text.contains("value + 1"), "хвост тела на месте: {text}");
        assert!(
            !text.contains("pub fn beta()"),
            "следующее объявление не входит в тело: {text}"
        );
    }

    #[test]
    fn body_reports_the_boundary_it_used() {
        let text = body(&view(SOURCE), "alpha").expect("тело найдено");
        // Граница — строка перед следующим объявлением (`beta` начинается на
        // шестой строке), поэтому в тело входит и пустая строка-разделитель.
        assert!(text.contains("строки 1–5"), "границы названы: {text}");
    }

    #[test]
    fn missing_symbol_is_an_input_error_with_a_hint() {
        let error = body(&view(SOURCE), "gamma").unwrap_err();
        assert!(
            matches!(error, LookupError::Input(_)),
            "нет символа — ошибка запроса: {error}"
        );
        assert!(
            error.reason().contains("alpha") && error.reason().contains("beta"),
            "подсказка перечисляет доступные символы: {error}"
        );
    }

    #[test]
    fn last_symbol_body_runs_to_the_end_of_file() {
        let text = body(&view(SOURCE), "beta").expect("тело найдено");
        assert!(text.contains("alpha()"), "тело последнего символа до конца: {text}");
    }
}
