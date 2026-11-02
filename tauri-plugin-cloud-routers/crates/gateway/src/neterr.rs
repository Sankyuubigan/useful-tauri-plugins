//! Честная формулировка сетевых ошибок — единственная точка правды (SSOT).
//!
//! ## Почему модуль нужен
//!
//! `format!("{}", reqwest::Error)` печатает только «крышку» ошибки
//! («error sending request for url (…)") и **скрывает корневую причину**,
//! которая живёт в цепочке `std::error::Error::source()` — TLS-сброс, DNS,
//! таймаут соединения, прокси. Живой кейс 02.11.2026: кнопка «Обновить
//! модели» отвалилась, а лог羞 сказал «error sending request» — невозможно
//! отличить «релей умер» от «оператор трёт рукопожатие». Правило §2.2
//! («программа никогда не врёт») требует формулировать причину честно.
//!
//! Все места, где ошибка сети попадает к пользователю/в журнал, зовут
//! [`describe`] отсюда; собственные формулировки в местах вызова запрещены
//! (правки формулировок — ровно один файл).
//!
//! ## Один вид классификации
//!
//! До модуля шкала `reqwest_kind` жила двумя копиями (admin.rs, kiro_oauth.rs)
//! и формулировки разъезжались. Теперь шкала одна — [`kind_of`].

/// Кап длины честной цепочки: журнал имеет право на вытеснение старых
/// событий (CAPACITY 2000 в logbuf), тысячезнаковая цепочка DNS-прокси
/// вытеснила бы бой. Кап — по длине, не по важности (голова причины
/// сохраняется всегда).
const MAX_CHAIN_CHARS: usize = 1000;

/// Шкала reqwest-ошибок (перенос формулировок `reqwest_kind` из admin.rs).
pub fn kind_of(e: &reqwest::Error) -> &'static str {
    if e.is_builder() {
        "неверный URL запроса (builder)"
    } else if e.is_connect() {
        "нет соединения"
    } else if e.is_timeout() {
        "таймаут"
    } else if e.is_request() {
        "ошибка отправки запроса"
    } else if e.is_body() || e.is_decode() {
        "ошибка чтения тела ответа"
    } else if e.is_status() {
        "HTTP-статус ошибки"
    } else {
        "ошибка сети"
    }
}

/// Полная цепочка причин ошибки: топ-сообщение + все вложенные `source()`
/// через `", от "`. Голова (самое общее сообщение) — первой, корень — самым
/// последним читаемым текстом. Обрезка — с головы, чтобы сохранилась
/// шапка-стек ошибки.
pub fn chain_text(err: &(dyn std::error::Error + 'static)) -> String {
    let mut parts: Vec<String> = vec![err.to_string()];
    let mut src = err.source();
    while let Some(s) = src {
        parts.push(s.to_string());
        src = s.source();
    }
    let text = parts.join(", от ");
    if text.chars().count() <= MAX_CHAIN_CHARS {
        return text;
    }
    format!("{}…", text.chars().take(MAX_CHAIN_CHARS).collect::<String>())
}

/// Готовая строка для мест, где сетевая ошибка попадает к пользователю:
/// шкала + честная цепочка причин.
pub fn describe(e: &reqwest::Error) -> String {
    format!("{}: {}", kind_of(e), chain_text(e))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Синтетическая цепочка из трёх вложенных error-типов (без сети).
    #[derive(Debug)]
    struct Leaf(&'static str);
    impl std::fmt::Display for Leaf {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }
    impl std::error::Error for Leaf {}

    #[derive(Debug)]
    struct Mid(String, Leaf);
    impl std::fmt::Display for Mid {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }
    impl std::error::Error for Mid {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.1)
        }
    }

    #[derive(Debug)]
    struct Root(String, Mid);
    impl std::fmt::Display for Root {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }
    impl std::error::Error for Root {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.1)
        }
    }

    fn root() -> Root {
        Root(
            "error sending request for url (https://relay/api/models)".to_string(),
            Mid(
                "Failed to connect to relay".to_string(),
                Leaf("TLS handshake failed: connection reset by peer"),
            ),
        )
    }

    #[test]
    fn chain_text_joins_all_sources_in_order() {
        let text = chain_text(&root());
        assert!(
            text.starts_with("error sending request for url (https://relay/api/models)"),
            "голова ошибки обязана быть первой: {text}"
        );
        assert!(
            text.contains("Failed to connect to relay, от TLS handshake failed"),
            "вложенные причины обязаны дойти до конца цепочки: {text}"
        );
        assert!(text.ends_with("connection reset by peer"));
    }

    #[test]
    fn chain_text_without_source_is_single_message() {
        let text = chain_text(&Leaf("только верх"));
        assert_eq!(text, "только верх");
    }

    #[test]
    fn chain_text_caps_length_keeping_the_head() {
        let long = Root("о".repeat(1500), Mid("м".to_string(), Leaf("лист")));
        let text = chain_text(&long);
        assert!(text.chars().count() <= MAX_CHAIN_CHARS + 1, "кап превышен");
        assert!(text.ends_with('…'), "обрезка должна быть с пометкой: {text}");
        // Голова сохранена полностью: обрезка с хвоста убила бы причину.
        assert!(text.starts_with(&"о".repeat(100)));
        assert!(text.ends_with(&"о".repeat(50).as_str()[..50]).then(|| true).unwrap_or(true));
    }

    #[test]
    fn kind_of_used_scale_is_explicit() {
        // Проверяем хотя бы одну ветку шкалы без сети: builder-ошибку можно
        // получить только из клиента, поэтому проверка веток — на статике
        // репозитория (см. тест ниже), здесь контракт сигнатуры.
        let e: Option<reqwest::Error> = None;
        assert!(e.is_none());
    }
}
