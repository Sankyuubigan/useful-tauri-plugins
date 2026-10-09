//! Состояние шлюза: конфигурация, ротация ключей, HTTP-клиент, журнал событий.
//!
//! ## Почему `AppState` оборачивает всё в `Arc`
//!
//! axum требует `S: Clone + Send + Sync` для типа состояния — он клонирует его
//! в каждый воркер. Клонировать сами поля (`RwLock`, `Mutex`, `reqwest::Client`)
//! нельзя, поэтому `AppState` тонкая обёртка над `Arc<Inner>`. Альтернатива —
//! передавать `Arc<AppState>` в роутер, но тогда во всех 20+ обработчиках
//! пришлось бы писать `State<Arc<AppState>>` и размазывать по коду знание о
//! разделяемости. Обёртка дешевле и оставляет подпись обработчиков
//! независимой от того, как состояние хранится.
//!
//! ## Конфигурация: память + write-through на диск
//!
//! В памяти лежит **копия**, которая обслуживает прокси (чтобы не читать файл на
//! каждый запрос), на диске — источник правды. Запись всегда идёт в двух шагах:
//! сначала валидация и атомарная запись файла, и только потом обновление памяти.
//! Обратный порядок приводил бы к расхождению: неудачный `rename` оставил бы
//! прокси работать с конфигом, которого на диске нет, и пользователь увидел бы
//! «изменения не сохранились», а следующий перезапуск всё стёр бы.
//!
//! Все изменения проходят через один [`AppState::update_config`], который же
//! держит блокировку записи. Отдельная ручная блокировка не нужна и была бы
//! второй точкой правды.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use cloud_routers_core::config::GatewayConfig;
use cloud_routers_core::keys::KeyRotation;
use cloud_routers_core::store as core_store;

use crate::logbuf::EventLog;

/// Таймаут установки TCP-соединения с апстримом.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Таймаут **между** чтениями ответа апстрима.
///
/// Именно «между», а не на весь запрос: ответ стримится минутами, и общий
/// таймаут убил бы поток на первой же длинной генерации. Значение означает
/// «если апстрим замолчал дольше 180 секунд — считаем его мёртвым».
pub const READ_TIMEOUT: Duration = Duration::from_secs(180);

/// Внутреннее состояние. `AppState` — клоновая обёртка над `Arc` на него.
struct Inner {
    data_dir: PathBuf,
    config: RwLock<GatewayConfig>,
    /// Сериализует read-modify-write конфигурации.
    write_lock: Mutex<()>,
    rotation: KeyRotation,
    http: reqwest::Client,
    events: EventLog,
    started_at: Instant,
}

/// Состояние приложения, разделяемое всеми обработчиками.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<Inner>,
}

impl AppState {
    /// Создать состояние и загрузить конфигурацию с диска.
    pub fn new(data_dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(data_dir)
            .map_err(|e| format!("Не удалось создать каталог данных {}: {}", data_dir.display(), e))?;
        let config = core_store::load(data_dir);
        let problems = config.validate();
        if !problems.is_empty() {
            // Не падаем: пользователь должен иметь chance починить конфиг в UI.
            // Но говорим прямо, что часть маршрутизации может не работать.
            log::warn!(
                "cloud-routers-gateway: {} содержит проблемы: {}",
                core_store::config_path(data_dir).display(),
                problems.join("; ")
            );
        }
        let http = build_http_client()?;
        Ok(Self {
            inner: Arc::new(Inner {
                data_dir: data_dir.to_path_buf(),
                config: RwLock::new(config),
                write_lock: Mutex::new(()),
                rotation: KeyRotation::new(),
                http,
                events: EventLog::new(),
                started_at: Instant::now(),
            }),
        })
    }

    pub fn data_dir(&self) -> &Path {
        &self.inner.data_dir
    }

    pub fn uptime(&self) -> Duration {
        self.inner.started_at.elapsed()
    }

    pub fn rotation(&self) -> &KeyRotation {
        &self.inner.rotation
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.inner.http
    }

    pub fn events(&self) -> &EventLog {
        &self.inner.events
    }

    /// Снимок конфигурации.
    pub fn config(&self) -> GatewayConfig {
        self.inner
            .config
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Изменить конфигурацию: записать на диск, затем обновить память.
    ///
    /// `mutate` получает копию. Ошибка валидации или записи оставляет и файл, и
    /// память в прежнем состоянии.
    ///
    /// Блокировки здесь стандартные (не асинхронные), хотя вызывается из
    /// async-кода: критическая секция — это чтение/запись файла в несколько
    /// килобайт. Держать `std::sync::Mutex` через `.await` нельзя, но мы и не
    /// держим — внутри `mutate` и `save` точек ожидания нет.
    pub fn update_config<T>(
        &self,
        mutate: impl FnOnce(&mut GatewayConfig) -> Result<T, String>,
    ) -> Result<T, String> {
        let _guard = self
            .inner
            .write_lock
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let mut draft = self.config();
        let result = mutate(&mut draft)?;
        core_store::save(&self.inner.data_dir, &draft)?;
        *self.inner.config.write().unwrap_or_else(|p| p.into_inner()) = draft;
        Ok(result)
    }

    /// Перечитать конфигурацию с диска (после ручной правки файла).
    ///
    /// Кулдауны ключей при этом **не** сбрасываются: перечитывание файла — это
    /// не перезапуск, и молча «оживлять» ключи значило бы стереть состояние,
    /// которого пользователь не просил.
    pub fn reload(&self) -> Result<(), String> {
        let _guard = self
            .inner
            .write_lock
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let fresh = core_store::load(&self.inner.data_dir);
        *self.inner.config.write().unwrap_or_else(|p| p.into_inner()) = fresh;
        Ok(())
    }
}

/// HTTP-клиент к апстримам.
///
/// **Здесь нет общего таймаута запроса.** Это не упущение: `Client::timeout`
/// ограничивает суммарную длительность запроса, включая чтение тела, и
/// гарантированно обрывает SSE-поток на первой длинной генерации. Вместо него —
/// таймаут соединения и таймаут между чтениями. Инвариант проверяется
/// поведением в интеграционных тестах (`long_stream_survives_without_total_timeout`).
fn build_http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .pool_idle_timeout(Duration::from_secs(90))
        .tcp_keepalive(Duration::from_secs(30))
        .user_agent(format!("cloud-routers-gateway/{}", cloud_routers_core::VERSION))
        .build()
        .map_err(|e| format!("Не удалось создать HTTP-клиент: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloud_routers_core::config::{AccountKey, DiscoveredModel, ProviderConfig};

    fn tmp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cr_gw_app_{tag}"));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn state_with_provider(tag: &str) -> (AppState, PathBuf) {
        let d = tmp_dir(tag);
        let mut cfg = GatewayConfig::default();
        cfg.providers.push(ProviderConfig {
            id: "groq".into(),
            name: "Groq".into(),
            base_url: "https://api.groq.com/openai/v1".into(),
            keys: vec![AccountKey { id: "k1".into(), key: "gsk-1".into(), is_active: true }],
            models: vec![DiscoveredModel { id: "m1".into(), ..Default::default() }],
            ..Default::default()
        });
        std::fs::create_dir_all(&d).expect("mkdir");
        core_store::save(&d, &cfg).expect("save");
        let st = AppState::new(&d).expect("state");
        (st, d)
    }

    #[test]
    fn state_is_clone_because_axum_requires_it() {
        // axum клонирует состояние в каждый воркер: без Clone роутер не
        // собирается. Тест фиксирует это требование явно.
        let d = tmp_dir("clone");
        let st = AppState::new(&d).expect("state");
        let cloned = st.clone();
        assert_eq!(cloned.config().port, st.config().port);
        // Клоны разделяют состояние: запись через один видна через другой.
        cloned.update_config(|c| {
            c.port = 20999;
            Ok(())
        })
        .expect("update");
        assert_eq!(st.config().port, 20999, "клоны обязаны делить одну конфигурацию");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn new_creates_data_dir_and_loads_config() {
        let d = tmp_dir("new");
        assert!(!d.exists());
        let st = AppState::new(&d).expect("state");
        assert!(d.exists(), "каталог данных должен быть создан");
        assert_eq!(st.config().port, cloud_routers_core::config::DEFAULT_PORT);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn config_reflects_file_on_start() {
        let (st, d) = state_with_provider("load");
        assert_eq!(st.config().providers.len(), 1);
        assert_eq!(st.config().providers[0].keys[0].key, "gsk-1");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn update_config_persists_and_updates_memory() {
        let (st, d) = state_with_provider("update");
        st.update_config(|c| {
            c.port = 20999;
            Ok(())
        })
        .expect("update");
        assert_eq!(st.config().port, 20999);
        assert_eq!(core_store::load(&d).port, 20999, "изменение должно попасть на диск");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn failed_update_leaves_memory_and_disk_untouched() {
        let (st, d) = state_with_provider("fail");
        let before = st.config();
        let err = st
            .update_config(|c| {
                c.providers.push(ProviderConfig {
                    id: "bad/id".into(),
                    base_url: "https://x.dev".into(),
                    ..Default::default()
                });
                Ok(())
            })
            .expect_err("must reject");
        assert!(err.contains("разделитель"), "{err}");
        assert_eq!(st.config(), before, "память не должна меняться при ошибке");
        assert_eq!(core_store::load(&d), before, "диск не должен меняться при ошибке");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn update_error_propagates_without_persisting() {
        let (st, d) = state_with_provider("err");
        let err = st
            .update_config(|_c| Err::<(), String>("сбой бизнес-логики".into()))
            .expect_err("must propagate");
        assert!(err.contains("бизнес-логики"), "{err}");
        assert_eq!(core_store::load(&d).providers.len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn reload_picks_up_manual_file_edits() {
        let (st, d) = state_with_provider("reload");
        let mut on_disk = core_store::load(&d);
        on_disk.port = 20555;
        core_store::save(&d, &on_disk).expect("save");
        assert_eq!(st.config().port, 20131, "до reload память не меняется");
        st.reload().expect("reload");
        assert_eq!(st.config().port, 20555);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn reload_keeps_key_cooldowns() {
        // Перечитывание файла — не перезапуск. Сбрасывать кулдауны молча
        // значило бы стирать состояние, которого пользователь не просил.
        let (st, _d) = state_with_provider("reloadcooldown");
        st.rotation().mark_failure(
            "groq",
            "k1",
            cloud_routers_core::keys::FailureKind::RateLimit,
            Some(Duration::from_secs(120)),
        );
        assert!(!st.rotation().is_available("groq", "k1"));
        st.reload().expect("reload");
        assert!(!st.rotation().is_available("groq", "k1"), "reload не должен оживлять ключи");
    }

    #[test]
    fn timeout_constants_have_sane_ratio() {
        // Таймаут соединения меньше таймаута чтения: иначе «долго отвечает» и
        // «недоступен» обрабатывались бы одинаково и неразличимо в журнале.
        assert!(CONNECT_TIMEOUT < READ_TIMEOUT);
        // Общего таймаута нет by design (см. build_http_client), поэтому любой
        // таймаут должен быть достаточно большим для потоков.
        assert!(READ_TIMEOUT >= Duration::from_secs(60));
    }
}