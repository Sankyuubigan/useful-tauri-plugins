//! Статистика сбоев движков — порт `search_stats.ts` (King Orch).
//!
//! Файл `search_stats.json` в `store::data_dir()`; формат записи движка
//! 1-в-1 с TS (camelCase-ключи: lastOkTs/lastFailTs/lastFailReason).
//!
//! Осознанные отличия (см. PRP §4.1):
//! - нет in-memory кеша TS: каждое чтение/запись идёт прямо в файл
//!   (`ko_json_store::write_atomic`); файл маленький, зато несколько процессов
//!   тула видят одни и те же данные и нет гонки «память перетирает файл»;
//! - запись под глобальным мьютексом: параллельный режим вызывает движки
//!   одновременно, а read-modify-write двух процессов без лока потерял бы
//!   инкремент;
//! - компактный JSON вместо `JSON.stringify(data, null, 1)` — формат файла
//!   внутренний, поведение не меняет;
//! - сортировка причин при равном счётчике — алфавит (serde_json Map —
//!   BTreeMap), в TS — порядок вставки. Влияет только на порядок `топ-2`.

use std::path::Path;

use chrono::TimeZone;
use regex::Regex;
use serde_json::{Value, json};

use super::store::data_dir;

const STATS_FILE: &str = "search_stats.json";

static WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn stats_path(dir: &Path) -> std::path::PathBuf {
    let mut p = dir.to_path_buf();
    p.push(STATS_FILE);
    p
}

/// Категоризация причины отказа — порт `categorizeReason` (порядок веток 1-в-1).
pub fn categorize_reason(reason: &str) -> &'static str {
    let r = reason;
    let rate = Regex::new(r"(?i)rate limit|ratelimit|429").expect("константная регулярка");
    if rate.is_match(r) {
        return "rate-limit";
    }
    let captcha = Regex::new(r"(?i)капча|captcha|challenge|anomaly|unusual|аномал|verify you are human")
        .expect("константная регулярка");
    if captcha.is_match(r) {
        return "captcha";
    }
    let antibot = Regex::new(r"(?i)Cloudflare|WAF|Anubis|PoW|угрожает|нужен браузер")
        .expect("константная регулярка");
    if antibot.is_match(r) {
        return "antibot";
    }
    let geo = Regex::new(r"(?i)451|гео-блок").expect("константная регулярка");
    if geo.is_match(r) {
        return "geo";
    }
    let forbidden = Regex::new(r"(?i)403|401|запрещено|доступ запрещён").expect("константная регулярка");
    if forbidden.is_match(r) {
        return "forbidden";
    }
    let deadline = Regex::new(r"(?i)дедлайн|завис").expect("константная регулярка");
    if deadline.is_match(r) {
        return "deadline";
    }
    // Таймаут-класс: перечисление причин — 1-в-1 с TS.
    let network = Regex::new(
        r"(?i)Таймаут|DNS|TLS|соединение|отклонено|недоступна|разорвано|Сеть|ETIMEDOUT|ECONN",
    )
    .expect("константная регулярка");
    if network.is_match(r) {
        return "network";
    }
    let http = Regex::new(r"(?i)HTTP \d|невалидный JSON|err_no|500|503|404").expect("константная регулярка");
    if http.is_match(r) {
        return "http";
    }
    let empty = Regex::new(r"(?i)пусто|не найдено результатов").expect("константная регулярка");
    if empty.is_match(r) {
        return "empty";
    }
    "other"
}

/// Тестовый крюк: фиксированное «сейчас» для детерминированных lastOkTs/lastFailTs.
static TEST_NOW: std::sync::Mutex<Option<u64>> = std::sync::Mutex::new(None);

fn now_ms() -> u64 {
    if let Ok(guard) = TEST_NOW.lock() {
        if let Some(v) = *guard {
            return v;
        }
    }
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn load_from(dir: &Path) -> Value {
    let path = stats_path(dir);
    if let Ok(Some(raw)) = ko_json_store::read_text(&path) {
        if let Ok(v) = serde_json::from_str::<Value>(&raw) {
            if v.is_object() {
                return v;
            }
        }
    }
    json!({})
}

fn save_to(dir: &Path, data: &Value) {
    let _ = std::fs::create_dir_all(dir);
    let content = serde_json::to_string(data).unwrap_or_else(|_| "{}".to_string());
    let _ = ko_json_store::write_atomic(&stats_path(dir), &content);
}

/// Зафиксировать исход одного вызова движка — порт `recordEngineCall`.
/// Вызывается при КАЖДОМ обращении (ok или fail). `ms` в файл не пишется —
/// соответствие TS (там параметр тоже не сохраняется).
pub fn record_engine_call(engine: &str, ok: bool, _ms: u64, reason: Option<&str>) {
    if engine.is_empty() {
        return;
    }
    let _guard = WRITE_LOCK.lock();
    record_into(&data_dir(), engine, ok, reason);
}

fn record_into(dir: &Path, engine: &str, ok: bool, reason: Option<&str>) {
    let mut data = load_from(dir);
    let obj = data.as_object_mut().expect("load_from всегда объект");
    let entry = obj
        .entry(engine.to_string())
        .or_insert_with(|| {
            json!({
                "calls": 0, "ok": 0, "fails": 0,
                "lastOkTs": Value::Null, "lastFailTs": Value::Null,
                "lastFailReason": "", "reasons": {}
            })
        })
        .as_object_mut()
        .expect("запись движка всегда объект");
    let bump = |v: &mut Value, delta: u64| {
        *v = json!(v.as_u64().unwrap_or(0) + delta);
    };
    bump(entry.get_mut("calls").expect("calls"), 1);
    if ok {
        bump(entry.get_mut("ok").expect("ok"), 1);
        entry.insert("lastOkTs".to_string(), json!(now_ms()));
    } else {
        bump(entry.get_mut("fails").expect("fails"), 1);
        let reason_str = clip_chars(reason.unwrap_or("неизвестная ошибка"), 200);
        entry.insert("lastFailTs".to_string(), json!(now_ms()));
        entry.insert("lastFailReason".to_string(), json!(reason_str));
        let category = categorize_reason(&reason_str);
        let reasons = entry.get_mut("reasons").expect("reasons").as_object_mut().expect("reasons объект");
        let count = reasons.get(category).and_then(|v| v.as_u64()).unwrap_or(0);
        reasons.insert(category.to_string(), json!(count + 1));
    }
    save_to(dir, &data);
}

/// Верхушка строки (port `String(rate).slice(0, 200)`).
fn clip_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// Краткая сводка по движкам — порт `statsSummary` (сортировка по доле
/// отказов, самые проблемные первыми).
pub fn stats_summary() -> String {
    let _guard = WRITE_LOCK.lock();
    summary_from(&data_dir())
}

fn summary_from(dir: &Path) -> String {
    let data = load_from(dir);
    let obj = data.as_object().expect("корень объект");
    let mut entries: Vec<(&String, &serde_json::Map<String, Value>)> = obj
        .iter()
        .map(|(k, v)| (k, v.as_object().expect("запись движка объект")))
        .filter(|(_, s)| s.get("calls").and_then(|v| v.as_u64()).unwrap_or(0) > 0)
        .collect();
    if entries.is_empty() {
        return "статистика поисковых движков пуста".to_string();
    }
    entries.sort_by(|a, b| {
        let fail_ratio = |s: &serde_json::Map<String, Value>| {
            let calls = s.get("calls").and_then(|v| v.as_u64()).unwrap_or(1) as f64;
            let fails = s.get("fails").and_then(|v| v.as_u64()).unwrap_or(0) as f64;
            fails / calls
        };
        fail_ratio(b.1)
            .partial_cmp(&fail_ratio(a.1))
            .expect("f64 без NaN")
    });
    let lines: Vec<String> = entries
        .iter()
        .map(|(name, s)| {
            let calls = s.get("calls").and_then(|v| v.as_u64()).unwrap_or(0);
            let ok = s.get("ok").and_then(|v| v.as_u64()).unwrap_or(0);
            let fails = s.get("fails").and_then(|v| v.as_u64()).unwrap_or(0);
            let fail_pct = if calls > 0 { (fails as f64 / calls as f64 * 100.0).round() as u64 } else { 0 };
            // Топ-2 причин по счётчику (при равном счётчике — алфавит, см. шапку).
            let mut reasons: Vec<(String, u64)> = s
                .get("reasons")
                .and_then(|v| v.as_object())
                .map(|m| m.iter().map(|(k, v)| (k.clone(), v.as_u64().unwrap_or(0))).collect())
                .unwrap_or_default();
            reasons.sort_by(|a, b| b.1.cmp(&a.1));
            let top = reasons
                .into_iter()
                .take(2)
                .map(|(t, n)| format!("{t}×{n}"))
                .collect::<Vec<_>>()
                .join(", ");
            let last_fail_ts = s.get("lastFailTs").and_then(|v| v.as_u64());
            let last = match last_fail_ts {
                Some(ts) => {
                    let reason_clip = clip_chars(
                        s.get("lastFailReason").and_then(|v| v.as_str()).unwrap_or_default(),
                        60,
                    );
                    format!(", посл. сбой: {} ({local})", fmt_ru(ts), local = reason_clip)
                }
                None => String::new(),
            };
            let top_part = if top.is_empty() { String::new() } else { format!(", причины: {top}") };
            format!("{name}: вызовов {calls}, ок {ok}, сбоев {fails} ({fail_pct}%){top_part}{last}")
        })
        .collect();
    lines.join("\n")
}

/// `new Date(ts).toLocaleString('ru-RU')` → «дд.мм.гггг, чч:мм:сс».
fn fmt_ru(ts_ms: u64) -> String {
    chrono::Local
        .timestamp_millis_opt(ts_ms as i64)
        .single()
        .map(|d| d.format("%d.%m.%Y, %H:%M:%S").to_string())
        .unwrap_or_else(|| format!("{ts_ms}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> std::path::PathBuf {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!(
            "agent-tools-stats-{}-{}",
            std::process::id(),
            seq
        ));
        let _ = std::fs::create_dir_all(&d);
        d
    }

    fn clean(name: &str) -> std::path::PathBuf {
        let dir = temp_dir();
        let mut p = dir.clone();
        p.push(name);
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn reasons_are_categorized_in_ts_order() {
        // Регулярки 1-в-1 с TS (search_stats.ts:55-64): 'rate limit' — с пробелом;
        // слова 'антибот' в TS-списке нет → (521) уходит в other.
        assert_eq!(categorize_reason("лимит запросов (rate limit)"), "rate-limit");
        assert_eq!(categorize_reason("429"), "rate-limit");
        assert_eq!(categorize_reason("лимит запросов (rate-limit)"), "other");
        assert_eq!(categorize_reason("Cloudflare: нужен браузер"), "antibot");
        assert_eq!(categorize_reason("антибот-защита сайта (521)"), "other");
        assert_eq!(categorize_reason("заблокирован (капча)"), "captcha");
        assert_eq!(categorize_reason("HTTP 451"), "geo");
        assert_eq!(categorize_reason("доступ запрещён (403)"), "forbidden");
        assert_eq!(categorize_reason("движок завис (дедлайн)"), "deadline");
        assert_eq!(categorize_reason("Не удалось разрешить DNS"), "network");
        assert_eq!(categorize_reason("err_no=301"), "http");
        assert_eq!(categorize_reason("что-то совсем другое"), "other");
    }

    #[test]
    fn record_engine_call_writes_ts_shape_and_summary_sorts() {
        let p = clean(STATS_FILE);
        let dir = p.parent().expect("в temp_dir");

        // Детерминированные метки времени через тестовый крюк.
        *TEST_NOW.lock().expect("крюк") = Some(777);
        record_into(dir, "brave", true, None);
        *TEST_NOW.lock().expect("крюк") = Some(888);
        // Пробел — как в TS-регулярке (rate limit, не rate-limit).
        record_into(dir, "brave", false, Some("лимит запросов (rate limit)"));
        *TEST_NOW.lock().expect("крюк") = Some(900);
        record_into(dir, "brave", false, Some("заблокирован (капча)"));
        *TEST_NOW.lock().expect("крюк") = Some(999);
        record_into(dir, "exa", true, None);
        *TEST_NOW.lock().expect("крюк") = None;

        let data: Value =
            serde_json::from_str(&std::fs::read_to_string(&p).expect("файл записан")).expect("json");
        let brave = &data["brave"];
        assert_eq!(brave["calls"], json!(3));
        assert_eq!(brave["ok"], json!(1));
        assert_eq!(brave["fails"], json!(2));
        assert_eq!(brave["lastOkTs"], json!(777));
        assert_eq!(brave["lastFailTs"], json!(900));
        // Последний отказ по ts=900 — капча, не rate-limit.
        assert_eq!(brave["lastFailReason"], json!("заблокирован (капча)"));
        assert_eq!(brave["reasons"]["rate-limit"], json!(1));
        assert_eq!(brave["reasons"]["captcha"], json!(1));

        // exa единственный без сбоев → в сводке последним, brave первым.
        let summary = summary_from(dir);
        let mut lines = summary.lines();
        assert!(lines.next().expect("строка brave").starts_with("brave: вызовов 3, ок 1"));
        assert!(lines.next().expect("строка exa").starts_with("exa: вызовов 1, ок 1"));
    }

    #[test]
    fn empty_stats_summary_is_honest() {
        let p = clean(STATS_FILE);
        let dir = p.parent().expect("в temp_dir").to_path_buf();
        assert_eq!(summary_from(&dir), "статистика поисковых движков пуста");
    }
}
