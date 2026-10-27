//! Локальный счётчик бесплатных (`:free`) запросов OpenRouter.
//!
//! ## Зачем
//!
//! `/api/v1/key` НЕ отдаёт остаток дневной бесплатной квоты (лимит аккаунтный
//! дневной, API показывает только потраченные доллары) — единственный способ
//! показать «осталось N бесплатных запросов сегодня» — считать запросы самим
//! шлюзом. Счёт живёт в файле `<data_dir>/free_usage.json`, атомарная запись
//! на каждый инкремент (чат-запросы не частые, файл — байты).
//!
//! ## Границы точности (честные, не скрываем)
//!
//! - Неудачные попытки засчитываются апстримом — счётчик считает так же
//!   ([`counts_toward_free_limit`] вызывается на каждую попытку, не на успех).
//! - Запросы из других клиентов/поштучно мимо шлюза не видны.
//!   Точность даёт только коррекция по заголовкам платформенного 429
//!   ([`FreeUsageState::correct`]) — best-effort: заголовков нет, счёт не
//!   меняется, инкремент уже сделан.
//!
//! ## Ключ дня
//!
//! UTC-день `ГГГГ-ММ-ДД` ([`today_ymd`]) — общий календарь шлюза: без chrono,
//! по алгоритму Хиннанта. Запись с чужим днём считается нулём — счётчик
//! не переносит остаток на завтра.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use cloud_routers_core::config::ProviderConfig;
use serde::{Deserialize, Serialize};

/// Сегодняшний UTC-день `ГГГГ-ММ-ДД`.
///
/// Без `chrono`: в шлюзе календарных часов нет, а тащить зависимость ради
/// одной строки запрещено минимализмом. Считаем дни от эпохи Unix. Раньше
/// жила в `admin.rs` (для `updated_at` каталога) — поднята сюда, когда
/// счётчику бесплатных запросов понадобился тот же ключ дня.
pub fn today_ymd() -> String {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0);
    let (y, m, d) = civil_from_days(days as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Дни от эпохи Unix → (год, месяц, день) — алгоритм «civil_from_days»
/// Говарда Хиннанта. Чистая функция: покрывается точными тестами на
/// известные дни (в отличие от обёртки над `now()`). Точный порт из
/// `admin.rs` — включая guard отрицательного z (trunc-деление Rust ≠ floor).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

/// Чистый предикат хука прокси: запрос этой модели у этого провайдера
/// расходует дневную бесплатную квоту OpenRouter?
///
/// `openrouter/free` — конвенция каталога, `…:free` — конвенция моделей
/// самого OpenRouter (`meta-llama/llama-3.3-70b-instruct:free`). Платные
/// модели и чужие провайдеры счётчик не трогают — их расход не дневной.
pub fn counts_toward_free_limit(provider: &ProviderConfig, upstream_model: &str) -> bool {
    crate::openrouter_check::is_openrouter_provider(provider)
        && (upstream_model == "openrouter/free" || upstream_model.ends_with(":free"))
}

/// Счёт одного провайдера за день.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
struct ProviderUsage {
    ymd: String,
    count: u64,
}

/// Файл состояния: `{ "providers": { "<provider_id>": { "ymd": "…", "count": N } } }`.
#[derive(Debug, Default, Serialize, Deserialize)]
struct FreeUsageFile {
    providers: BTreeMap<String, ProviderUsage>,
}

/// Состояние счётчика: путь к файлу + сериализация read-modify-write.
///
/// `Mutex` обязателен: два параллельных чат-запроса без него потеряли бы
/// инкремент (оба прочитали N, оба записали N+1).
pub struct FreeUsageState {
    file: PathBuf,
    io: Mutex<()>,
}

impl FreeUsageState {
    pub fn new(data_dir: &Path) -> Self {
        Self { file: data_dir.join("free_usage.json"), io: Mutex::new(()) }
    }

    /// Прочитать файл. Ошибка чтения — не молчание: `warn!` с причиной и
    /// работа дальше с нулевым счётом (счётчик — диагностика, а не биллинг:
    /// отказ не должен ломать чат).
    fn read(&self) -> FreeUsageFile {
        let _guard = self.io.lock().unwrap_or_else(|p| p.into_inner());
        match std::fs::read_to_string(&self.file) {
            Ok(text) if text.trim().is_empty() => FreeUsageFile::default(),
            Ok(text) => match serde_json::from_str(&text) {
                Ok(f) => f,
                Err(e) => {
                    log::warn!(
                        "cloud-routers-gateway: {}: нечитаемый файл счётчика бесплатных запросов ({e}) — счёт начнётся с нуля",
                        self.file.display()
                    );
                    FreeUsageFile::default()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => FreeUsageFile::default(),
            Err(e) => {
                log::warn!(
                    "cloud-routers-gateway: {}: не удалось прочитать счётчик бесплатных запросов ({e}) — счёт начнётся с нуля",
                    self.file.display()
                );
                FreeUsageFile::default()
            }
        }
    }

    /// Записать файл атомарно (как `gateway.json` — tmp + rename). Ошибка
    /// записи — `warn!`, счёт в памяти вызова продолжается с новым значением,
    /// но файл мог не обновиться: коррекция 429 выправит.
    fn write(&self, f: &FreeUsageFile) {
        let json = match serde_json::to_string_pretty(f) {
            Ok(j) => j,
            Err(e) => {
                log::warn!("cloud-routers-gateway: не удалось сериализовать счётчик бесплатных запросов: {e}");
                return;
            }
        };
        if let Err(e) = cloud_routers_core::store::write_atomic(&self.file, &json) {
            log::warn!(
                "cloud-routers-gateway: {}: не удалось записать счётчик бесплатных запросов ({e})",
                self.file.display()
            );
        }
    }

    /// Сколько бесплатных запросов провайдер сделал за текущий UTC-день.
    pub fn today_count(&self, provider_id: &str) -> u64 {
        let f = self.read();
        f.providers
            .get(provider_id)
            .filter(|u| u.ymd == today_ymd())
            .map(|u| u.count)
            .unwrap_or(0)
    }

    /// Засчитать одну попытку `:free`-запроса.
    pub fn increment(&self, provider_id: &str) {
        let _guard = self.io.lock().unwrap_or_else(|p| p.into_inner());
        let mut f = match std::fs::read_to_string(&self.file) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => FreeUsageFile::default(),
        };
        let today = today_ymd();
        let u = f.providers.entry(provider_id.to_string()).or_default();
        if u.ymd != today {
            // Новый UTC-день: счёт обнуляется, остаток не переносится.
            u.ymd = today;
            u.count = 0;
        }
        u.count = u.count.saturating_add(1);
        self.write(&f);
    }

    /// Синхронизировать счёт по заголовкам платформенного 429.
    ///
    /// Платформа — истина о расходе: `использовано = limit - remaining`.
    /// Меньше всех прочих источников погрешности — здесь же лежат и попытки,
    /// сделанные мимо шлюза. Вызов без заголовков — это наш инкремент и так
    /// посчитал попытку; вызов с частично нечитаемыми заголовками — ничего
    /// не меняем (half-таблица не истина).
    pub fn correct(&self, provider_id: &str, limit: u64, remaining: u64) {
        let _guard = self.io.lock().unwrap_or_else(|p| p.into_inner());
        let mut f = match std::fs::read_to_string(&self.file) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => FreeUsageFile::default(),
        };
        let used = limit.saturating_sub(remaining);
        let u = f.providers.entry(provider_id.to_string()).or_default();
        u.ymd = today_ymd();
        u.count = used;
        self.write(&f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(tag: &str) -> FreeUsageState {
        let dir = std::env::temp_dir().join(format!("cr_gw_free_{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        FreeUsageState::new(&dir)
    }

    fn provider(base: &str) -> ProviderConfig {
        ProviderConfig {
            id: "openrouter".into(),
            name: "OpenRouter".into(),
            base_url: base.into(),
            ..Default::default()
        }
    }

    #[test]
    fn today_ymd_format_and_known_day() {
        let ymd = today_ymd();
        assert_eq!(ymd.len(), 10, "{ymd}");
        assert_eq!(ymd.as_bytes()[4], b'-', "{ymd}");
        assert_eq!(ymd.as_bytes()[7], b'-', "{ymd}");
    }

    #[test]
    fn civil_from_days_matches_known_dates() {
        // Точный тест Хиннанта на фиксированных днях (не зависят от now()).
        assert_eq!(civil_from_days(0), (1970, 1, 1), "эпоха Unix");
        assert_eq!(civil_from_days(20_753), (2026, 10, 27), "день написания счётчика");
        assert_eq!(civil_from_days(19_723), (2024, 1, 1), "граница года");
        assert_eq!(civil_from_days(11_016), (2000, 2, 29), "високосный февраль");
        assert_eq!(civil_from_days(365), (1971, 1, 1), "ровно год");
        // Сегодняшний ymd согласован с days-счётом.
        let days = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64 / 86_400)
            .unwrap_or(0);
        let (y, m, d) = civil_from_days(days);
        assert_eq!(today_ymd(), format!("{y:04}-{m:02}-{d:02}"));
    }

    #[test]
    fn increment_persists_and_resets_on_new_day() {
        let s = state("persist");
        assert_eq!(s.today_count("openrouter"), 0);
        for _ in 0..3 {
            s.increment("openrouter");
        }
        assert_eq!(s.today_count("openrouter"), 3);
        // Файл существует и перечитывается новым состоянием (перезапуск шлюза).
        let s2 = FreeUsageState::new(s.file.parent().expect("dir"));
        assert_eq!(s2.today_count("openrouter"), 3);

        // Подделка дня на старый — счёт «сегодня» нулевой (день сменился).
        let mut file: FreeUsageFile =
            serde_json::from_str(&std::fs::read_to_string(&s.file).expect("read")).expect("json");
        file.providers.get_mut("openrouter").expect("entry").ymd = "2000-01-01".into();
        std::fs::write(&s.file, serde_json::to_string(&file).expect("ser")).expect("write");
        assert_eq!(s.today_count("openrouter"), 0, "чужой день — счёт нулевой");
        // Инкремент после смены дня начинается с единицы, а не с 4.
        s.increment("openrouter");
        assert_eq!(s.today_count("openrouter"), 1);
        let _ = std::fs::remove_dir_all(s.file.parent().expect("dir"));
    }

    #[test]
    fn correct_takes_platform_truth_and_starts_fresh_day() {
        let s = state("correct");
        for _ in 0..5 {
            s.increment("openrouter");
        }
        // Платформа говорит: лимит 10, осталось 2 → использовано 8. Наш счёт
        // (5) был занижен — запросы мимо шлюза или неточность: платформа права.
        s.correct("openrouter", 10, 2);
        assert_eq!(s.today_count("openrouter"), 8);
        // remaining > limit — некорректные заголовки: saturating не даст уйти в минус.
        s.correct("openrouter", 10, 15);
        assert_eq!(s.today_count("openrouter"), 0);
        // Коррекция на новом дне (файл мог остаться со вчерашним ymd) —
        // выставляет сегодняший.
        let mut file: FreeUsageFile =
            serde_json::from_str(&std::fs::read_to_string(&s.file).expect("read")).expect("json");
        file.providers.get_mut("openrouter").expect("entry").ymd = "2000-01-01".into();
        std::fs::write(&s.file, serde_json::to_string(&file).expect("ser")).expect("write");
        s.correct("openrouter", 10, 7);
        assert_eq!(s.today_count("openrouter"), 3);
        let _ = std::fs::remove_dir_all(s.file.parent().expect("dir"));
    }

    #[test]
    fn counts_only_openrouter_free_models() {
        let or = provider("https://openrouter.ai/api/v1");
        assert!(counts_toward_free_limit(&or, "openrouter/free"));
        assert!(counts_toward_free_limit(&or, "meta-llama/llama-3.3-70b-instruct:free"));
        assert!(!counts_toward_free_limit(&or, "openrouter/gpt-4o"), "платная модель");
        assert!(!counts_toward_free_limit(&or, "free"), "просто «free» без двоеточия — не конвенция :free");

        let groq = provider("https://api.groq.com/openai/v1");
        assert!(!counts_toward_free_limit(&groq, "llama:free"), "чужой провайдер");
        let proxy_host = provider("http://127.0.0.1:9/mirror/openrouter.ai/api/v1");
        assert!(
            counts_toward_free_limit(&proxy_host, "m:free"),
            "детектор по подстроке — совпадает и с зеркалом пути: осознанный компромисс SSOT"
        );
    }

    #[test]
    fn unreadable_file_does_not_break_increment() {
        let s = state("broken");
        // Файл-мусор: чтение даёт нулевой счёт с warn, инкремент переписывает его.
        std::fs::write(&s.file, "{not json").expect("write garbage");
        assert_eq!(s.today_count("openrouter"), 0);
        s.increment("openrouter");
        assert_eq!(s.today_count("openrouter"), 1, "инкремент восстановил файл");
        let _ = std::fs::remove_dir_all(s.file.parent().expect("dir"));
    }
}
