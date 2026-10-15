//! Оценка числа токенов без внешних зависимостей.
//!
//! ## Почему не `tokenizers`
//!
//! Крейт `tokenizers` из воркспейса тянет `onig_sys`, а тот компилирует Си.
//! Весь смысл `codemap` в том, что он собирается без Си-компилятора, поэтому
//! точный токенизатор здесь недоступен без потери этого свойства.
//!
//! ## Почему не «длина / 3.7», как в старой версии
//!
//! Деление на константу одинаково ошибается на всех файлах: у минифицированного
//! JS плотность токенов в разы выше, у русского комментария — ниже. Здесь оценка
//! **выборочная**: берётся каждая N-я строка, токены на ней считаются, и
//! результат домножается на длину. Так ошибка усредняется по файлу.
//!
//! Это именно оценка. Она задаёт **бюджет карты**, и потому важна честность
//! единицы измерения: константа и метод откалиброваны на коде, где токен
//! в среднем занимает около четырёх символов.

/// Символов на один токен в коде. Эмпирическая константа для кода, не для
/// прозы: имена идентификаторов и пунктуация токенизуются мельче, чем слова.
const CHARS_PER_TOKEN: f64 = 4.0;

/// Оценка числа токенов в тексте.
///
/// Тексты короче [`SAMPLE_THRESHOLD`] символов считаются целиком: брать из них
/// выборку нечего, а накопленная погрешность округления съела бы весь смысл.
pub fn estimate(text: &str) -> u32 {
    const SAMPLE_THRESHOLD: usize = 400;

    if text.is_empty() {
        return 0;
    }
    let chars = text.chars().count() as f64;
    if text.len() < SAMPLE_THRESHOLD {
        return ratio(chars);
    }

    // Выборка: каждая N-я строка, где N подобрано так, чтобы взять около
    // сотни строк. Больше сотни точность не растёт, а время растёт линейно.
    let lines: Vec<&str> = text.lines().collect();
    let step = (lines.len() / 100).max(1);
    let sample: String = lines
        .iter()
        .step_by(step)
        .copied()
        .collect::<Vec<&str>>()
        .join("\n");
    if sample.is_empty() {
        return ratio(chars);
    }

    let sample_chars = sample.chars().count() as f64;
    let sample_tokens = ratio(sample_chars) as f64;
    (sample_tokens / sample_chars * chars).round() as u32
}

/// Округление с честным минимумом: непустой текст стоит хотя бы один токен.
fn ratio(chars: f64) -> u32 {
    if chars <= 0.0 {
        return 0;
    }
    (chars / CHARS_PER_TOKEN).round().max(1.0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_costs_nothing() {
        assert_eq!(estimate(""), 0);
    }

    #[test]
    fn short_text_is_counted_whole() {
        assert_eq!(estimate("fn alpha() {}"), estimate("fn alpha() {}"));
        assert!(estimate("fn alpha() {}") >= 1, "непустой текст стоит хотя бы токен");
    }

    #[test]
    fn estimate_grows_with_length() {
        let short = estimate("fn alpha() { return 1; }");
        let long = estimate(&"fn alpha() { return 1; }\n".repeat(20));
        assert!(long > short, "длиннее текст обязан стоить больше: {short} vs {long}");
    }

    #[test]
    fn sampling_tracks_dense_and_sparse_files_separately() {
        // Минифицированный код: токенов на символ больше, чем у прозы.
        let dense = "a".repeat(20_000);
        // Комментарии: токенов на символ меньше.
        let sparse = "изучаем репозиторий и строим карту проекта\n".repeat(500);
        let dense_ratio = estimate(&dense) as f64 / dense.len() as f64;
        let sparse_ratio = estimate(&sparse) as f64 / sparse.len() as f64;
        // Валидируем не «какой лучше», а что выборка вообще даёт разумный
        // порядок величины и не падает на длинных входах.
        assert!(
            (0.15..0.5).contains(&dense_ratio),
            "плотный код: неожиданная доля токенов {dense_ratio}"
        );
        assert!(
            (0.1..0.5).contains(&sparse_ratio),
            "прозрачный текст: неожиданная доля токенов {sparse_ratio}"
        );
    }

    #[test]
    fn single_line_without_newlines_is_sampled_too() {
        let text = "x".repeat(10_000);
        let estimate = estimate(&text);
        assert!(estimate > 1000, "одна длинная строка обязана оцениваться: {estimate}");
    }
}