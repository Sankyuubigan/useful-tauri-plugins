//! Общие текстовые утилиты движков (двери движков, PRP Шаг 5).
//!
//! Раньше `clip_chars` жил в `brave.rs`, и 7 других движков звали его
//! через чужой модуль (`brave::clip_chars`) — нарушение принципа дверей
//! (core §2.3). Теперь тело здесь, а re-export в `engines/mod.rs` —
//! движки зовут `super::clip_chars`.

/// Стриппер по символам: JS-подобный `slice(0, n)` (порт
/// `query.slice(0, 60)` / `title.slice(0, 200)`).
pub fn clip_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_chars_takes_n_chars_not_bytes() {
        assert_eq!(clip_chars("привет мир", 6), "привет");
        assert_eq!(clip_chars("ab", 10), "ab");
        assert_eq!(clip_chars("", 3), "");
        // Кириллица не рвётся посреди байтов.
        assert_eq!(clip_chars("кириллица", 4), "кири");
    }
}
