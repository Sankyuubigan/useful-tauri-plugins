//! Сетевые источники каталога провайдеров.
//!
//! ## Почему список, а не один URL
//!
//! Один URL — это одна точка отказа: файл переехал (как случилось с
//! `Sankyuubigan/useful-tauri-plugins/.../provider-presets.json` → 404) — и
//! каталог мёртв, а вместе с ним и кнопка «Обновить». Список источников с
//! обходом по очереди делает такой отказ невозможным конструктивно: первый
//! валидный выигрывает.
//!
//! ## Почему checks по содержимому, а не по имени источника
//!
//! Внешние источники не публикуют `default_base_url` для всех провайдеров
//! (проверено 2026-10-16: в `models.dev/api.json` из ~230 провайдеров поля
//! `api` нет у `google`, `groq`, `cerebras`, `openai`, `anthropic`, `mistral`,
//! `cohere`, `xai`, `togetherai`, `azure`). Поэтому недостающие base_url берутся
//! из `assets/base_url_overrides.json`, а не «догадываются» — см. [`adapters`].
//!
//! Формат каталога для **нашего** кэша версионируется полем `version`
//! (см. [`super::MIN_SUPPORTED_VERSION`]) и парсится [`super::parse`].

use super::{ProviderPreset, ProviderPresetsCatalog, is_valid_provider_id};

/// Основной источник: провайдеры, их endpoints и модели.
///
/// Формат: `{ "<id>": { "id", "env", "npm", "name", "doc", "api"?, "models": {...} } }`.
/// Проверен размер ~5.6 МБ (2026-10): шлюз тянет его отдельным HTTP-запросом с
/// собственным таймаутом, а не через общий клиент прокси (см. `admin.rs`).
pub const MODELS_DEV_API: &str = "https://models.dev/api.json";

/// Фолбэк №1: тот же набор данных, обёрнутый в `{"providers": {...}}`.
pub const MODELS_DEV_CATALOG: &str = "https://models.dev/catalog.json";

/// Фолбэк №2: характеристики моделей (контекст, capabilities, цены).
///
/// Провайдеров и base_url здесь нет — только плоская карта `"провайдер/модель"`
/// → метаданные. Используется для `default_models_hint`, а не для каталога.
pub const MODELS_DEV_MODELS: &str = "https://models.dev/models.json";

/// Фолбэк №3: цены и характеристики моделей из LiteLLM.
///
/// Проверено 2026-10-16: **ни одна** из ~4100 записей не содержит `api_base` —
/// как каталог провайдеров этот файл не годится. Только подсказки моделей.
pub const LITELLM_PRICES: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";

/// Источники по умолчанию — по убыванию приоритета.
///
/// `MODELS_DEV_API` первый: единственный источник, который даёт и провайдеров,
/// и base_url (у ~205 из ~230). Остальные — фолбэки.
pub const DEFAULT_SOURCES: &[&str] =
    &[MODELS_DEV_API, MODELS_DEV_CATALOG, MODELS_DEV_MODELS, LITELLM_PRICES];

/// Таймаут одного источника, секунды.
///
/// Отдельный от прокси-таймаутов: файл `api.json` весит ~5.6 МБ, на медленной
/// сети 20 секунд соединения может не хватить, а вечное ожидание превратило бы
/// кнопку «Обновить» в вечный спиннер. 90 секунд — осознанный компромисс.
pub const SOURCE_TIMEOUT_SECS: u64 = 90;

/// Сколько id моделей класть в `default_models_hint`.
///
/// Подсказка — не источник истины (модели берутся реальным `GET /models`),
/// поэтому весь список из сотен id не нужен: он раздул бы кэш и выпадающий
/// список без пользы.
pub const MODELS_HINT_LIMIT: usize = 50;

/// Переопределения base_url для провайдеров, которые его не публикуют.
///
/// Файл лежит в исходниках крейта, каждый URL — со ссылкой-доказательством.
/// Это данные, а не логика: обновление не требует выпуска новой версии шлюза,
/// достаточно обновить файл и пересобрать крейт.
///
/// Почему нельзя «просто взять из models.dev»: проверено 2026-10-16 — у
/// `google`, `groq`, `cerebras`, `openai`, `anthropic`, `mistral`, `cohere`,
/// `xai`, `togetherai`, `azure` поля `api` там нет.
pub const BASE_URL_OVERRIDES: &str = include_str!("../../assets/base_url_overrides.json");

/// Одна запись таблицы переопределений.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct BaseUrlOverride {
    /// id провайдера в нашем каталоге (уже slug).
    pub id: String,
    /// Отображаемое имя.
    pub name: String,
    /// Базовый URL OpenAI-совместимого API.
    pub base_url: String,
    /// Доказательство: откуда взят URL (дока или живой ответ сервера).
    /// Пустое запрещено — URL без доказательства в каталог не попадает.
    pub proof: String,
    /// Ссылка на документацию провайдера.
    #[serde(default)]
    pub api_docs_url: Option<String>,
}

impl BaseUrlOverride {
    /// Запись таблицы как пресет каталога. Протокол — OpenAI: overrides
    /// покрывают только OpenAI-совместимые endpoints (см. `_note` в JSON).
    pub fn as_preset(&self) -> ProviderPreset {
        ProviderPreset {
            id: self.id.clone(),
            name: self.name.clone(),
            default_base_url: self.base_url.clone(),
            api_docs_url: self.api_docs_url.clone(),
            icon_url: None,
            default_models_hint: None,
            protocol: crate::protocol::VendorProtocol::Openai,
        }
    }
}

/// Слить внешний источник в наш каталог.
///
/// Правила слияния — следствие проверенных фактов, а не вкуса:
///
/// 1. **base_url: curated побеждает внешний при конфликте.** Наши файлы
///    (`default_providers.json`, overrides) содержат значения, с которыми
///    провайдеры реально работают у пользователей; внешние значения
///    непроверяемы без ключа (доказано 2026-10-16: `nebius` в models.dev ведёт
///    на `tokenfactory`, `novita-ai` — на другой путь, чем наш рабочий;
///    проверить без ключа нельзя — оба дают 403). Сломать рабочую маршрутизацию
///    «свежим» URL хуже, чем оставить рабочее. Расхождение пишется в `warn!`,
///    чтобы человек обновил curated-файл.
/// 2. **Нет curated base_url — берём пригодный внешний.** Пригодный =
///    начинается с `http(s)://` и не содержит `${` (шаблоны `neon`,
///    `databricks` — не литеральный URL). Без этого `google`, `groq`,
///    `cerebras` (у которых в models.dev поля `api` нет) пропали бы вовсе —
///    то есть фикс вернул бы баг «нет Джемини» в новом виде: для них base_url
///    дают overrides.
/// 3. **Ни того, ни другого — провайдер отбрасывается** (с `warn!`). Каталог
///    с пустым `base_url` не пройдёт `validate`, а браковать весь каталог
///    из-за одного провайдера нельзя (см. [`super::validate`]).
/// 4. **Имя и документация — из внешнего источника** (свежее наших файлов),
///    fallback — curated, затем id. Устаревшее имя маршрутизацию не ломает.
/// 5. **Модели — объединение**: сначала внешнего источника, затем `extra_hints`
///    (подсказки litellm), с обрезкой до [`MODELS_HINT_LIMIT`].
/// 6. **Curated-провайдеры, которых нет во внешнем источнике, остаются**:
///    локальные `ollama`, `llamacpp`, `vllm` в models.dev отсутствуют
///    (проверено). Без этого sync удалял бы локальные движки.
///
/// `updated_at` ставит вызывающий (шлюз знает момент sync; в `core` нет часов
/// с календарём — только `std::time`, и тащить `chrono` ради одной строки
/// запрещено минимализмом зависимостей крейта).
pub fn merge_catalogs(
    external: Vec<super::adapters::models_dev::ExternalProvider>,
    extra_hints: &[(String, Vec<String>)],
    updated_at: &str,
) -> Result<ProviderPresetsCatalog, String> {
    let overrides = base_url_overrides()?;
    let override_presets: Vec<ProviderPreset> = overrides.iter().map(|o| o.as_preset()).collect();
    let hints_by_id: std::collections::HashMap<&str, &[String]> =
        extra_hints.iter().map(|(id, m)| (id.as_str(), m.as_slice())).collect();

    let mut providers: Vec<ProviderPreset> = Vec::with_capacity(external.len() + 8);
    let mut seen: Vec<String> = Vec::with_capacity(external.len() + 8);

    // Curated-слой: встроенный seed + overrides. Ключ — id провайдера.
    let seed = super::embedded();
    let mut curated: std::collections::HashMap<&str, &ProviderPreset> = std::collections::HashMap::with_capacity(
        seed.providers.len() + override_presets.len(),
    );
    for p in &seed.providers {
        curated.insert(p.id.as_str(), p);
    }
    for p in &override_presets {
        curated.insert(p.id.as_str(), p);
    }

    for ext in &external {
        let usable_api = ext
            .api
            .as_deref()
            .map(str::trim)
            .filter(|u| !u.is_empty())
            .filter(|u| u.starts_with("http://") || u.starts_with("https://"))
            .filter(|u| !u.contains("${"))
            .map(super::super::config::trim_base_url);
        let curated_preset = curated.get(ext.id.as_str()).copied();

        // Правило 1: curated побеждает при конфликте. Расхождение — в warn,
        // чтобы человек обновил curated-файл, а не чтобы шлюз молча переехал.
        let base_url: Option<String> = match (curated_preset, usable_api) {
            (Some(cur), Some(ext_url)) => {
                if cur.default_base_url != ext_url {
                    log::warn!(
                        "cloud-routers: base_url «{}» расходится: curated «{}», источник «{}» — оставлен curated",
                        ext.id,
                        cur.default_base_url,
                        ext_url
                    );
                }
                Some(cur.default_base_url.clone())
            }
            (Some(cur), None) => Some(cur.default_base_url.clone()),
            (None, Some(ext_url)) => Some(ext_url),
            (None, None) => None,
        };
        let Some(base_url) = base_url else {
            log::warn!(
                "cloud-routers: провайдер «{}» пропущен: нет base_url ни в источнике, ни в curated",
                ext.id
            );
            continue;
        };

        let name = if !ext.name.trim().is_empty() {
            ext.name.clone()
        } else {
            curated_preset.map(|p| p.name.clone()).unwrap_or_else(|| ext.id.clone())
        };
        let api_docs_url = ext
            .api_docs_url
            .clone()
            .or_else(|| curated_preset.and_then(|p| p.api_docs_url.clone()));

        let mut models: Vec<String> = Vec::with_capacity(MODELS_HINT_LIMIT);
        for m in ext.models.iter().chain(hints_by_id.get(ext.id.as_str()).copied().unwrap_or(&[])) {
            let m = m.trim();
            if m.is_empty() || models.iter().any(|x| x == m) {
                continue;
            }
            if models.len() >= MODELS_HINT_LIMIT {
                break;
            }
            models.push(m.to_string());
        }

        seen.push(ext.id.clone());
        providers.push(ProviderPreset {
            id: ext.id.clone(),
            name,
            default_base_url: base_url,
            api_docs_url,
            icon_url: None,
            default_models_hint: if models.is_empty() { None } else { Some(models) },
            // Внешние каталоги протокола не знают: их провайдеры — OpenAI по
            // определению (иначе base_url из них был бы непригоден). Kiro
            // приходит только curated-путём (шаг 4/5).
            protocol: crate::protocol::VendorProtocol::Openai,
        });
    }

    // Правило 6: curated-провайдеры, которых нет во внешнем источнике
    // (локальные движки `ollama`/`llamacpp`/`vllm` и curated overrides).
    // Без этого sync удалял бы локальные движки.
    for p in seed.providers.iter().chain(override_presets.iter()) {
        if !seen.iter().any(|id| id == &p.id) {
            seen.push(p.id.clone());
            providers.push(p.clone());
        }
    }

    providers.sort_by(|a, b| a.id.cmp(&b.id));
    let catalog = ProviderPresetsCatalog {
        version: super::MIN_SUPPORTED_VERSION,
        updated_at: updated_at.to_string(),
        providers,
    };
    super::validate(&catalog)?;
    Ok(catalog)
}

/// Таблица переопределений base_url.
///
/// Парсинг строгий, без `expect` в пользу вызывающего: битый файл в исходниках —
/// ошибка сборки/репозитория, и вызывающий решает, падать ли. Возвращаются сами
/// записи (с `proof`), а не готовые пресеты: доказательство URL — часть записи,
/// и терять его конвертацией запрещено.
pub fn base_url_overrides() -> Result<Vec<BaseUrlOverride>, String> {
    let items: Vec<BaseUrlOverride> = serde_json::from_str(BASE_URL_OVERRIDES)
        .map_err(|e| format!("base_url_overrides.json не является JSON: {}", e))?;
    for item in &items {
        if !is_valid_provider_id(&item.id) {
            return Err(format!("Некорректный id в overrides: «{}»", item.id));
        }
        if !(item.base_url.starts_with("http://") || item.base_url.starts_with("https://")) {
            return Err(format!("У «{}» base_url без схемы: «{}»", item.id, item.base_url));
        }
        if item.proof.trim().is_empty() {
            return Err(format!("У «{}» нет доказательства URL (поле proof)", item.id));
        }
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::super::adapters::models_dev::ExternalProvider;
    use super::*;

    fn ext(id: &str, api: Option<&str>) -> ExternalProvider {
        ExternalProvider {
            id: id.to_string(),
            name: format!("{id} ext"),
            api: api.map(|s| s.to_string()),
            api_docs_url: Some(format!("https://docs.{id}.example")),
            models: vec!["m1".to_string(), "m2".to_string()],
        }
    }

    #[test]
    fn curated_wins_on_conflict() {
        // `groq` есть и в seed, и в overrides с curated-URL. Внешний источник
        // даёт другой URL — побеждает curated, маршрутизация не ломается.
        let external = vec![ext("groq", Some("https://some-new-groq.example/v1"))];
        let c = merge_catalogs(external, &[], "2026-10-16").expect("merge");
        let groq = c.providers.iter().find(|p| p.id == "groq").expect("groq");
        assert_eq!(groq.default_base_url, "https://api.groq.com/openai/v1");
    }

    #[test]
    fn external_url_used_when_no_curated() {
        let external = vec![ext("brand-new", Some("https://api.brand-new.example/v1"))];
        let c = merge_catalogs(external, &[], "2026-10-16").expect("merge");
        let p = c.providers.iter().find(|p| p.id == "brand-new").expect("brand-new");
        assert_eq!(p.default_base_url, "https://api.brand-new.example/v1");
        assert_eq!(p.api_docs_url.as_deref(), Some("https://docs.brand-new.example"));
        assert_eq!(p.default_models_hint.as_deref(), Some(&["m1".to_string(), "m2".to_string()][..]));
    }

    #[test]
    fn provider_without_any_base_url_is_dropped_not_fatal() {
        let external = vec![
            ext("no-url", None),
            ext("templated", Some("https://${SOME_HOST}/v1")),
            ext("ok-one", Some("https://ok-one.example/v1")),
        ];
        let c = merge_catalogs(external, &[], "2026-10-16").expect("merge");
        assert!(!c.providers.iter().any(|p| p.id == "no-url"));
        assert!(!c.providers.iter().any(|p| p.id == "templated"));
        assert!(c.providers.iter().any(|p| p.id == "ok-one"));
    }

    #[test]
    fn seed_providers_missing_externally_survive() {
        // Внешний источник пуст по локальным движкам — `ollama` обязан остаться.
        let external = vec![ext("some-cloud", Some("https://some-cloud.example/v1"))];
        let c = merge_catalogs(external, &[], "2026-10-16").expect("merge");
        let ollama = c.providers.iter().find(|p| p.id == "ollama").expect("ollama survives");
        assert_eq!(ollama.default_base_url, "http://localhost:11434/v1");
    }

    #[test]
    fn overrides_fill_missing_external_api() {
        // `google` — ключевой кейс задачи: в models.dev поля `api` нет,
        // base_url обязан прийти из overrides.
        let external = vec![ext("google", None)];
        let c = merge_catalogs(external, &[], "2026-10-16").expect("merge");
        let google = c.providers.iter().find(|p| p.id == "google").expect("google");
        assert_eq!(google.default_base_url, "https://generativelanguage.googleapis.com/v1beta/openai");
    }

    #[test]
    fn litellm_hints_are_merged_and_capped() {
        let external = vec![ext("some-cloud", Some("https://some-cloud.example/v1"))];
        let extra = vec![(
            "some-cloud".to_string(),
            vec!["m2".to_string(), "m3".to_string(), "  ".to_string()],
        )];
        let c = merge_catalogs(external, &extra, "2026-10-16").expect("merge");
        let p = c.providers.iter().find(|p| p.id == "some-cloud").expect("some-cloud");
        // m1, m2 из источника + m3 из litellm; дубликат m2 и пустая строка отброшены.
        assert_eq!(
            p.default_models_hint.as_deref(),
            Some(&["m1".to_string(), "m2".to_string(), "m3".to_string()][..])
        );
    }

    #[test]
    fn overrides_table_is_valid() {
        let ov = base_url_overrides().expect("overrides parse");
        assert!(ov.iter().any(|o| o.id == "google"), "без google задача не решена");
        assert!(ov.iter().any(|o| o.id == "groq"), "без groq сломаем рабочую маршрутизацию");
        assert!(ov.iter().any(|o| o.id == "cerebras"));
        for o in &ov {
            // `proof` уже проверен парсером; здесь — инвариант таблицы как данных.
            assert!(!o.proof.trim().is_empty(), "{}: нет доказательства URL", o.id);
            assert!(o.base_url.starts_with("https://"), "{}", o.id);
        }
    }

    #[test]
    fn default_sources_list_starts_with_models_dev() {
        assert_eq!(DEFAULT_SOURCES[0], MODELS_DEV_API);
        assert!(DEFAULT_SOURCES.contains(&LITELLM_PRICES), "litellm — фолбэк подсказок");
    }
}

