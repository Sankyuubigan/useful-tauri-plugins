//! Динамический порядок движков из статистики — новый файл (PRP Шаг 1).
//!
//! SSOT: единственный источник правды — `search_stats.json` через дверь
//! `stats.rs`. `DEFAULT_ORDER` остаётся только fallback при пустой статистике.
//!
//! Логика (PRP §3 Шаг 1):
//! - движки сортируются по success_rate (ok/calls), tie — более свежий lastOkTs;
//! - движки без данных (calls == 0) идут после проверенных рабочих;
//! - движок с 0 OK за ≥8 вызовов — исключён из дефолтной цепочки (мёртв,
//!   статистически: startpage 0/12 на момент PRP);
//! - движок, чей последний сбой — rate-limit, пропускается 10 минут;
//!   капча/антибот — 30 минут (cooldown);
//! - явный список `engines=[...]` (run_parallel) поток не фильтрует —
//!   cooldown применяется только к дефолтной цепочке.

use serde_json::Value;

use super::stats;

/// Cooldown после последнего rate-limit-сбоя (10 минут).
const RATE_LIMIT_COOLDOWN_MS: u64 = 10 * 60 * 1000;
/// Cooldown после последнего капча/антибот-сбоя (30 минут).
const ANTIBOT_COOLDOWN_MS: u64 = 30 * 60 * 1000;
/// Порог «мёртвого» движка: 0 OK при ≥N вызовах.
const DEAD_MIN_CALLS: u64 = 8;

/// Дефолтная цепочка прямо из боевой статистики. Пустая статистика →
/// `DEFAULT_ORDER` без изменений (fallback, как в PRP 1.3).
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

/// Чистая функция сортировки/фильтрации (используется и тестами — без сети).
pub fn default_chain_from(stats_data: &Value) -> Vec<String> {
    let Some(obj) = stats_data.as_object() else { return Vec::new() };

    // 1. Движки с данными и живые движки без данных отдельно.
    // Порядок обхода — DEFAULT_ORDER: и свежие (без данных) в хвосте
    // сохраняют его относительный порядок.
    let mut known: Vec<&str> = Vec::new();
    let mut fresh: Vec<&str> = Vec::new();
    for name in super::DEFAULT_ORDER {
        if let Some(s) = obj.get(*name).and_then(Value::as_object) {
            let calls = s.get("calls").and_then(Value::as_u64).unwrap_or(0);
            if calls == 0 {
                fresh.push(name);
            } else {
                known.push(name);
            }
        } else {
            fresh.push(name);
        }
    }

    // 2. Мёртвые (0 OK за ≥8) и охлаждённые — skip.
    let now = now_ms();
    known.retain(|name| {
        let s = obj.get(*name).and_then(Value::as_object).expect("движок объект");
        let calls = s.get("calls").and_then(Value::as_u64).unwrap_or(0);
        let ok = s.get("ok").and_then(Value::as_u64).unwrap_or(0);
        if ok == 0 && calls >= DEAD_MIN_CALLS {
            return false; // статистически мёртв — из дефолтной цепочки.
        }
        !on_cooldown(s, now)
    });

    // 3. Сортировка: success_rate desc, tie — свежий lastOkTs выше.
    // У движков, никогда не работавших (fresh), отдельный порядок —
    // они дописываются в конце в порядке DEFAULT_ORDER.
    let rank = |name: &str| -> (u64, i64) {
        let s = obj.get(name).and_then(Value::as_object);
        let calls = s.as_ref().map_or(0, |s| s.get("calls").and_then(Value::as_u64).unwrap_or(0));
        let ok = s.as_ref().map_or(0, |s| s.get("ok").and_then(Value::as_u64).unwrap_or(0));
        // score в промилле, чтобы сравнение было на целых (f64-сортировки с NaN нет).
        let score_ppm = if calls == 0 { u64::MAX } else { ok * 1_000_000 / calls };
        let last_ok = s.as_ref().and_then(|s| s.get("lastOkTs")).and_then(Value::as_u64).unwrap_or(0);
        (score_ppm, last_ok as i64)
    };
    // score desc, lastOkTs desc; стабильный sort сохраняет порядок DEFAULT_ORDER.
    known.sort_by(|a, b| {
        let (sa, la) = rank(a);
        let (sb, lb) = rank(b);
        (sb, lb).cmp(&(sa, la))
    });
    known.iter().chain(fresh.iter()).map(|s| s.to_string()).collect()
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
        // Реестр растёт (Шаг 4 добавил адресные движки): длина цепочки —
        // ALL позиций DEFAULT_ORDER минус 1 исключённый мёртвый startpage.
        assert_eq!(chain.len(), super::super::DEFAULT_ORDER.len() - 1);
    }

    #[test]
    fn not_enough_calls_means_not_dead() {
        // 0/3 (baidu на момент PRP) — порога 8 не хватает: движок остаётся,
        // но в хвосте по score.
        let data = json!({
            "baidu": entry(3, 0, None, 0, "заблокирован (антибот)"),
            "juejin": entry(12, 12, Some(6000), 0, ""),
        });
        let chain = default_chain_from(&data);
        assert_eq!(chain.len(), super::super::DEFAULT_ORDER.len());
        assert_eq!(chain[0], "juejin");
        assert_eq!(chain[1], "baidu", "0/3 хуже 12/12, но жив");
    }

    #[test]
    fn last_fail_captcha_sets_30_min_cooldown() {
        let data = json!({
            "sogou": entry(11, 1, Some(1000), now_ms() - 60_000, "заблокирован (капча)"),
            "marginalia": entry(6, 6, Some(2000), 0, ""),
        });
        let chain = default_chain_from(&data);
        assert!(!chain.contains(&"sogou".to_string()), "sogou на cooldown");
        assert_eq!(chain[0], "marginalia");
    }

    #[test]
    fn rate_limit_sets_10_min_cooldown() {
        let data = json!({
            "brave": entry(23, 12, Some(3000), now_ms() - 60_000, "лимит запросов (rate limit)"),
            "csdn": entry(1, 1, Some(4000), 0, ""),
        });
        let chain = default_chain_from(&data);
        assert!(!chain.contains(&"brave".to_string()), "brave на cooldown 10 мин");
        assert_eq!(chain[0], "csdn");
    }

    #[test]
    fn cooldown_expires_after_window() {
        // Сбой 11 минут назад — rate-limit window (10 мин) истёк.
        let data = json!({
            "brave": entry(23, 12, Some(3000), now_ms() - 11 * 60_000, "лимит запросов (rate limit)"),
            "csdn": entry(1, 1, Some(4000), 0, ""),
        });
        let chain = default_chain_from(&data);
        // Сортировка — по score: csdn 1.0 выше brave 0.52; brave вернулся.
        assert_eq!(&chain[..2], &["csdn".to_string(), "brave".to_string()]);
    }

    #[test]
    fn engines_without_data_go_after_working_ones() {
        let data = json!({
            "bing": entry(0, 0, None, 0, ""),
            "exa": entry(3, 0, None, 0, "HTTP 500"),
            "juejin": entry(12, 12, Some(6000), 0, ""),
        });
        let chain = default_chain_from(&data);
        assert_eq!(chain.len(), super::super::DEFAULT_ORDER.len());
        assert_eq!(chain[0], "juejin");
        assert_eq!(chain[1], "exa");
        assert_eq!(*chain.last().expect("хвост"), "bing", "никогда не вызывавшийся — в самом конце");
    }

    #[test]
    fn sorted_by_success_rate_then_fresh_last_ok() {
        let data = json!({
            "duckduckgo": entry(65, 41, Some(100), 0, ""),   // 0.63
            "brave": entry(23, 12, Some(5000), 0, ""),       // 0.52
            "wiby": entry(2, 1, Some(7000), 0, ""),          // 0.50
        });
        let chain = default_chain_from(&data);
        assert_eq!(&chain[..3], &["duckduckgo".to_string(), "brave".to_string(), "wiby".to_string()]);
        assert_eq!(chain.len(), super::super::DEFAULT_ORDER.len());
    }

    #[test]
    fn tie_breaks_by_fresher_last_ok() {
        let data = json!({
            "csdn": entry(2, 1, Some(1000), 0, ""),
            "wiby": entry(2, 1, Some(2000), 0, ""),
        });
        let chain = default_chain_from(&data);
        assert_eq!(chain.len(), super::super::DEFAULT_ORDER.len());
        assert_eq!(&chain[..2], &["wiby".to_string(), "csdn".to_string()]);
    }

    #[test]
    fn empty_stats_means_default_order_verbatim() {
        // Пустая статистика: все движки «fresh» → порядок ровно DEFAULT_ORDER.
        let empty = json!({});
        let expected: Vec<String> = super::super::DEFAULT_ORDER.iter().map(|s| s.to_string()).collect();
        assert_eq!(names(&default_chain_from(&empty)), expected);
    }

    #[test]
    fn non_object_stats_falls_back_to_default_order() {
        // Формат файла повреждён (не объект) → current_default_chain
        // обязан вернуть DEFAULT_ORDER (fallback, PRP 1.3).
        let chain = current_default_chain_with(&json!([1, 2, 3]));
        let expected: Vec<String> = super::super::DEFAULT_ORDER.iter().map(|s| s.to_string()).collect();
        assert_eq!(chain, expected);
    }

    #[test]
    fn fresher_last_ok_breaks_score_tie() {
        let data = json!({
            "duckduckgo": entry(1, 1, Some(1), 0, ""),
            "wiby": entry(1, 1, Some(2), 0, ""),
        });
        let chain = default_chain_from(&data);
        // fresher lastOkTs — первым.
        assert_eq!(chain[0], "wiby");
    }
}
