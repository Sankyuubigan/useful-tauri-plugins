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

/// Вид релея: Vercel Edge Function.
pub const RELAY_KIND_VERCEL: &str = "vercel";
/// Вид релея: Cloudflare Workers.
pub const RELAY_KIND_CLOUDFLARE: &str = "cloudflare";

/// Подключённый релей («слот»).
///
/// После миграции — единственный источник правды про URL и секрет релея;
/// `vercel_relay_url`/`vercel_relay_token` остаются только как место чтения
/// старых конфигов и очищаются при первом сохранении (SSOT - документация
/// миграции см. `GatewayConfig::migrate_relay_slots`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RelaySlot {
    /// Slug-идентификатор ("vercel", "cf-main"). На него ссылается
    /// `active_relay`; `/` в id запрещён тем же запретом, что у провайдеров.
    pub id: String,
    /// Вид - [`RELAY_KIND_VERCEL`] или [`RELAY_KIND_CLOUDFLARE`].
    pub kind: String,
    /// Публичный base релея; при `None` запрос со слотами хранит настройки
    /// деплоя, но сам релей может быть ещё не развёрнут.
    pub url: Option<String>,
    /// Общий секрет воркера: `x-relay-token` одновременно с клиентской
    /// стороны и `RELAY_TOKEN` в окружении воркера.
    pub relay_secret: Option<String>,
    /// Для cloudflare: Account ID деплоя.
    pub cf_account_id: Option<String>,
    /// Для cloudflare: имя Worker-скрипта (из него собирается workers.dev URL).
    pub cf_script_name: Option<String>,
}

impl Default for RelaySlot {
    fn default() -> Self {
        Self {
            id: String::new(),
            kind: RELAY_KIND_VERCEL.to_string(),
            url: None,
            relay_secret: None,
            cf_account_id: None,
            cf_script_name: None,
        }
    }
}

impl RelaySlot {
    /// Секрет в_reader для проверки «настроен» или нет (trim пустоты).
    pub fn usable_secret(&self) -> Option<String> {
        self.relay_secret.clone().filter(|t| !t.trim().is_empty())
    }
}

/// Конфигурация шлюза целиком (`<data_dir>/gateway.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GatewayConfig {    /// Порт HTTP-сервера.
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
    /// Подключённые релеи. После [`GatewayConfig::migrate_relay_slots`] —
    /// единственный источник правды; до него пусто, а смысл живёт в
    /// legacy-полях `vercel_relay_*`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relays: Vec<RelaySlot>,
    /// Слот релея по умолчанию: новые провайдеры наследуют его привязкой
    /// (`add_provider`), а legacy-миграция флага `use_relay` привязывает к нему
    /// старых провайдеров. Каждому провайдеру назначается свой слот — это не
    /// глобальный маршрут.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_relay: Option<String>,
    /// Пользовательские комбо-связки: именованные цепочки участников с
    /// fallback. Порядок в списке = порядок в `/v1/models` и `/api/combos`.
    /// Пусто у всех старых конфигов — поле читается как пустой список.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub combos: Vec<ComboConfig>,
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
            relays: Vec::new(),
            active_relay: None,
            combos: Vec::new(),
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
    /// LEGACY (27.10.2026): булев флаг «через релей» из старых конфигов.
    ///
    /// Мигрируется в [`Self::relay_id`] при загрузке
    /// ([`GatewayConfig::migrate_provider_relay_ids`]) и после миграции
    /// нигде не читается. Поле оставлено, чтобы миграция могла прочитать
    /// флаг из старого `gateway.json` (serde молча выбросил бы его при удалении
    /// из структуры — и провайдеры потеряли бы привязку без предупреждения).
    pub use_relay: bool,
    /// Слот релея, через который ходит этот провайдер: `None` — напрямую,
    /// `Some(id)` — слот с таким id в [`GatewayConfig::relays`].
    ///
    /// Релеи не универсальны (воркеры на разных платформах под разными
    /// провайдерами), поэтому привязка — свойство провайдера, а не один
    /// глобальный активный слот.
    pub relay_id: Option<String>,
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
            relay_id: None,
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
    /// Понятное имя ключа, заданное пользователем при добавлении
    /// (`mihael2`, `work-main`). Опционально: у ключей из старых конфигов и
    /// из пресетов имени нет — читать их без `None` нельзя.
    pub name: Option<String>,
}

impl Default for AccountKey {
    fn default() -> Self {
        Self { id: String::new(), key: String::new(), is_active: true, name: None }
    }
}

/// Конструктор нового ключа с нормализацией полей: секрет без краевых
/// пробелов, пустое/пробельное имя схлопывается в `None`.
///
/// SSOT для всех потоков добавления ключей (модалка провайдера, «Добавить
/// ключи» в дашборде): раньше каждый поток собирал `AccountKey` руками и
/// расползался бы при следующем новом поле.
pub fn account_key(id: String, secret: String, name: Option<String>) -> AccountKey {
    let trimmed_name = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    AccountKey {
        id,
        key: secret.trim().to_string(),
        is_active: true,
        name: trimmed_name,
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

/// Пользовательская комбо-связка: именованная цепочка участников с fallback.
///
/// Имя — идентификатор, по которому клиент выбирает связку (`model=<имя>`).
/// Имя без `/` — обязательный инвариант: разделитель `/` занят форматом
/// per-provider комбо `{provider_id}/{model_id}` (см. [`crate::models::split_combo`]),
/// и коллизия сделала бы оба формата неразличимыми.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ComboConfig {
    /// Имя комбо = id модели для клиентов API. Не пустое, без `/`, уникальное.
    pub name: String,
    /// Участники цепочки в порядке приоритета: запрос идёт первому, при его
    /// отказе — второму и так далее.
    pub members: Vec<ComboMember>,
}

impl Default for ComboConfig {
    fn default() -> Self {
        Self { name: String::new(), members: Vec::new() }
    }
}

/// Участник комбо-связки: конкретная модель конкретного провайдера.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ComboMember {
    /// id провайдера из [`GatewayConfig::providers`].
    pub provider_id: String,
    /// id модели из обнаруженных у этого провайдера.
    pub model_id: String,
    /// Подмножество ключей провайдера для этого участника.
    ///
    /// `None` — использовать все активные ключи провайдера (обычная ротация).
    /// `Some(ids)` — только перечисленные ключи, порядок списка = порядок
    /// попыток. Неактивные ключи из списка при запросе пропускаются:
    /// деактивация (`is_active = false`) сильнее явного перечисления — ключ,
    /// выключенный апстримом (401), не оживёт от упоминания в комбо.
    pub key_ids: Option<Vec<String>>,
}

impl Default for ComboMember {
    fn default() -> Self {
        Self { provider_id: String::new(), model_id: String::new(), key_ids: None }
    }
}

impl ComboConfig {
    /// Проверить одну связку на согласованность с провайдерами конфига.
    /// Возвращает список проблем; пустой список — связка корректна.
    ///
    /// Правило живёт рядом с данными (как [`ProviderConfig::validate`]): и
    /// загрузка конфига, и CRUD-эндпоинт дашборда обязаны проверять связку
    /// одним и тем же кодом, иначе две правды о «валидной связке» разъезжаются.
    pub fn validate_against(&self, providers: &[ProviderConfig]) -> Vec<String> {
        let mut problems = Vec::new();
        if self.name.trim().is_empty() {
            problems.push("комбо-связка с пустым именем".to_string());
        }
        if self.name.contains('/') {
            problems.push(format!(
                "имя комбо «{}» содержит «/» — это разделитель в id комбо формата «провайдер/модель»",
                self.name
            ));
        }
        if self.members.is_empty() {
            problems.push(format!(
                "комбо «{}» без участников — связка обязана иметь хотя бы одного",
                self.name
            ));
        }
        for member in &self.members {
            let Some(p) = providers.iter().find(|p| p.id == member.provider_id) else {
                problems.push(format!(
                    "комбо «{}»: участник ссылается на провайдера «{}», которого нет в конфиге",
                    self.name, member.provider_id
                ));
                continue;
            };
            if !p.models.iter().any(|m| m.id == member.model_id) {
                problems.push(format!(
                    "комбо «{}»: модель «{}» не обнаружена у провайдера «{}» — список моделей провайдера единственный источник правды",
                    self.name, member.model_id, member.provider_id
                ));
            }
            if let Some(ids) = &member.key_ids {
                if ids.is_empty() {
                    problems.push(format!(
                        "комбо «{}»: у участника «{}/{}» пустой список key_ids — либо все активные ключи (None), либо непустой список",
                        self.name, member.provider_id, member.model_id
                    ));
                }
                let mut seen: Vec<&str> = Vec::new();
                for id in ids {
                    if seen.contains(&id.as_str()) {
                        problems.push(format!(
                            "комбо «{}»: у участника «{}/{}» дубликат ключа «{}»",
                            self.name, member.provider_id, member.model_id, id
                        ));
                    }
                    seen.push(id);
                    if !p.keys.iter().any(|k| &k.id == id) {
                        problems.push(format!(
                            "комбо «{}»: у участника «{}/{}» ключ «{}» не существует у провайдера «{}»",
                            self.name, member.provider_id, member.model_id, id, member.provider_id
                        ));
                    }
                }
            }
        }
        problems
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

    /// Слот релея по умолчанию (его наследуют новые провайдеры и legacy-флаг).
    pub fn active_slot(&self) -> Option<&RelaySlot> {
        let id = self.active_relay.as_deref()?;
        self.relays.iter().find(|s| s.id == id)
    }

    /// Vercel-слот (тот, что знает конфиг-совместимая запись через панель).
    pub fn vercel_slot(&self) -> Option<&RelaySlot> {
        self.relays.iter().find(|s| s.kind == RELAY_KIND_VERCEL)
    }

    pub fn vercel_slot_mut(&mut self) -> Option<&mut RelaySlot> {
        self.relays.iter_mut().find(|s| s.kind == RELAY_KIND_VERCEL)
    }

    /// URL релея для маршрутизации запросов: активный слот; в конфигах без
    /// слотов (не мигрировали — не сохраняли после старта) - legacy-поле.
    pub fn relay_url(&self) -> Option<String> {
        self.active_slot()
            .and_then(|s| s.url.clone())
            .or_else(|| self.vercel_relay_url.clone())
    }

    /// Секрет релея дляHeaders `x-relay-token`: активный слот, затем legacy.
    pub fn relay_token(&self) -> Option<String> {
        self.active_slot().and_then(|s| s.usable_secret()).or_else(|| self.vercel_relay_token.clone())
    }

    /// Миграция legacy-полей релея в слоты. Идемпотентно, в памяти (файл
    /// перепишется при следующем сохранении — как миграция `presets_sync_url`).
    ///
    /// Инвариант: если в конфиге есть legacy-URL/секрет, то после миграции
    /// существует и Vercel-слот. Регресс 27.10.2026: условие было «список
    /// слотов пуст», поэтому после удаления единственного Vercel-слота
    /// (остались слоты других видов) legacy-деплой оставался невидим в списке
    /// слотов и не мог быть привязан к провайдерам. Теперь слот ищется по
    /// `kind`, а не по пустоте списка.
    pub fn migrate_relay_slots(&mut self) -> bool {
        if self.vercel_slot().is_some() {
            return false;
        }
        if self.vercel_relay_url.is_none() && self.vercel_relay_token.is_none() {
            return false;
        }
        self.relays.push(RelaySlot {
            id: "vercel".to_string(),
            kind: RELAY_KIND_VERCEL.to_string(),
            url: self.vercel_relay_url.clone(),
            relay_secret: self.vercel_relay_token.clone(),
            cf_account_id: None,
            cf_script_name: None,
        });
        self.active_relay = Some("vercel".to_string());
        self.vercel_relay_url = None;
        self.vercel_relay_token = None;
        true
    }

    /// Миграция legacy-флага `use_relay` провайдеров в их привязку `relay_id`.
    ///
    /// 27.10.2026: релей назначается каждому провайдеру отдельно, а не одному
    /// активному на всех. Провайдер с флагом получает активный слот как свою
    /// привязку — это и был прежний смысл флага. Если активного слота нет,
    /// назначать нечем: флаг снимается, привязка остаётся `None` (провайдер
    /// пойдёт напрямую), а в журнал пишется `warn` — смена поведения видима,
    /// не молчалива. Идемпотентно, в памяти (файл перепишется при следующем
    /// сохранении — как миграция `migrate_relay_slots`).
    pub fn migrate_provider_relay_ids(&mut self) -> bool {
        let active = self.active_relay.clone();
        let mut changed = false;
        for p in &mut self.providers {
            if !p.use_relay {
                continue;
            }
            changed = true;
            p.relay_id = p.relay_id.clone().or_else(|| active.clone());
            p.use_relay = false;
            if p.relay_id.is_none() {
                log::warn!(
                    "cloud-routers: провайдер «{}» был настроен «через релей», но активного релея нет — назначена работа напрямую",
                    p.id
                );
            }
        }
        changed
    }

    /// Базовый URL шлюза для подключения IDE.
    pub fn base_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }

    /// Проверить конфигурацию перед сохранением. Возвращает список проблем;
    /// пустой список — конфигурация корректна.
    ///
    /// Проверяется ровно то, что ломает маршрутизацию: дубликаты id, `/`
    /// внутри id (ломает разбор комбо), пустые `base_url` без схемы,
    /// привязка провайдера к несуществующему слоту релея.
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
        for slot in &self.relays {
            if slot.id.trim().is_empty() {
                problems.push("релей: пустой id слота".to_string());
            }
            if slot.kind != RELAY_KIND_VERCEL && slot.kind != RELAY_KIND_CLOUDFLARE {
                problems.push(format!(
                    "релей «{}»: неизвестный вид «{}» (ожидали vercel или cloudflare)",
                    slot.id, slot.kind
                ));
            }
            if let Some(raw) = slot.url.as_deref().filter(|s| !trim_base_url(s).is_empty()) {
                if relay_base_url(raw).is_none() {
                    problems.push(format!(
                        "релей «{}»: URL «{raw}» не начинается с http:// или https://",
                        slot.id
                    ));
                }
            }
        }
        if let Some(id) = self.active_relay.as_deref() {
            if !self.relays.iter().any(|s| s.id == id) {
                problems.push(format!("активный релей «{id}» не найден среди слотов"));
            }
        }
        for p in &self.providers {
            if let Some(rid) = p.relay_id.as_deref() {
                if !self.relays.iter().any(|s| s.id == rid) {
                    problems.push(format!(
                        "провайдер «{}» привязан к релею «{rid}», которого нет среди слотов",
                        p.id
                    ));
                }
            }
        }
        let mut seen_combos: Vec<&str> = Vec::new();
        for c in &self.combos {
            if !c.name.trim().is_empty() && seen_combos.contains(&c.name.as_str()) {
                problems.push(format!("дубликат имени комбо-связки «{}»", c.name));
            }
            seen_combos.push(&c.name);
            problems.extend(c.validate_against(&self.providers));
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
            relay_id: None,
            keys: vec![AccountKey { id: "k1".into(), key: "sk-1".into(), is_active: true, name: None }],
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
    fn migrate_relay_slots_moves_legacy_into_slot_and_clears_it() {
        // Договор миграции 23.10.2026: слот — единственный источник правды,
        // legacy-поля после переноса пусты, активный слот назначен.
        let mut c = GatewayConfig::default();
        c.vercel_relay_url = Some("https://my-relay.vercel.app".into());
        c.vercel_relay_token = Some("tok".into());
        assert!(c.migrate_relay_slots(), "миграция должна сработать");
        assert_eq!(c.relays.len(), 1);
        let slot = &c.relays[0];
        assert_eq!(slot.id, "vercel");
        assert_eq!(slot.kind, RELAY_KIND_VERCEL);
        assert_eq!(slot.url.as_deref(), Some("https://my-relay.vercel.app"));
        assert_eq!(slot.relay_secret.as_deref(), Some("tok"));
        assert_eq!(c.active_relay.as_deref(), Some("vercel"));
        assert_eq!(c.vercel_relay_url, None);
        assert_eq!(c.vercel_relay_token, None);
        // Идемпотентность: второй вызов ничего не делает.
        assert!(!c.migrate_relay_slots());
        assert_eq!(c.relays.len(), 1);
    }

    #[test]
    fn migrate_relay_slots_is_noop_without_legacy() {
        let mut c = GatewayConfig::default();
        assert!(!c.migrate_relay_slots());
        assert!(c.relays.is_empty());
        assert_eq!(c.active_relay, None);
    }

    /// 27.10.2026 — регресс: слот ищется по `kind`, а не по пустоте списка.
    /// Пользователь удалил Vercel-слот (остались слоты других видов), затем
    /// развёл новый: URL ушёл в legacy-поля, релей «исчезал» из списка слотов
    /// и не привязывался к провайдерам.
    #[test]
    fn migrate_relay_slots_restores_vercel_slot_among_other_slots() {
        let mut c = GatewayConfig::default();
        c.relays.push(RelaySlot {
            id: "cloudflare".into(),
            kind: "cloudflare".into(),
            url: Some("https://cf.workers.dev".into()),
            relay_secret: Some("cf-tok".into()),
            cf_account_id: Some("acc".into()),
            cf_script_name: Some("relay".into()),
        });
        c.vercel_relay_url = Some("https://new-relay.vercel.app".into());
        c.vercel_relay_token = Some("tok".into());

        assert!(c.migrate_relay_slots(), "миграция обязана восстановить слот");
        assert_eq!(c.relays.len(), 2);
        let slot = c.relays.iter().find(|s| s.kind == RELAY_KIND_VERCEL).expect("vercel-слот");
        assert_eq!(slot.id, "vercel");
        assert_eq!(slot.url.as_deref(), Some("https://new-relay.vercel.app"));
        assert_eq!(slot.relay_secret.as_deref(), Some("tok"));
        // Соседний слот другого вида не тронут.
        let cf = c.relays.iter().find(|s| s.id == "cloudflare").expect("cf-слот");
        assert_eq!(cf.url.as_deref(), Some("https://cf.workers.dev"));
        assert_eq!(c.active_relay.as_deref(), Some("vercel"));
        assert_eq!(c.vercel_relay_url, None);
        assert_eq!(c.vercel_relay_token, None);
        // Идемпотентность: второй вызов ничего не делает.
        assert!(!c.migrate_relay_slots());
        assert_eq!(c.relays.len(), 2);
    }

    /// Слот kind=vercel уже есть — миграция ничего не делает, даже если
    /// legacy-поля остались с другого прогона: слот приоритетен (SSOT).
    #[test]
    fn migrate_relay_slots_noop_when_vercel_slot_exists() {
        let mut c = GatewayConfig::default();
        c.relays.push(RelaySlot {
            id: "vercel".into(),
            kind: RELAY_KIND_VERCEL.into(),
            url: Some("https://slot.vercel.app".into()),
            relay_secret: Some("slot-tok".into()),
            cf_account_id: None,
            cf_script_name: None,
        });
        c.vercel_relay_url = Some("https://legacy.vercel.app".into());
        c.vercel_relay_token = Some("legacy-tok".into());
        assert!(!c.migrate_relay_slots());
        let slot = c.vercel_slot().expect("слот");
        assert_eq!(slot.url.as_deref(), Some("https://slot.vercel.app"));
        assert_eq!(slot.relay_secret.as_deref(), Some("slot-tok"));
    }

    // 27.10.2026 — релей назначается на провайдера (`relay_id`), а не одним
    // активным слотом на всех. Legacy-флаг `use_relay` при загрузке превращается
    // в привязку к активному слоту (его прежний смысл) и снимается; после
    // миграции флаг нигде не читается — единственный источник правды `relay_id`.
    #[test]
    fn migrate_provider_relay_ids_moves_flag_to_active_slot() {
        let mut c = GatewayConfig::default();
        c.relays.push(RelaySlot {
            id: "vercel".into(),
            kind: RELAY_KIND_VERCEL.into(),
            url: Some("https://r.vercel.app".into()),
            ..Default::default()
        });
        c.active_relay = Some("vercel".into());
        let mut p = provider("groq", "https://a.dev");
        p.use_relay = true;
        c.providers.push(p);
        assert!(c.migrate_provider_relay_ids(), "миграция должна сработать");
        assert_eq!(c.providers[0].relay_id.as_deref(), Some("vercel"));
        assert!(!c.providers[0].use_relay, "флаг снят");
        // Идемпотентность: второй вызов ничего не делает.
        assert!(!c.migrate_provider_relay_ids());
        assert_eq!(c.providers[0].relay_id.as_deref(), Some("vercel"));
    }

    #[test]
    fn migrate_provider_relay_ids_without_active_relay_clears_flag() {
        // Провайдер был «через релей», но назначить нечего: флаг снимается,
        // привязки нет — провайдер пойдёт напрямую (warn в журнал об этом).
        let mut c = GatewayConfig::default();
        let mut p = provider("groq", "https://a.dev");
        p.use_relay = true;
        c.providers.push(p);
        assert!(c.migrate_provider_relay_ids());
        assert_eq!(c.providers[0].relay_id, None);
        assert!(!c.providers[0].use_relay);
        assert!(!c.migrate_provider_relay_ids());
    }

    #[test]
    fn migrate_provider_relay_ids_preserves_explicit_binding() {
        // Привязка уже задана вручную — миграция не подменяет её активным
        // слотом, только снимает устаревший флаг.
        let mut c = GatewayConfig::default();
        c.active_relay = Some("vercel".into());
        let mut p = provider("groq", "https://a.dev");
        p.use_relay = true;
        p.relay_id = Some("cf-main".into());
        c.providers.push(p);
        assert!(c.migrate_provider_relay_ids());
        assert_eq!(c.providers[0].relay_id.as_deref(), Some("cf-main"));
        assert!(!c.providers[0].use_relay);
    }

    #[test]
    fn validate_catches_dangling_provider_relay_id() {
        // Привязка на удалённый слот — конфигурационная проблема: запросы
        // провайдера упали бы с ошибкой на каждом вызове.
        let mut c = GatewayConfig::default();
        let mut p = provider("groq", "https://a.dev");
        p.relay_id = Some("ghost".into());
        c.providers.push(p);
        let pr = c.validate();
        assert!(pr.iter().any(|m| m.contains("ghost")), "{:?}", pr);
        // Найденный слот снимает проблему.
        c.relays.push(RelaySlot {
            id: "ghost".into(),
            kind: RELAY_KIND_VERCEL.into(),
            url: Some("https://g.vercel.app".into()),
            ..Default::default()
        });
        assert!(c.validate().is_empty(), "{:?}", c.validate());
    }

    #[test]
    fn relay_url_and_token_prefer_active_slot_over_legacy() {
        let mut c = GatewayConfig::default();
        c.vercel_relay_url = Some("https://legacy.vercel.app".into());
        c.vercel_relay_token = Some("legacy-tok".into());
        // До миграции работает legacy.
        assert_eq!(c.relay_url().as_deref(), Some("https://legacy.vercel.app"));
        assert_eq!(c.relay_token().as_deref(), Some("legacy-tok"));
        // После миграции + добавления CF-слота активным становится CF.
        c.migrate_relay_slots();
        c.relays.push(RelaySlot {
            id: "cf-main".into(),
            kind: RELAY_KIND_CLOUDFLARE.into(),
            url: Some("https://cf-relay.workers.dev".into()),
            relay_secret: Some("cf-tok".into()),
            ..Default::default()
        });
        c.active_relay = Some("cf-main".into());
        assert_eq!(c.relay_url().as_deref(), Some("https://cf-relay.workers.dev"));
        assert_eq!(c.relay_token().as_deref(), Some("cf-tok"));
        // После миграции legacy-поле стёрто (SSOT): без активного слота URL нет.
        c.active_relay = None;
        assert_eq!(c.relay_url(), None);
    }

    #[test]
    fn validate_catches_unknown_relay_kind_and_dangling_active() {
        let mut c = GatewayConfig::default();
        c.relays.push(RelaySlot {
            id: "cfx".into(),
            kind: "netlify".into(),
            url: Some("https://r.workers.dev".into()),
            ..Default::default()
        });
        c.active_relay = Some("ghost".into());
        let p = c.validate();
        assert!(p.iter().any(|m| m.contains("неизвестный вид")), "{:?}", p);
        assert!(p.iter().any(|m| m.contains("ghost")), "{:?}", p);
        // Корректный слот не даёт проблем.
        c.relays.clear();
        c.relays.push(RelaySlot {
            id: "cf-main".into(),
            kind: RELAY_KIND_CLOUDFLARE.into(),
            url: Some("https://cf.workers.dev".into()),
            ..Default::default()
        });
        c.active_relay = Some("cf-main".into());
        assert!(c.validate().iter().all(|m| !m.contains("релей")), "{:?}", c.validate());
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

    // ---- Комбо-связки -------------------------------------------------------

    fn member(provider_id: &str, model_id: &str) -> ComboMember {
        ComboMember { provider_id: provider_id.into(), model_id: model_id.into(), key_ids: None }
    }

    fn config_with_provider() -> GatewayConfig {
        let mut c = GatewayConfig::default();
        c.providers.push(provider("groq", "https://a.dev"));
        c
    }

    #[test]
    fn validate_rejects_slash_in_combo_name() {
        // Инвариант «/» занят форматом provider/model: без этого запрета
        // resolve_user_combo и resolve_combo спорили бы об одном id.
        let mut c = config_with_provider();
        c.combos.push(ComboConfig {
            name: "fast/llama".into(),
            members: vec![member("groq", "m")],
        });
        let p = c.validate();
        assert!(p.iter().any(|m| m.contains("разделитель")), "{:?}", p);
    }

    #[test]
    fn validate_rejects_empty_and_duplicate_combo_names() {
        let mut c = config_with_provider();
        c.combos.push(ComboConfig { name: "  ".into(), members: vec![member("groq", "m")] });
        assert!(c.validate().iter().any(|m| m.contains("пустым именем")), "{:?}", c.validate());

        c.combos.clear();
        c.combos.push(ComboConfig { name: "fast".into(), members: vec![member("groq", "m")] });
        c.combos.push(ComboConfig { name: "fast".into(), members: vec![member("groq", "m")] });
        let p = c.validate();
        assert!(p.iter().any(|m| m.contains("дубликат имени комбо")), "{:?}", p);
    }

    #[test]
    fn validate_rejects_combo_without_members() {
        // Отказ «связка пуста» структурно невозможен: конфиг без участников
        // отвергается валидацией ещё на сохранении, а не в прокси.
        let mut c = config_with_provider();
        c.combos.push(ComboConfig { name: "fast".into(), members: Vec::new() });
        let p = c.validate();
        assert!(p.iter().any(|m| m.contains("без участников")), "{:?}", p);
    }

    #[test]
    fn validate_rejects_member_with_unknown_provider_or_model() {
        let mut c = config_with_provider();
        c.combos.push(ComboConfig {
            name: "fast".into(),
            members: vec![member("ghost", "m"), member("groq", "m2")],
        });
        let p = c.validate();
        assert!(p.iter().any(|m| m.contains("ghost") && m.contains("нет в конфиге")), "{:?}", p);
        assert!(
            p.iter().any(|m| m.contains("m2") && m.contains("не обнаружена")),
            "{:?}",
            p
        );
        // Оба участника корректны — проблем от комбо нет.
        c.combos[0].members = vec![member("groq", "m")];
        assert!(c.validate().is_empty(), "{:?}", c.validate());
    }

    #[test]
    fn validate_rejects_bad_key_ids() {
        let mut c = config_with_provider();
        c.combos.push(ComboConfig {
            name: "fast".into(),
            members: vec![ComboMember {
                provider_id: "groq".into(),
                model_id: "m".into(),
                key_ids: Some(vec!["k1".into(), "k1".into(), "ghost".into()]),
            }],
        });
        c.combos.push(ComboConfig {
            name: "empty_keys".into(),
            members: vec![ComboMember {
                provider_id: "groq".into(),
                model_id: "m".into(),
                key_ids: Some(Vec::new()),
            }],
        });
        let p = c.validate();
        assert!(p.iter().any(|m| m.contains("дубликат ключа")), "{:?}", p);
        assert!(p.iter().any(|m| m.contains("ghost") && m.contains("не существует")), "{:?}", p);
        assert!(p.iter().any(|m| m.contains("empty_keys") && m.contains("пустой список key_ids")), "{:?}", p);
    }

    #[test]
    fn valid_combo_passes_validation() {
        let mut c = config_with_provider();
        c.combos.push(ComboConfig {
            name: "fast".into(),
            members: vec![
                member("groq", "m"),
                ComboMember {
                    provider_id: "groq".into(),
                    model_id: "m".into(),
                    key_ids: Some(vec!["k1".into()]),
                },
            ],
        });
        assert!(c.validate().is_empty(), "{:?}", c.validate());
    }

    #[test]
    fn combos_field_roundtrips_and_old_configs_parse_without_it() {
        // Старый конфиг без поля combos читается с пустым списком — нулевая миграция.
        let raw = r#"{"port":20131,"providers":[{"id":"groq","name":"G","base_url":"https://a.dev"}]}"#;
        let cfg: GatewayConfig = serde_json::from_str(raw).expect("parse");
        assert!(cfg.combos.is_empty());

        // Пользовательская связка переживает serialize→deserialize без искажений,
        // включая подмножество ключей.
        let mut cfg2 = config_with_provider();
        cfg2.combos.push(ComboConfig {
            name: "fast".into(),
            members: vec![ComboMember {
                provider_id: "groq".into(),
                model_id: "m".into(),
                key_ids: Some(vec!["k1".into()]),
            }],
        });
        let back: GatewayConfig =
            serde_json::from_value(serde_json::to_value(&cfg2).expect("serialize")).expect("parse back");
        assert_eq!(back.combos, cfg2.combos, "комбо обязано пережить roundtrip");
    }

    #[test]
    fn active_keys_skips_deactivated_and_blank() {
        let mut p = provider("groq", "https://a.dev");
        p.keys.push(AccountKey { id: "k2".into(), key: "sk-2".into(), is_active: false, name: None });
        p.keys.push(AccountKey { id: "k3".into(), key: "   ".into(), is_active: true, name: None });
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

    // 23.10.26 — поле name у ключа: имена задаются при добавлении, но ждать их
    // от старых конфигов нельзя. Поэтому конструктор нормализует пустые имена
    // в None (структурно исключая «имя из пробелов» как отдельное состояние),
    // а старые JSON читаются как name: None — отсутствие имени не ошибка.
    #[test]
    fn account_keys_without_name_parse_and_roundtrip_names_survive() {
        // Старый JSON: у ключа нет name — читается как None.
        let raw = r#"{"port":20131,"providers":[{"id":"g","name":"G","base_url":"https://a.dev","keys":[{"id":"k1","key":"sk-1","is_active":true}]}]}"#;
        let cfg: GatewayConfig = serde_json::from_str(raw).expect("parse");
        assert_eq!(cfg.providers[0].keys[0].name, None, "у старых ключей имени нет");

        // Конструктор обрезает края: пробелы вокруг секрета и имени не живут.
        let named = account_key("k1".into(), "  sk-2  ".into(), Some(" mihael2 ".into()));
        assert_eq!(named.key, "sk-2");
        assert_eq!(named.name.as_deref(), Some("mihael2"));
        let blank = account_key("k2".into(), "sk-3".into(), Some("   ".into()));
        assert_eq!(blank.name, None, "имя из одних пробелов не сохраняется как Some(\"   \")");

        // Имя и секрет переживают serialize→deserialize без искажений.
        let mut p = provider("g", "https://a.dev");
        p.keys.clear();
        p.keys.push(named);
        let values = serde_json::to_value(&p).expect("serialize");
        let back: ProviderConfig = serde_json::from_value(values).expect("parse back");
        assert_eq!(
            back.keys[0].name.as_deref(),
            Some("mihael2"),
            "имя ключа обязано пережить roundtrip"
        );
        assert_eq!(back.keys[0].key, "sk-2");
    }
}