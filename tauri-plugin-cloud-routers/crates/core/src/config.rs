//! Конфигурация шлюза: провайдеры, ключи, обнаруженные модели, релей.
//!
//! Всё в этом модуле — **сериализуемое и персистентное**. Временного состояния
//! (кулдауны, счётчик ротации) здесь нет намеренно: оно живёт в памяти, см. [`crate::keys`].
//!
//! Причина принципиальная. В исходном ТЗ ключ содержал
//! `#[serde(skip)] cooldown_until: Option<Instant>`. Это смешивало два разных
//! жизненных цикла в одну структуру: `Instant` не сериализуется, не сравнивается
//! между процессами и обнуляется при рестарте — поле молча терялось бы при любой
//! перезаписи конфига, а правка конфига из дашборда обнуляла бы кулдауны у
//! живого провайдера. Разделение делает такое невозможным конструктивно.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Порт шлюза по умолчанию.
///
/// Продолжает семейство портов плагина (`9router` 20128, `extremerouter` 20129,
/// `omniroute` 20130), чтобы все шлюзы плагина были видны в диспетчере задач и
/// `netstat` рядом. Значение 28888 из исходного ТЗ не использовано: оно ничего
/// не кодирует и для пользователя выглядит как случайное число.
pub const DEFAULT_PORT: u16 = 20131;

/// Адрес по умолчанию. Только loopback: шлюз держит API-ключи провайдеров и
/// не должен быть доступен из сети.
pub const DEFAULT_HOST: &str = "127.0.0.1";

/// Конфигурация шлюза целиком (`<data_dir>/gateway.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GatewayConfig {
    /// Порт HTTP-сервера.
    pub port: u16,
    /// Интерфейс bind. По умолчанию только loopback.
    pub host: String,
    /// Ленивый автозапуск: поднимать шлюз по требованию.
    pub auto_start: bool,
    /// URL Vercel-релея, если оно развёрнуто.
    pub vercel_relay_url: Option<String>,
    /// Секрет релея. Заголовок `x-relay-token`.
    ///
    /// Генерируется автоматически при первом развёртывании релея, чтобы
    /// пользователю не пришлось ничего вводить. Без него развёрнутый Edge-воркер
    /// был бы публичным прокси «в никуда» для всех, кто узнает URL.
    pub vercel_relay_token: Option<String>,
    /// Провайдеры (облачные LLM-сервисы).
    pub providers: Vec<ProviderConfig>,
    /// Сетевые источники каталога пресетов провайдеров, по убыванию приоритета.
    ///
    /// Пусто → [`crate::presets::sources::DEFAULT_SOURCES`]. Поле — список, а не
    /// один URL: один URL — одна точка отказа (переезд файла = мёртвый каталог,
    /// доказано 2026-10). Сломанный/перенесённый источник чинится правкой
    /// конфига, а не выпуском новой версии.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presets_sources: Vec<String>,
    /// Поля, которых эта версия крейта не знает.
    ///
    /// Сохраняются и переписываются при следующем сохранении. Это не
    /// косметика: **шлюз переживает перезапуск приложения**, поэтому во время
    /// обновления старый и новый экземпляр пишут в один и тот же файл. Без
    /// этого поля старая версия молча вытирала бы настройки новой — и потеря
    /// была бы необратимой (пользователь увидит «пустой список провайдеров»).
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            host: DEFAULT_HOST.to_string(),
            auto_start: true,
            vercel_relay_url: None,
            vercel_relay_token: None,
            providers: Vec::new(),
            presets_sources: Vec::new(),
            extra: BTreeMap::new(),
        }
    }
}

/// Один облачный провайдер с набором ключей и списком обнаруженных моделей.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    /// Идентификатор провайдера. Только `[a-z0-9_-]`, **без `/`** —
    /// это разделитель в id комбо, см. [`crate::models`].
    pub id: String,
    /// Человекочитаемое имя для UI.
    pub name: String,
    /// Базовый URL OpenAI-совместимого API, без завершающего `/`.
    ///
    /// Для не-OpenAI протоколов (`protocol != openai`) поле — подсказка/запасной
    /// адрес, а не истина: реальные endpoints строит адаптер протокола.
    pub base_url: String,
    /// Протокол апстрима. Дефолт — OpenAI (нулевая миграция старых конфигов).
    pub protocol: crate::protocol::VendorProtocol,
    /// Способ авторизации: ключи (`ApiKeys`, текущее поведение) или OAuth-сессия.
    pub auth: crate::protocol::ProviderAuth,
    /// Участвует ли провайдер в маршрутизации.
    pub is_enabled: bool,
    /// Пускать ли запросы этого провайдера через Vercel-релей.
    pub use_relay: bool,
    /// API-ключи с ротацией.
    pub keys: Vec<AccountKey>,
    /// Модели, обнаруженные реальным `GET /models` (не захардкожены).
    pub models: Vec<DiscoveredModel>,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            base_url: String::new(),
            protocol: crate::protocol::VendorProtocol::default(),
            auth: crate::protocol::ProviderAuth::default(),
            is_enabled: true,
            use_relay: false,
            keys: Vec::new(),
            models: Vec::new(),
        }
    }
}

impl ProviderConfig {
    /// Проверить одного провайдера. Возвращает список проблем; пустой список —
    /// провайдер корректен.
    ///
    /// Вынесено из `GatewayConfig::validate`, чтобы правило жило рядом с
    /// данными, а не размазывалось по вызывающему: иначе добавление поля
    /// (как `protocol`) требовало бы синхронных правок в двух местах.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.id.trim().is_empty() {
            problems.push("провайдер с пустым id".to_string());
            return problems;
        }
        if self.id.contains('/') {
            problems.push(format!(
                "id провайдера «{}» содержит «/» — это разделитель в id комбо",
                self.id
            ));
        }
        // Для не-OpenAI протоколов base_url — подсказка, а не истина:
        // endpoints строит адаптер. Пустой допустим, мусорная схема — нет.
        let needs_base = self.protocol == crate::protocol::VendorProtocol::Openai;
        if self.base_url.trim().is_empty() {
            if needs_base {
                problems.push(format!("у провайдера «{}» пустой base_url", self.id));
            }
        } else if !(self.base_url.starts_with("http://") || self.base_url.starts_with("https://")) {
            problems.push(format!(
                "base_url провайдера «{}» должен начинаться с http:// или https://",
                self.id
            ));
        }
        if let crate::protocol::ProviderAuth::OAuth { session_id } = &self.auth {
            if session_id.trim().is_empty() {
                problems.push(format!("у провайдера «{}» OAuth без session_id", self.id));
            }
        }
        problems
    }

    /// Ключи, готовые к использованию (включённые). Кулдаун учитывается
    /// вызывающим кодом через [`crate::keys::KeyRotation`].
    pub fn active_keys(&self) -> impl Iterator<Item = &AccountKey> {
        self.keys.iter().filter(|k| k.is_active && !k.key.trim().is_empty())
    }

    /// `true`, если провайдер можно использовать прямо сейчас: включён, есть
    /// живой ключ и хотя бы одна обнаруженная модель.
    ///
    /// Для OAuth-провайдеров наличие сессии проверяет шлюз (у него файл сессий),
    /// а не конфиг: `core` не знает, существует ли сессия на диске. Поэтому
    /// здесь OAuth считается «авторизация возможна», а точную проверку делает
    /// прокси перед запросом. Ложного «не маршрутизируется» для настроенного
    /// OAuth быть не должно — иначе пользователь видит мёртвого провайдера.
    pub fn is_routable(&self) -> bool {
        let has_auth = match &self.auth {
            crate::protocol::ProviderAuth::ApiKeys => self.active_keys().next().is_some(),
            crate::protocol::ProviderAuth::OAuth { .. } => true,
        };
        self.is_enabled && has_auth && !self.models.is_empty()
    }
}

/// API-ключ провайдера.
///
/// `is_active = false` — это персистентная деактивация «мёртвого» ключа
/// (провайдер ответил 401/403). Она переживает перезапуск, в отличие от
/// кулдауна: ключ с неверным токеном не станет валидным сам по себе.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AccountKey {
    /// Идентификатор ключа внутри провайдера. Генерируется при добавлении.
    pub id: String,
    /// Секрет (`gsk_...`, `sk-...`).
    pub key: String,
    /// Ручное/автоматическое отключение.
    pub is_active: bool,
}

impl Default for AccountKey {
    fn default() -> Self {
        Self { id: String::new(), key: String::new(), is_active: true }
    }
}

/// Модель, обнаруженная реальным запросом `GET {base_url}/models`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiscoveredModel {
    /// Идентификатор модели так, как его знает апстрим.
    pub id: String,
    /// Тип объекта из ответа провайдера (обычно `"model"`).
    pub object: Option<String>,
    /// Unix-время создания, если провайдер его отдаёт.
    pub created: Option<u64>,
}

impl Default for DiscoveredModel {
    fn default() -> Self {
        Self { id: String::new(), object: None, created: None }
    }
}

/// Убрать завершающие `/`, чтобы `"{base}/chat/completions"` не давал `//`.
pub fn trim_base_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

/// База релея, пригодная для склейки URL запроса.
///
/// Возвращает `Some(trimmed)`, если значение настроено и начинается с
/// `http://`/`https://` (после обрезки завершающих `/`), иначе `None`.
///
/// Почему проверка схемы здесь и единственная в кодовой базе — реальный кейс
/// 22.10.2026: в конфиге оказался `vercel_relay_url` **без** схемы
/// (`my-relay.vercel.app`). Значение считалось «настроенным», релей
/// подключался, и склейка `{base}{path}` давала scheme-less URL — reqwest
/// отвечал `Kind::Builder` («builder error») на **каждый** запрос через релей:
/// sync каталога, модели, Kiro. Пользователь видел бессмысленное «неверный URL
/// запроса». Единое место превращает негодную базу в `None`, а потребители
/// обязаны дать понятную причину вместо мёртвого «builder error».
pub fn relay_base_url(raw: &str) -> Option<String> {
    let base = trim_base_url(raw);
    if base.is_empty() {
        return None;
    }
    if base.starts_with("http://") || base.starts_with("https://") {
        Some(base)
    } else {
        None
    }
}

impl GatewayConfig {
    /// Источники каталога с учётом пользовательского списка.
    ///
    /// Пустой `presets_sources` → дефолты из [`crate::presets::sources::DEFAULT_SOURCES`].
    /// Непустой → пользовательский список целиком (пользователь взял управление
    /// на себя; дефолты НЕ подмешиваются — иначе «убрать источник» было бы
    /// невозможно).
    pub fn effective_presets_sources(&self) -> Vec<String> {
        if self.presets_sources.is_empty() {
            crate::presets::sources::DEFAULT_SOURCES.iter().map(|s| s.to_string()).collect()
        } else {
            self.presets_sources.clone()
        }
    }

    /// Найти провайдера по id.
    pub fn provider(&self, id: &str) -> Option<&ProviderConfig> {
        self.providers.iter().find(|p| p.id == id)
    }

    /// Найти провайдера по id (для мутации).
    pub fn provider_mut(&mut self, id: &str) -> Option<&mut ProviderConfig> {
        self.providers.iter_mut().find(|p| p.id == id)
    }

    /// Провайдеры, участвующие в маршрутизации.
    pub fn routable_providers(&self) -> impl Iterator<Item = &ProviderConfig> {
        self.providers.iter().filter(|p| p.is_routable())
    }

    /// Базовый URL шлюза для подключения IDE.
    pub fn base_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }

    /// Проверить конфигурацию перед сохранением. Возвращает список проблем;
    /// пустой список — конфигурация корректна.
    ///
    /// Проверяется ровно то, что ломает маршрутизацию: дубликаты id, `/`
    /// внутри id (ломает разбор комбо), пустые `base_url` без схемы.
    /// Наличие ключей и моделей **не** проверяется — это нормальное состояние
    /// только что созданного провайдера, а не ошибка конфигурации.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.host.trim().is_empty() {
            problems.push("host не может быть пустым".to_string());
        }
        if self.port == 0 {
            problems.push("port должен быть больше нуля".to_string());
        }
        let mut seen: Vec<&str> = Vec::new();
        for p in &self.providers {
            if seen.contains(&p.id.as_str()) {
                problems.push(format!("дубликат id провайдера «{}»", p.id));
            }
            seen.push(&p.id);
            problems.extend(p.validate());
        }
        if let Some(raw) = self.vercel_relay_url.as_deref().filter(|s| !trim_base_url(s).is_empty()) {
            if relay_base_url(raw).is_none() {
                problems.push(format!(
                    "vercel_relay_url «{raw}» не начинается с http:// или https:// — релей обязан быть абсолютным URL"
                ));
            }
        }
        problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(id: &str, base: &str) -> ProviderConfig {
        ProviderConfig {
            id: id.to_string(),
            name: id.to_string(),
            base_url: base.to_string(),
            is_enabled: true,
            use_relay: false,
            keys: vec![AccountKey { id: "k1".into(), key: "sk-1".into(), is_active: true }],
            models: vec![DiscoveredModel { id: "m".into(), object: None, created: None }],
            ..Default::default()
        }
    }

    #[test]
    fn defaults_are_loopback_and_plugin_port_family() {
        let c = GatewayConfig::default();
        assert_eq!(c.port, 20131);
        assert_eq!(c.host, "127.0.0.1");
        assert!(c.auto_start);
        assert_eq!(c.base_url(), "http://127.0.0.1:20131");
    }

    #[test]
    fn config_roundtrip_preserves_unknown_future_fields() {
        // Гарантия совместимости: добавление полей в новой версии шлюза не должно
        // стирать их при сохранении конфига старой версией.
        let raw = r#"{"port":9999,"something_new":{"a":1},"providers":[]}"#;
        let cfg: GatewayConfig = serde_json::from_str(raw).expect("parse");
        assert_eq!(cfg.port, 9999);
        let back = serde_json::to_value(&cfg).expect("serialize");
        assert_eq!(back.get("something_new").and_then(|v| v.get("a")), Some(&serde_json::json!(1)));
    }

    #[test]
    fn trim_base_url_removes_trailing_slashes() {
        assert_eq!(trim_base_url("https://api.groq.com/openai/v1/"), "https://api.groq.com/openai/v1");
        assert_eq!(trim_base_url("  https://x.dev///  "), "https://x.dev");
    }

    #[test]
    fn relay_base_url_accepts_only_absolute_addresses() {
        // Реальный кейс 22.10.2026: конфиг содержал релей без схемы — и sync
        // каталога падал «builder error» на каждом источнике. Единственная
        // точка проверки схемы: негодная база отдаёт None, а не URL-мусор.
        assert_eq!(
            relay_base_url("https://my-relay.vercel.app"),
            Some("https://my-relay.vercel.app".to_string())
        );
        assert_eq!(relay_base_url("http://127.0.0.1:8787///"), Some("http://127.0.0.1:8787".to_string()));
        assert_eq!(relay_base_url("my-relay.vercel.app"), None);
        assert_eq!(relay_base_url("   "), None);
        assert_eq!(relay_base_url(""), None);
    }

    #[test]
    fn validate_catches_relay_url_without_scheme() {
        let mut c = GatewayConfig::default();
        c.vercel_relay_url = Some("my-relay.vercel.app".into());
        let p = c.validate();
        assert!(
            p.iter().any(|m| m.contains("vercel_relay_url") && m.contains("https://")),
            "{:?}",
            p
        );
        // С корректным релеем проблем от этого поля нет.
        c.vercel_relay_url = Some("https://my-relay.vercel.app".into());
        assert!(c.validate().iter().all(|m| !m.contains("vercel_relay_url")));
    }

    #[test]
    fn validate_catches_slash_in_provider_id() {
        let mut c = GatewayConfig::default();
        c.providers.push(provider("bad/id", "https://x.dev"));
        let p = c.validate();
        assert!(p.iter().any(|m| m.contains("разделитель")), "{:?}", p);
    }

    #[test]
    fn validate_catches_duplicate_ids() {
        let mut c = GatewayConfig::default();
        c.providers.push(provider("groq", "https://a.dev"));
        c.providers.push(provider("groq", "https://b.dev"));
        let p = c.validate();
        assert!(p.iter().any(|m| m.contains("дубликат")), "{:?}", p);
    }

    #[test]
    fn validate_catches_bad_base_url_scheme() {
        let mut c = GatewayConfig::default();
        c.providers.push(provider("groq", "api.groq.com"));
        let p = c.validate();
        assert!(p.iter().any(|m| m.contains("http://")), "{:?}", p);
    }

    #[test]
    fn validate_allows_fresh_provider_without_keys_or_models() {
        // Провайдер с валидным base_url, но без ключей и моделей — норма, а не
        // ошибка конфигурации: ошибкой это станет только в момент попытки
        // маршрутизации, и там сработает `is_routable()`.
        let mut c = GatewayConfig::default();
        c.providers.push(ProviderConfig {
            id: "groq".into(),
            base_url: "https://api.groq.com/openai/v1".into(),
            ..Default::default()
        });
        assert!(c.validate().is_empty(), "{:?}", c.validate());
    }

    #[test]
    fn validate_rejects_provider_with_empty_base_url() {
        let mut c = GatewayConfig::default();
        c.providers.push(ProviderConfig { id: "groq".into(), ..Default::default() });
        assert!(c.validate().iter().any(|m| m.contains("base_url")), "{:?}", c.validate());
    }

    #[test]
    fn active_keys_skips_deactivated_and_blank() {
        let mut p = provider("groq", "https://a.dev");
        p.keys.push(AccountKey { id: "k2".into(), key: "sk-2".into(), is_active: false });
        p.keys.push(AccountKey { id: "k3".into(), key: "   ".into(), is_active: true });
        let ids: Vec<&str> = p.active_keys().map(|k| k.id.as_str()).collect();
        assert_eq!(ids, vec!["k1"]);
    }

    #[test]
    fn is_routable_requires_enabled_key_and_models() {
        let mut p = provider("groq", "https://a.dev");
        assert!(p.is_routable());
        p.is_enabled = false;
        assert!(!p.is_routable());
        p.is_enabled = true;
        p.models.clear();
        assert!(!p.is_routable());
        p.models.push(DiscoveredModel { id: "m".into(), ..Default::default() });
        p.keys[0].is_active = false;
        assert!(!p.is_routable());
    }

    #[test]
    fn oauth_provider_is_routable_without_keys_but_models_required() {
        let mut p = provider("kiro", "https://q.us-east-1.amazonaws.com");
        p.keys.clear();
        p.auth = crate::protocol::ProviderAuth::OAuth { session_id: "s1".into() };
        assert!(p.is_routable(), "настроенный OAuth не должен выглядеть мёртвым");
        p.models.clear();
        assert!(!p.is_routable(), "без моделей маршрутизировать нечего даже по OAuth");
    }

    #[test]
    fn kiro_provider_may_have_empty_base_url() {
        let mut p = provider("kiro", "");
        p.protocol = crate::protocol::VendorProtocol::Kiro;
        p.keys.clear();
        p.auth = crate::protocol::ProviderAuth::OAuth { session_id: "s1".into() };
        assert!(p.validate().is_empty(), "{:?}", p.validate());
    }

    #[test]
    fn validate_rejects_oauth_without_session_id() {
        let mut p = provider("kiro", "");
        p.protocol = crate::protocol::VendorProtocol::Kiro;
        p.auth = crate::protocol::ProviderAuth::OAuth { session_id: "  ".into() };
        assert!(p.validate().iter().any(|m| m.contains("session_id")), "{:?}", p.validate());
    }

    #[test]
    fn old_configs_without_new_fields_parse_with_defaults() {
        // Нулевая миграция: старые gateway.json без protocol/auth читаются как OpenAI+ApiKeys.
        let raw = r#"{"port":20131,"providers":[{"id":"groq","name":"Groq","base_url":"https://a.dev"}]}"#;
        let cfg: GatewayConfig = serde_json::from_str(raw).expect("parse");
        assert_eq!(cfg.providers[0].protocol, crate::protocol::VendorProtocol::Openai);
        assert_eq!(cfg.providers[0].auth, crate::protocol::ProviderAuth::ApiKeys);
    }
}