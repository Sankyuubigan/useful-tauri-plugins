//! Генерация секрета релея (`RELAY_TOKEN` / `x-relay-token`).
//!
//! ## Почему это в `core`, а не в шлюзе
//!
//! Секрет — часть конфигурации (`GatewayConfig::vercel_relay_token`), и
//! единственный источник правды о нём — конфиг. Значит и «как выглядит
//! корректный секрет» должно жить рядом с ним: иначе правило разъедутся между
//! крейтом-хранилищем и крейтом-исполнителем, и проверка «а достаточной ли
//! длины токен» окажется в другом месте, чем сам токен.
//!
//! ## Почему криптостойкость, а не «просто случайные числа»
////!
//! Релей развёрнут публично, и этот секрет — единственное, что отличает
//! «свой» запрос от чужого. Предсказуемый токен здесь — не «слабая защита», а
//! её отсутствие: перебор 32 байт не имеет смысла, а последовательность из
//! `SystemTime` перебирается за секунды. Источник энтропии — системный CSPRNG
//! (`getrandom`, под капотом `BCryptGenRandom` на Windows).

use std::fmt::Write as _;

/// Сколько байт энтропии в секрете.
///
/// 32 байта = 64 hex-символа. Меньше — перебирается, больше — Vercel режет
/// значение и тратит место в панели без выигрыша в безопасности.
pub const RELAY_TOKEN_BYTES: usize = 32;

/// Ошибка получения энтропии. Отдельная, чтобы вызывающий мог сказать
/// пользователю, ЧТО именно сломалось, а не «релей не настроился».
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenError {
    /// Системный генератор случайных чисел недоступен.
    Entropy(String),
}

impl std::fmt::Display for TokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TokenError::Entropy(e) => write!(
                f,
                "системный генератор случайных чисел недоступен ({}). \
                 Секрет релея не создан — релей не развёрнут, потому что был бы публичным прокси.",
                e
            ),
        }
    }
}

impl std::error::Error for TokenError {}

/// Сгенерировать секрет релея: 64 hex-символа (32 байта энтропии).
///
/// `hex`, а не base64/base64url: значение попадает в `RELAY_TOKEN` на Vercel и в
/// заголовок `x-relay-token`. Base64 содержит `+`, `/`, `=` — их пришлось бы
/// экранировать на двух сторонах, а любая ошибка экранирования выглядит как
/// «секрет не сходится», и пользователь ищет причину не там.
pub fn generate_relay_token() -> Result<String, TokenError> {
    let mut bytes = [0u8; RELAY_TOKEN_BYTES];
    getrandom::fill(&mut bytes).map_err(|e| TokenError::Entropy(e.to_string()))?;
    let mut out = String::with_capacity(RELAY_TOKEN_BYTES * 2);
    for b in bytes {
        // Форматирование не может упасть: write! в String с зарезервированной
        // ёмкостью. Ошибка игнорируется сознательно и безопасно.
        let _ = write!(out, "{:02x}", b);
    }
    Ok(out)
}

/// Достаточно ли секрета, чтобы считать его рабочим.
///
/// Пустая строка и строка из пробелов секретом не являются: воркер сравнивает
/// заголовок с `process.env.RELAY_TOKEN` буквально, поэтому пробельный токен
/// прошёл бы «задан ли» и молча не пустил бы ни один запрос.
pub fn is_usable_token(token: Option<&str>) -> bool {
    matches!(token, Some(t) if t.trim().len() == t.len() && !t.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_64_hex_chars() {
        let token = generate_relay_token().expect("энтропия доступна");
        assert_eq!(token.len(), RELAY_TOKEN_BYTES * 2, "ожидалось 64 hex-символа: {}", token);
        assert!(
            token.chars().all(|c| c.is_ascii_hexdigit()),
            "секрет обязан быть hex: {}",
            token
        );
        assert!(
            token.chars().all(|c| !c.is_ascii_uppercase()),
            "нужны строчные: {}",
            token
        );
    }

    #[test]
    fn tokens_differ_between_calls() {
        // 32 байта энтропии: совпадение двух подряд — не «маловероятно», а
        // признак сломанного генератора.
        let a = generate_relay_token().expect("энтропия");
        let b = generate_relay_token().expect("энтропия");
        assert_ne!(a, b, "два вызова подряд дали одинаковый секрет");
    }

    #[test]
    fn error_message_explains_that_relay_stays_undeployed() {
        // Пользователь видит это сообщение в UI. «Генератор недоступен» без
        // последствий заставило бы гадать, работает ли релей.
        let msg = TokenError::Entropy("access denied".into()).to_string();
        assert!(msg.contains("публичным прокси"), "{msg}");
        assert!(msg.contains("access denied"), "в сообщении нет причины: {msg}");
    }

    #[test]
    fn is_usable_token_rejects_absent_blank_and_padded_values() {
        assert!(is_usable_token(Some(&generate_relay_token().expect("энтропия"))));
        assert!(!is_usable_token(None));
        assert!(!is_usable_token(Some("")), "пустая строка — не секрет");
        assert!(!is_usable_token(Some("   ")), "пробелы — не секрет");
        assert!(
            !is_usable_token(Some(" abc ")),
            "секрет с краями пробелов не совпадёт с заголовком посимвольно"
        );
    }

    #[test]
    fn is_usable_token_accepts_exactly_the_generated_shape() {
        // Связка «генератор → проверка»: секрет, который мы сами произвели,
        // обязан проходить ту же проверку, которую проходит поле из конфига.
        assert!(is_usable_token(Some(&generate_relay_token().expect("энтропия"))));
    }
}