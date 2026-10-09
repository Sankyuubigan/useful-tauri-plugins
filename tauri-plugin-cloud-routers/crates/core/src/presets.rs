//! Каталог пресетов провайдеров: встроенный fallback + сетевая синхронизация.
//!
//! ## Зачем
//!
//! Чтобы софт жил годами без обновлений, каталог провайдеров (имя, `base_url`)
//! **не должен быть зашит в код**. Иначе появление нового провайдера или смена
//! чужого `base_url` требуют выпуска новой версии — то есть software update там,
//! где пользователь ждёт кнопку «Обновить».
//!
//! ## Три уровня
//!
//! 1. **Встроенный bundle** (`assets/default_providers.json`, `include_str!`).
//!    Гарантирует рабочий список при первом запуске **без интернета**. Это
//!    начальное зерно, а не источник правды: его предполагается перезаписать.
//! 2. **Локальный кэш** (`<data_dir>/presets_cache.json`). Переживает рестарт
//!    и офлайн. Приоритетнее встроенного bundle.
//! 3. **Сетевой sync** — `GET` JSON по URL из конфига. Ответ валидируется и
//!    только после этого атомарно заменяет кэш. Приложение при неудаче честно
//!    сообщает об ошибке и **продолжает работать** на текущем каталоге.
//!
//! ## Формат — наш собственный
//!
//! Схема `ProviderPreset` принадлежит нам, а не заимствована из LobeChat или
//! Cherry Studio. Причина: у тех проектов своя внутренняя структура, и привязка
//! к ней означала бы, что чужой релиз-цикл (переименование поля, переезд файла)
//! ломает наш парсер. Синхронизируем **наш** JSON из нашего репозитория; формат
//! версионируется полем `version`, а не фактом существования файла.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::store::{presets_cache_path};

/// Встроенный каталог: работает без сети при первом запуске.
pub const EMBEDDED_CATALOG: &str = include_str!("../assets/default_providers.json");

/// URL сетевого sync по умолчанию (raw JSON в репозитории плагинов).
///
/// Переопределяется полем `presets_sync_url` в `gateway.json` — чтобы сломанный
/// или перенесённый источник можно было починить, не выпуская новую версию.
pub const DEFAULT_SYNC_URL: &str = "https://raw.githubusercontent.com/Sankyuubigan/useful-tauri-plugins/main/cloud-routers-gateway/assets/provider-presets.json";

/// Поддерживаемая версия формата каталога.
///
/// Диапазон, а не «минимум»: каталог **из будущего** принять нельзя вслепую —
/// его поля могли поменяться, и мы бы молча потеряли часть провайдеров.
///
/// ## Почему версия не растёт при добавлении провайдеров
///
/// Новые провайдеры и новые base_url — это **данные**, а не формат: версия
/// остаётся прежней, файл обновляется по сети, апдейт софта не нужен. Версия
/// растёт **только** при несовместимом изменении полей (удаление, переименование,
/// смена типа). Новое поле обязано быть `Option`/`#[serde(default)]` — иначе
/// старый шлюз не сможет прочитать каталог и откатится во встроенный bundle.
pub const MIN_SUPPORTED_VERSION: u32 = 1;
/// Верхняя граница поддерживаемой версии.
pub const MAX_SUPPORTED_VERSION: u32 = 1;

/// Один пресет провайдера.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderPreset {
    /// Идентификатор. Только `[a-z0-9_-]`, **без `/`** — разделитель в id комбо.
    pub id: String,
    /// Отображаемое имя.
    pub name: String,
    /// Базовый URL OpenAI-совместимого API.
    pub default_base_url: String,
    /// Ссылка на документацию провайдера.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_docs_url: Option<String>,
    /// Иконка (URL или data-URI).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
    /// Подсказки по моделям на случай офлайна. Не заменяют реальный опрос.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_models_hint: Option<Vec<String>>,
}

impl Default for ProviderPreset {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            default_base_url: String::new(),
            api_docs_url: None,
            icon_url: None,
            default_models_hint: None,
        }
    }
}

/// Каталог пресетов целиком — формат файла сетевого sync и кэша.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderPresetsCatalog {
    /// Версия формата.
    pub version: u32,
    /// Дата обновления каталога (для показа в UI).
    pub updated_at: String,
    /// Провайдеры.
    pub providers: Vec<ProviderPreset>,
}

impl Default for ProviderPresetsCatalog {
    fn default() -> Self {
        Self { version: MIN_SUPPORTED_VERSION, updated_at: String::new(), providers: Vec::new() }
    }
}

/// Откуда взят текущий каталог — для честного показа в UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogSource {
    /// Встроенный bundle (первый запуск, нет кэша).
    Embedded,
    /// Локальный кэш (был успешный sync).
    Cache,
    /// Только что синхронизирован по сети.
    Network,
}

impl CatalogSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            CatalogSource::Embedded => "встроенный",
            CatalogSource::Cache => "кэш",
            CatalogSource::Network => "сеть",
        }
    }
}

/// Допустим ли идентификатор провайдера.
///
/// Первый символ — буква или цифра, дальше буквы/цифры/`-`/`_`. Строгая схема
/// нужна не для красоты: id попадает в `"{id}/{model}"` и в имя папки, поэтому
/// `/`, пробел и пустая строка сделали бы разбор неоднозначным или путь
/// непригодным.
pub fn is_valid_provider_id(id: &str) -> bool {
    if id.is_empty() || id.len() > 64 {
        return false;
    }
    let mut chars = id.chars();
    let first = chars.next().expect("non-empty");
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return false;
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// Привести произвольную строку к допустимому id (slug).
pub fn slugify(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "provider".to_string()
    } else {
        trimmed
    }
}

/// Разобрать и провалидировать каталог из произвольной строки.
pub fn parse(raw: &str) -> Result<ProviderPresetsCatalog, String> {
    let catalog: ProviderPresetsCatalog =
        serde_json::from_str(raw).map_err(|e| format!("Каталог провайдеров не является JSON: {}", e))?;
    validate(&catalog)?;
    Ok(catalog)
}

/// Проверить каталог. Ошибка означает «не принимать файл целиком».
pub fn validate(catalog: &ProviderPresetsCatalog) -> Result<(), String> {
    if catalog.version < MIN_SUPPORTED_VERSION || catalog.version > MAX_SUPPORTED_VERSION {
        return Err(format!(
            "Версия каталога {} вне поддерживаемого диапазона {}..={}",
            catalog.version, MIN_SUPPORTED_VERSION, MAX_SUPPORTED_VERSION
        ));
    }
    if catalog.providers.is_empty() {
        return Err("Каталог не содержит ни одного провайдера".to_string());
    }
    let mut seen: Vec<&str> = Vec::new();
    for p in &catalog.providers {
        if !is_valid_provider_id(&p.id) {
            return Err(format!("Некорректный id провайдера: «{}»", p.id));
        }
        if seen.contains(&p.id.as_str()) {
            return Err(format!("Дубликат id провайдера: «{}»", p.id));
        }
        seen.push(&p.id);
        if p.name.trim().is_empty() {
            return Err(format!("У провайдера «{}» пустое имя", p.id));
        }
        if !(p.default_base_url.starts_with("http://") || p.default_base_url.starts_with("https://")) {
            return Err(format!(
                "У провайдера «{}» base_url должен начинаться с http:// или https://",
                p.id
            ));
        }
    }
    Ok(())
}

/// Встроенный каталог.
///
/// `expect` здесь оправдан: файл лежит в исходниках крейта, его пишем мы и
/// тестируем ([`tests::embedded_catalog_is_valid`]). Отсутствие файла — ошибка
/// сборки/репозитория, а не runtime-состояние.
pub fn embedded() -> ProviderPresetsCatalog {
    parse(EMBEDDED_CATALOG).expect("встроенный каталог провайдеров должен быть валидным")
}

/// Прочитать кэш, если он есть и валиден. Иначе `None` — вызывающий откатится
/// на встроенный bundle.
///
/// Битый кэш не перетирается: если пользователь открыл его руками, он должен
/// иметь шанс починить. Но и не используется — «битый JSON» не должен ломать шлюз.
pub fn load_cache(data_dir: &Path) -> Option<ProviderPresetsCatalog> {
    let path = presets_cache_path(data_dir);
    let raw = std::fs::read_to_string(&path).ok()?;
    match parse(&raw) {
        Ok(c) => Some(c),
        Err(e) => {
            log::warn!("cloud-routers: {} невалиден ({}), используется встроенный каталог", path.display(), e);
            None
        }
    }
}

/// Каталог для работы приложения + источник, откуда он взят.
pub fn load(data_dir: &Path) -> (ProviderPresetsCatalog, CatalogSource) {
    match load_cache(data_dir) {
        Some(c) => (c, CatalogSource::Cache),
        None => (embedded(), CatalogSource::Embedded),
    }
}

/// Сохранить каталог в кэш (после успешного сетевого sync).
pub fn save_cache(data_dir: &Path, catalog: &ProviderPresetsCatalog) -> Result<(), String> {
    validate(catalog)?;
    let json = serde_json::to_string_pretty(catalog).map_err(|e| e.to_string())?;
    ko_json_store::write_atomic(&presets_cache_path(data_dir), &json)
        .map_err(|e| format!("Не удалось записать кэш каталога: {}", e))
}

/// Найти пресет по id.
pub fn find<'a>(catalog: &'a ProviderPresetsCatalog, id: &str) -> Option<&'a ProviderPreset> {
    catalog.providers.iter().find(|p| p.id == id)
}

/// Свободный id для нового провайдера на основе желаемого.
///
/// Префикс из [preset] уже занят? → `groq-2`. Коллизия возможна, потому что
/// один и тот же сервис пользователь может завести дважды (разные ключи/регионы),
/// а молча перезаписать существующего провайдера нельзя.
pub fn unique_provider_id(existing: &[String], desired: &str) -> String {
    let base = slugify(desired);
    if !existing.iter().any(|e| e == &base) {
        return base;
    }
    for n in 2..1000 {
        let candidate = format!("{base}-{n}");
        if !existing.iter().any(|e| e == &candidate) {
            return candidate;
        }
    }
    format!("{base}-new")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("cr_core_presets_{tag}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("create tmp");
        d
    }

    fn preset(id: &str) -> ProviderPreset {
        ProviderPreset {
            id: id.to_string(),
            name: id.to_string(),
            default_base_url: format!("https://{id}.example.com/v1"),
            ..Default::default()
        }
    }

    fn catalog(ids: &[&str]) -> ProviderPresetsCatalog {
        ProviderPresetsCatalog {
            version: MIN_SUPPORTED_VERSION,
            updated_at: "2026-10-08".into(),
            providers: ids.iter().map(|i| preset(i)).collect(),
        }
    }

    #[test]
    fn embedded_catalog_is_valid() {
        let c = embedded();
        assert!(c.providers.len() >= 20, "встроенный каталог слишком мал: {}", c.providers.len());
        assert!(c.version >= MIN_SUPPORTED_VERSION);
        // Список должен быть полезным, а не формальным.
        for want in ["openai", "groq", "deepseek", "mistral", "openrouter"] {
            assert!(find(&c, want).is_some(), "встроенный каталог без «{}»", want);
        }
    }

    #[test]
    fn embedded_catalog_has_unique_ids_and_sane_urls() {
        let c = embedded();
        let mut ids: Vec<&str> = c.providers.iter().map(|p| p.id.as_str()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "дубликаты id во встроенном каталоге");
        for p in &c.providers {
            assert!(
                p.default_base_url.starts_with("http://") || p.default_base_url.starts_with("https://"),
                "{}: подозрительный base_url «{}»",
                p.id,
                p.default_base_url
            );
        }
    }

    #[test]
    fn provider_id_charset_is_enforced() {
        assert!(is_valid_provider_id("groq"));
        assert!(is_valid_provider_id("samba-nova"));
        assert!(is_valid_provider_id("my_provider_2"));
        assert!(!is_valid_provider_id(""));
        assert!(!is_valid_provider_id("Groq"), "верхний регистр недопустим");
        assert!(!is_valid_provider_id("groq/x"), "слэш ломает разбор комбо");
        assert!(!is_valid_provider_id("-groq"), "не может начинаться с дефиса");
        assert!(!is_valid_provider_id("_groq"));
        assert!(!is_valid_provider_id("gro q"));
        assert!(!is_valid_provider_id(&"a".repeat(65)));
    }

    #[test]
    fn slugify_normalizes_arbitrary_input() {
        assert_eq!(slugify("Groq Cloud"), "groq-cloud");
        assert_eq!(slugify("  SambaNova  "), "sambanova");
        assert_eq!(slugify("meta/llama"), "meta-llama");
        assert_eq!(slugify("!!!"), "provider");
    }

    #[test]
    fn parse_rejects_invalid_json() {
        assert!(parse("nope").is_err());
        assert!(parse(r#"{"version":1,"providers":[]}"#).is_err(), "пустой каталог запрещён");
    }

    #[test]
    fn parse_rejects_catalog_from_the_future() {
        // Каталог из будущего нельзя принять вслепую: поля могли поменяться, и
        // мы бы молча потеряли часть провайдеров.
        let raw = r#"{"version":999,"providers":[{"id":"groq","name":"Groq","default_base_url":"https://a.dev"}]}"#;
        let err = parse(raw).expect_err("must reject future version");
        assert!(err.contains("диапазона"), "{err}");
    }

    #[test]
    fn parse_rejects_version_zero() {
        let raw = r#"{"version":0,"providers":[{"id":"groq","name":"Groq","default_base_url":"https://a.dev"}]}"#;
        assert!(parse(raw).is_err(), "версия 0 должна отвергаться");
    }

    #[test]
    fn current_supported_version_is_accepted() {
        let raw = format!(
            r#"{{"version":{},"providers":[{{"id":"groq","name":"Groq","default_base_url":"https://a.dev"}}]}}"#,
            MAX_SUPPORTED_VERSION
        );
        assert!(parse(&raw).is_ok());
    }

    #[test]
    fn parse_rejects_duplicate_ids() {
        let raw = r#"{"version":1,"providers":[
            {"id":"groq","name":"A","default_base_url":"https://a.dev"},
            {"id":"groq","name":"B","default_base_url":"https://b.dev"}
        ]}"#;
        let err = parse(raw).expect_err("must reject");
        assert!(err.contains("Дубликат"), "{err}");
    }

    #[test]
    fn parse_rejects_bad_base_url() {
        let raw = r#"{"version":1,"providers":[{"id":"groq","name":"A","default_base_url":"api.groq.com"}]}"#;
        assert!(parse(raw).is_err());
    }

    #[test]
    fn parse_accepts_minimal_valid_catalog() {
        let raw = r#"{"version":1,"providers":[{"id":"groq","name":"Groq","default_base_url":"https://api.groq.com/openai/v1"}]}"#;
        let c = parse(raw).expect("parse");
        assert_eq!(c.providers.len(), 1);
        assert_eq!(c.providers[0].name, "Groq");
        assert_eq!(c.providers[0].api_docs_url, None);
    }

    #[test]
    fn cache_takes_priority_over_embedded() {
        let d = tmp_dir("priority");
        let c0 = load(&d);
        assert_eq!(c0.1, CatalogSource::Embedded);

        save_cache(&d, &catalog(&["custom"])).expect("save");
        let c1 = load(&d);
        assert_eq!(c1.1, CatalogSource::Cache);
        assert_eq!(c1.0.providers[0].id, "custom");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn corrupt_cache_falls_back_to_embedded_without_deleting_it() {
        let d = tmp_dir("corrupt");
        std::fs::write(presets_cache_path(&d), "{oops").expect("write");
        let (c, src) = load(&d);
        assert_eq!(src, CatalogSource::Embedded);
        assert!(c.providers.len() >= 20);
        assert!(presets_cache_path(&d).exists(), "битый кэш нельзя молча удалять");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn save_cache_rejects_invalid_catalog() {
        let d = tmp_dir("bad");
        let mut c = catalog(&["groq"]);
        c.providers[0].default_base_url = "nope".into();
        assert!(save_cache(&d, &c).is_err());
        assert!(!presets_cache_path(&d).exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn unique_provider_id_avoids_collisions() {
        let existing = vec!["groq".to_string()];
        assert_eq!(unique_provider_id(&existing, "groq"), "groq-2");
        assert_eq!(unique_provider_id(&existing, "Groq Cloud"), "groq-cloud");
        assert_eq!(unique_provider_id(&[], "Groq"), "groq");
        let existing2 = vec!["groq".to_string(), "groq-2".to_string()];
        assert_eq!(unique_provider_id(&existing2, "groq"), "groq-3");
    }
}