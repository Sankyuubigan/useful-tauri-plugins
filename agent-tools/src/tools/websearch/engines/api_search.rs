//! Общая база адресных API-движков + префикс-детект (PRP Шаг 4).
//!
//! Схема маршрутизации — СТРОГАЯ (PRP): адресные движки НИКОГДА не входят
//! в DEFAULT_ORDER; движок срабатывает либо по префиксу в тексте запроса
//! (`so:`, `npm:`, `mdn:`, `hf:`, `crates:`), либо по явному списку
//! `engines=[...]`. Тематические эвристики («нейронки» → hf) запрещены
//! (core §1.9) — здесь их нет.
//!
//! Префикс-детект — ЕДИНСТВЕННАЯ точка решения маршрутизации: handle()
//! зовёт только `apply_prefixes`/`detect_prefix` из этого файла.

use serde_json::Value;

use super::super::http;
use super::{http_status_error, SearchResult};

/// Префиксы: (текст, имя движка). Порядок — longest-first, чтобы «crates:»
/// не пересекался с более короткими.
const PREFIXES: &[(&str, &str)] = &[
    ("crates:", "crates"),
    ("mdn:", "mdn"),
    ("npm:", "npm"),
    ("hf:", "huggingface"),
    ("so:", "stackexchange"),
];

/// Детект префикса в одном запросе. Правила:
/// - только в НАЧАЛЕ запроса (после trim);
/// - регистр ASCII не важен (`HF: krea2` → huggingface);
/// - после префикса обязан быть пробел или конец строки — иначе это не
///   префикс (защита от «sogou: ...» → «so:» + «gou: ...»);
/// - голый префикс без содержимого (`"hf:"`) → None: пустой запрос движку
///   не уходит, caller честно вернёт ошибку ввода.
/// Возвращает (имя движка, запрос БЕЗ префикса).
pub fn detect_prefix(query: &str) -> Option<(&'static str, String)> {
    let q = query.trim();
    // Доменные триггеры юзера для Commons (PRP Шаг 3). Это ЯВНЫЕ операторы
    // поиска (`site:...` — доменный фильтр, `File:` — тип файла), а не
    // тематическая эвристика: юзер сам их напечатал. Проверяются ДО
    // обычных префиксов (маркер длиннее любого из них).
    let q_lower = q.to_lowercase();
    if q_lower.contains("site:commons.wikimedia.org") {
        let cleaned = super::wikimedia::strip_site_marker(q);
        if cleaned.is_empty() {
            return None; // один маркер без текста — не профильный запрос
        }
        return Some(("commons", cleaned));
    }
    if q_lower.starts_with("file:") {
        return Some(("commons", q.to_string()));
    }
    for (prefix, engine) in PREFIXES {
        let head_len = prefix.len();
        let Some(head) = q.get(..head_len) else { continue };
        if !head.eq_ignore_ascii_case(prefix) {
            continue;
        }
        let raw_rest = &q[head_len..];
        if !raw_rest.is_empty() && !raw_rest.starts_with(' ') {
            continue; // символ сразу после префикса — это часть другого слова
        }
        let rest = raw_rest.trim();
        if rest.is_empty() {
            return None;
        }
        return Some((*engine, rest.to_string()));
    }
    None
}

/// Разбор списка запросов: если хотя бы один запрос адресный — ВСЕ должны
/// быть адресными одним движком (префикс отрезается на месте). Возвращает
/// Ok(None) — адресных нет; Ok(Some(имя_движка)) — все адресные; Err(текст)
/// — смешаны разные префиксы или адресные с обычными (честная ошибка ввода,
/// caller превращает её в HandlerError::input).
pub fn apply_prefixes(queries: &mut [String]) -> Result<Option<&'static str>, String> {
    let mut chosen: Option<&'static str> = None;
    let mut any_prefixed = false;
    for q in queries.iter_mut() {
        if q.trim().is_empty() {
            continue;
        }
        match detect_prefix(q) {
            Some((engine, cleaned)) => {
                any_prefixed = true;
                if chosen.is_some() && chosen != Some(engine) {
                    let prev = chosen.expect("не None по условию выше");
                    return Err(format!(
                        "смешаны адресные запросы разных движков ({prev} и {engine}) — разведи на отдельные вызовы"
                    ));
                }
                chosen = Some(engine);
                *q = cleaned;
            }
            None if any_prefixed => {
                return Err(format!(
                    "адресный запрос смешан с обычным (\"{q}\") — разведи на отдельные вызовы"
                ));
            }
            None => {}
        }
    }
    Ok(chosen)
}

/// Единая дверь HTTP→JSON для адресных API: GET, 200 → парс JSON.
/// Остальные статусы — честный текст ошибки (в том же стиле, что cause
/// существующих движков ставит в stats).
pub async fn fetch_json(
    url: &str,
    headers: &[(&str, &str)],
    timeout_ms: u64,
) -> Result<Value, String> {
    let opts = http::RequestOpts {
        headers: headers
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        timeout_ms: Some(timeout_ms),
        ..Default::default()
    };
    let res = http::request(url, &opts).await.map_err(|e| e.message)?;
    if res.status != 200 {
        return Err(http_status_error(res.status));
    }
    serde_json::from_str(&res.text).map_err(|_| "невалидный JSON".to_string())
}

/// Движок-адрес API пропустил пустые поля → отбрасывать честно, без мусора.
pub fn push_nonempty(
    out: &mut Vec<SearchResult>,
    title: String,
    url: String,
    snippet: String,
    source: &str,
) {
    if title.is_empty() || url.is_empty() {
        return;
    }
    out.push(SearchResult {
        title,
        url,
        snippet,
        source: source.to_string(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_route_to_exact_engines() {
        assert_eq!(
            detect_prefix("so: espresso use custom scala version"),
            Some(("stackexchange", "espresso use custom scala version".into()))
        );
        assert_eq!(
            detect_prefix("npm: async client"),
            Some(("npm", "async client".into()))
        );
        assert_eq!(
            detect_prefix("mdn: fetch api"),
            Some(("mdn", "fetch api".into()))
        );
        assert_eq!(
            detect_prefix("hf: krea2 turbo"),
            Some(("huggingface", "krea2 turbo".into()))
        );
        assert_eq!(
            detect_prefix("crates: serde derive"),
            Some(("crates", "serde derive".into()))
        );
    }

    #[test]
    fn prefix_is_case_insensitive_but_reserved_for_header() {
        assert_eq!(
            detect_prefix("HF: krea2 turbo").unwrap().0,
            "huggingface",
            "верхний регистр префикса допустим"
        );
        assert_eq!(detect_prefix("лучше hf движок для поиска"), None, "префикс только в начале");
    }

    #[test]
    fn non_prefixed_queries_are_not_routed() {
        assert_eq!(detect_prefix("сравнение Krea2 и тест апскейла на 8 ГБ"), None);
        assert_eq!(detect_prefix("какая нейронка лучше для апскейла"), None);
        assert_eq!(detect_prefix("rust futures"), None);
        assert_eq!(detect_prefix("npm registry лежит"), None, "слово npm внутри — не префикс");
        assert_eq!(detect_prefix(""), None);
    }

    #[test]
    fn sogou_word_is_not_swallowed_by_so_prefix() {
        // «so:» совпадает с началом «sogou: ...», но после "so:" нет пробела —
        // значит это не адресный префикс.
        assert_eq!(detect_prefix("sogou: расширенная выдача"), None);
    }

    #[test]
    fn bare_prefix_or_no_content_is_not_routed() {
        assert_eq!(detect_prefix("hf:"), None, "пустой запрос после префикса");
    }

    #[test]
    fn commons_domain_markers_route_by_explicit_user_operators() {
        // Явные операторы юзера (PRP Шаг 3) — site-фильтр и тип файла.
        assert_eq!(
            detect_prefix("site:commons.wikimedia.org File crunches Canada"),
            Some(("commons", "File crunches Canada".into()))
        );
        assert_eq!(
            detect_prefix("SITE:COMMONS.WIKIMEDIA.ORG file windows"),
            Some(("commons", "file windows".into()))
        );
        // Маркер в середине запроса — тоже юзерский оператор.
        assert_eq!(
            detect_prefix("windows site:commons.wikimedia.org Toronto"),
            Some(("commons", "windows Toronto".into()))
        );
        assert_eq!(
            detect_prefix("File:Building in Toronto.jpg"),
            Some(("commons", "File:Building in Toronto.jpg".into()))
        );
        // Голый маркер без текста — не маршрут.
        assert_eq!(detect_prefix("site:commons.wikimedia.org"), None);
        // Другой wiki-домен — не триггер.
        assert_eq!(detect_prefix("site:en.wikipedia.org Rust"), None);
    }

    #[test]
    fn apply_prefixes_cuts_and_requires_single_engine() {
        let mut qs = vec![" so: espresso use custom scala version ".to_string(),
            "hf: другое".to_string()];
        let err = apply_prefixes(&mut qs).unwrap_err();
        assert!(err.contains("разных движков"), "разные префиксы → ошибка: {err}");
    }

    #[test]
    fn apply_prefixes_rejects_mixing_prefixed_and_plain() {
        let mut qs = vec!["so: espresso".to_string(), "обычный запрос".to_string()];
        let err = apply_prefixes(&mut qs).unwrap_err();
        assert!(err.contains("смешан"));
    }

    #[test]
    fn apply_prefixes_routes_all_queries_to_one_engine() {
        let mut qs = vec!["hf: krea2 turbo".to_string(),
            "hf: flux schnell".to_string()];
        let chosen = apply_prefixes(&mut qs).expect("один движок");
        assert_eq!(chosen, Some("huggingface"));
        assert_eq!(qs, vec!["krea2 turbo".to_string(), "flux schnell".to_string()]);
    }

    #[test]
    fn apply_prefixes_ignores_plain_queries() {
        let mut qs = vec!["rust futures".to_string()];
        assert_eq!(apply_prefixes(&mut qs).expect("нет адресных"), None);
        assert_eq!(qs[0], "rust futures", "обычный запрос не меняется");
    }
}
