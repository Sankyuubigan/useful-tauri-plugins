//! Состав дефолтной цепочки из статистики — `search_stats.json` через дверь
//! `stats.rs`. `DEFAULT_ORDER` — базовый порядок; статистика только ФИЛЬТРУЕТ.
//!
//! Регрессия из боевых данных (окт 2026): цепочку сортировали по success_rate
//! (ok/calls), но метрика меряет «движок не упал и что-то вернул», а не
//! «вернул релевантное». Никогда не ошибающиеся китайские платформы (juejin
//! 32/32, marginalia 15/15) вытеснили DDG (66/91) из головы, ранний выход
//! закреплял их монополию, и латинские запросы получали китайский мусор с
//! «релевантностью 100%». Поэтому пересортировки больше нет: порядок —
//! DEFAULT_ORDER, динамика оставляет только два механизма:
//! - движок с 0 OK за ≥8 вызовов — исключён (мёртв);
//! - последний сбой rate-limit → пропуск 10 минут; капча/антибот → 30 минут.
//!
//! Поверх фильтра работает языковой гейт: без CJK в запросах китайские
//! платформенные движки уходят в хвост (после западных), с CJK — порядок
//! DEFAULT_ORDER.
//!
//! Явный список `engines=[...]` (run_parallel) не фильтруется вообще —
//! и фильтры, и гейт применяются только к дефолтной цепочке.

use serde_json::Value;

use super::stats;

/// Cooldown после последнего rate-limit-сбоя (10 минут).
const RATE_LIMIT_COOLDOWN_MS: u64 = 10 * 60 * 1000;
/// Cooldown после последнего капча/антибот-сбоя (30 минут).
const ANTIBOT_COOLDOWN_MS: u64 = 30 * 60 * 1000;
/// Порог «мёртвого» движка: 0 OK при ≥N вызовах.
const DEAD_MIN_CALLS: u64 = 8;

/// Китайские платформенные движки: осмысленны только для CJK-запросов.
/// По латинице/кириллице выдача — одиночные совпадения токенов (боевое:
/// tarot-запрос → CSS-статья со словом `upright`, RFC 9001 → статьи про
/// порт 9001 MinIO), поэтому в дефолтной цепочке они живут в хвосте.
pub const CJK_PLATFORM_ENGINES: &[&str] = &["juejin", "csdn", "baidu", "sogou"];

/// Дефолтная цепочка прямо из боевой статистики. Пустая статистика →
/// `DEFAULT_ORDER` без изменений.
pub fn current_default_chain() -> Vec<String> {
    current_default_chain_with(&stats::raw_entries())
}

/// То же, но с вручную заданной статистикой (дверь для тестов).
pub fn current_default_chain_with(stats_data: &Value) -> Vec<String> {
    let chain = default_chain_from(stats_data);
    if chain.is_empty() {
        super::DEFAULT_ORDER.iter().map(|s| s.to_string()).collect()
    } else {
        chain
    }
}

/// Чистая функция фильтрации (используется и тестами — без сети):
/// DEFAULT_ORDER как есть, минус мёртвые и охлаждённые движки.
pub fn default_chain_from(stats_data: &Value) -> Vec<String> {
    let Some(obj) = stats_data.as_object() else { return Vec::new() };
    let now = now_ms();
    let mut chain: Vec<String> = Vec::new();
    for name in super::DEFAULT_ORDER {
        let Some(s) = obj.get(*name).and_then(Value::as_object) else {
            chain.push(name.to_string()); // нет данных — движок жив, на своём месте.
            continue;
        };
        let calls = s.get("calls").and_then(Value::as_u64).unwrap_or(0);
        let ok = s.get("ok").and_then(Value::as_u64).unwrap_or(0);
        if calls >= DEAD_MIN_CALLS && ok == 0 {
            continue; // статистически мёртв — из дефолтной цепочки.
        }
        if on_cooldown(s, now) {
            continue;
        }
        chain.push(name.to_string());
    }
    chain
}

/// Есть ли в запросах CJK-символы (иероглифы, кана, хангыль).
pub fn has_cjk(queries: &[String]) -> bool {
    queries.iter().any(|q| {
        q.chars().any(|c| {
            matches!(c as u32,
                0x3400..=0x4DBF   // CJK Extension A
                | 0x4E00..=0x9FFF // CJK Unified Ideographs
                | 0x3040..=0x30FF // Hiragana + Katakana
                | 0xF900..=0xFAFF // CJK Compatibility Ideographs
                | 0xAC00..=0xD7AF // Hangul
            )
        })
    })
}

/// Языковой гейт: без CJK в запросах китайские платформы уходят в хвост
/// цепочки (после западных, в их относительном порядке). С CJK — без
/// изменений: платформы на своих местах DEFAULT_ORDER.
pub fn demote_cjk_engines(chain: Vec<String>, queries: &[String]) -> Vec<String> {
    if has_cjk(queries) {
        return chain;
    }
    let mut tail: Vec<String> = Vec::new();
    let mut head: Vec<String> = Vec::new();
    for name in chain {
        if CJK_PLATFORM_ENGINES.contains(&name.as_str()) {
            tail.push(name);
        } else {
            head.push(name);
        }
    }
    head.extend(tail);
    head
}

/// Движок на cooldown: категория последнего сбоя + свежесть lastFailTs.
fn on_cooldown(s: &serde_json::Map<String, Value>, now: u64) -> bool {
    let last_fail_reason = s.get("lastFailReason").and_then(Value::as_str).unwrap_or_default();
    let category = stats::categorize_reason(last_fail_reason);
    let window = match category {
        "rate-limit" => Some(RATE_LIMIT_COOLDOWN_MS),
        "captcha" | "antibot" => Some(ANTIBOT_COOLDOWN_MS),
        _ => None,
    };
    let Some(window) = window else { return false };
    let last_fail_ts = s.get("lastFailTs").and_then(Value::as_u64).unwrap_or(0);
    last_fail_ts > 0 && now.saturating_sub(last_fail_ts) < window
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Фикстура: stats-объект вида search_stats.json (формы stats.rs).
    fn entry(calls: u64, ok: u64, last_ok: Option<u64>, last_fail_ts: u64, last_fail_reason: &str) -> Value {
        json!({
            "calls": calls, "ok": ok,
            "lastOkTs": last_ok, "lastFailTs": if last_fail_ts == 0 { Value::Null } else { json!(last_fail_ts) },
            "lastFailReason": last_fail_reason,
            "reasons": {}
        })
    }

    fn names(v: &[String]) -> Vec<String> {
        v.to_vec()
    }

    #[test]
    fn unknown_engine_names_are_ignored_even_if_in_stats() {
        // Чужие движки в файле статистики не врываются в цепочку.
        let mut data = json!({});
        data["спутник"] = entry(10, 10, Some(9999), 0, "");
        let chain = default_chain_from(&data);
        assert!(!chain.contains(&"спутник".to_string()), "чужое имя не входит");
        // Все известные движки остаются, порядок — DEFAULT_ORDER.
        let expected: Vec<String> = super::super::DEFAULT_ORDER.iter().map(|s| s.to_string()).collect();
        assert_eq!(names(&chain), expected);
    }

    #[test]
    fn dead_engine_with_zero_ok_over_8_calls_is_excluded() {
        let data = json!({
            "startpage": entry(12, 0, None, 999, "Anubis PoW-челлендж"),
            "duckduckgo": entry(65, 41, Some(5000), 0, ""),
        });
        let chain = default_chain_from(&data);
        assert!(!chain.contains(&"startpage".to_string()), "0 OK при 12 вызовах — мёртв");
        assert_eq!(chain[0], "duckduckgo");
        // Реестр растёт (адресные движки добавлены): длина цепочки —
        // ALL позиций DEFAULT_ORDER минус 1 исключённый мёртвый startpage.
        assert_eq!(chain.len(), super::super::DEFAULT_ORDER.len() - 1);
    }

    #[test]
    fn not_enough_calls_means_not_dead() {
        // 0/3 (baidu на момент PRP) — порога 8 не хватает: движок остаётся
        // на своём месте DEFAULT_ORDER.
        let data = json!({
            "baidu": entry(3, 0, None, 0, "заблокирован (антибот)"),
            "juejin": entry(12, 12, Some(6000), 0, ""),
        });
        let chain = default_chain_from(&data);
        assert_eq!(chain.len(), super::super::DEFAULT_ORDER.len());
        assert!(chain.contains(&"baidu".to_string()));
        assert!(chain.contains(&"juejin".to_string()));
    }

    #[test]
    fn last_fail_captcha_sets_30_min_cooldown() {
        let data = json!({
            "sogou": entry(11, 1, Some(1000), now_ms() - 60_000, "заблокирован (капча)"),
            "marginalia": entry(6, 6, Some(2000), 0, ""),
        });
        let chain = default_chain_from(&data);
        assert!(!chain.contains(&"sogou".to_string()), "sogou на cooldown");
        assert_eq!(chain[0], "duckduckgo", "порядок — DEFAULT_ORDER без пересортировки");
    }

    #[test]
    fn rate_limit_sets_10_min_cooldown() {
        let data = json!({
            "brave": entry(23, 12, Some(3000), now_ms() - 60_000, "лимит запросов (rate limit)"),
            "csdn": entry(1, 1, Some(4000), 0, ""),
        });
        let chain = default_chain_from(&data);
        assert!(!chain.contains(&"brave".to_string()), "brave на cooldown 10 мин");
    }

    #[test]
    fn cooldown_expires_after_window() {
        // Сбой 11 минут назад — rate-limit window (10 мин) истёк.
        let data = json!({
            "brave": entry(23, 12, Some(3000), now_ms() - 11 * 60_000, "лимит запросов (rate limit)"),
        });
        let chain = default_chain_from(&data);
        assert!(chain.contains(&"brave".to_string()), "cooldown истёк — brave вернулся");
        // Порядок всё равно DEFAULT_ORDER: brave на своём 4-м месте.
        let expected_pos = super::super::DEFAULT_ORDER.iter().position(|n| *n == "brave").unwrap();
        assert_eq!(chain[expected_pos], "brave");
    }

    #[test]
    fn high_success_rate_does_not_reorder_chain() {
        // КЛЮЧЕВОЙ регресс-тест: никогда не падающий китайский движок не
        // должен вставать перед DDG — success rate меряет доступность,
        // а не релевантность (боевая регрессия окт 2026).
        let data = json!({
            "duckduckgo": entry(91, 66, Some(100), 0, ""),  // 0.725
            "juejin": entry(32, 32, Some(5000), 0, ""),     // 1.0
            "csdn": entry(8, 7, Some(4000), 0, ""),         // 0.875
        });
        let chain = default_chain_from(&data);
        let expected: Vec<String> = super::super::DEFAULT_ORDER.iter().map(|s| s.to_string()).collect();
        assert_eq!(names(&chain), expected, "порядок DEFAULT_ORDER независимо от success rate");
    }

    #[test]
    fn empty_stats_means_default_order_verbatim() {
        // Пустая статистика: порядок ровно DEFAULT_ORDER.
        let empty = json!({});
        let expected: Vec<String> = super::super::DEFAULT_ORDER.iter().map(|s| s.to_string()).collect();
        assert_eq!(names(&default_chain_from(&empty)), expected);
    }

    #[test]
    fn non_object_stats_falls_back_to_default_order() {
        // Формат файла повреждён (не объект) → current_default_chain
        // обязан вернуть DEFAULT_ORDER (fallback).
        let chain = current_default_chain_with(&json!([1, 2, 3]));
        let expected: Vec<String> = super::super::DEFAULT_ORDER.iter().map(|s| s.to_string()).collect();
        assert_eq!(chain, expected);
    }

    #[test]
    fn cjk_detection() {
        assert!(has_cjk(&["塔罗牌 json".to_string()]));
        assert!(has_cjk(&["plain".to_string(), "カード".to_string()]));
        assert!(has_cjk(&["한국어".to_string()]));
        assert!(!has_cjk(&["tarot card dataset".to_string()]));
        assert!(!has_cjk(&["RFC 9001 QUIC secret".to_string()]));
        assert!(!has_cjk(&["таролог колода".to_string()]));
    }

    #[test]
    fn cjk_engines_demoted_to_tail_for_non_cjk_queries() {
        let chain: Vec<String> = super::super::DEFAULT_ORDER.iter().map(|s| s.to_string()).collect();
        let demoted = demote_cjk_engines(chain.clone(), &["tarot card dataset".to_string()]);
        // Хвост: sogou, juejin, csdn, baidu — в их относительном порядке
        // DEFAULT_ORDER (sogou стоит раньше juejin).
        assert_eq!(&demoted[demoted.len() - 4..], &["sogou", "juejin", "csdn", "baidu"]);
        // Голова — западные в порядке DEFAULT_ORDER, DDG первым.
        assert_eq!(demoted[0], "duckduckgo");
        assert_eq!(demoted[1], "wiby");
        // Китайских в голове нет.
        assert!(!demoted[..demoted.len() - 4].iter().any(|n| CJK_PLATFORM_ENGINES.contains(&n.as_str())));
    }

    #[test]
    fn cjk_queries_keep_default_order_untouched() {
        let chain: Vec<String> = super::super::DEFAULT_ORDER.iter().map(|s| s.to_string()).collect();
        let demoted = demote_cjk_engines(chain.clone(), &["塔罗牌 json".to_string()]);
        assert_eq!(names(&demoted), names(&chain), "CJK-запрос — порядок без изменений");
    }
}
