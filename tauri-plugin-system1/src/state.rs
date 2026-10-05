//! Состояние System-1: один загруженный экземпляр модели на процесс.
//!
//! ## Почему модель одна
//!
//! В хосте `LayaValidator::load(...)` вызывался **внутри** обработчика ноды
//! (`nodes.rs`), то есть 646 МБ ONNX и токенизатор перечитывались на каждом
//! прогоне графа. `Mutex<Session>` этого не спасал: сессия и так создавалась
//! заново, так что блокировка защищала единственный экземпляр, который тут же
//! выбрасывался.
//!
//! Здесь модель грузится лениво и живёт до `remove_model` или смены
//! идентификатора. Устраняет сам отказ, а не сокращает его: повторного
//! чтения модели больше не существует как явления.
//!
//! ## Почему слот общий для UI и графа
//!
//! Workflow исполняется глубоко в оркестраторе, где нет `AppHandle`, поэтому
//! состояние Tauri ему недоступно. Вместо того чтобы пробрасывать `AppHandle`
//! через четыре слоя (и трогать все места создания `WorkflowRunner`), плагин
//! отдаёт **один и тот же** слот двумя способами:
//!
//! * `init()` кладёт слот в состояние Tauri — оттуда его берут команды;
//! * [`shared()`] отдаёт тот же слот без `AppHandle` — оттуда его берёт workflow.
//!
//! Это принципиально: если бы это были два слота, панель показывала бы
//! «модель не загружена» в момент, когда граф уже держит модель в памяти.
//! Публичная функция вместо `static mut` — сознательно: единственный
//! глобал, с явным именем и объяснением.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::catalog::{self, ModelEntry};
use crate::inference::System1Model;

/// Разрешено ли автоматически доставлять недостающие артефакты.
///
/// По умолчанию `false`: молча качать 646 МБ без согласия пользователя — тоже
/// невежливо. Флаг ставится один раз в `init()` из `tauri.conf.json`.
static AUTO_DOWNLOAD: AtomicBool = AtomicBool::new(false);

/// Разрешить или запретить автоскачивание артефактов System-1.
///
/// Вызывается из `init()` хоста. После включения первое обращение к модели
/// само доставит недостающее (ONNX Runtime, затем файлы модели), вместо того
/// чтобы падать с «модель не скачана».
pub fn set_auto_download(enabled: bool) {
    AUTO_DOWNLOAD.store(enabled, Ordering::SeqCst);
    log::info!(
        "[system1] автоскачивание артефактов {}",
        if enabled { "включено" } else { "выключено" }
    );
}

/// Текущее значение флага автоскачивания.
pub fn auto_download() -> bool {
    AUTO_DOWNLOAD.load(Ordering::SeqCst)
}

/// Загруженная модель. `None` = «ещё не грузили», это НЕ ошибка.
#[derive(Clone, Default)]
pub struct ModelSlot {
    inner: Arc<Mutex<Option<Arc<System1Model>>>>,
}

impl ModelSlot {
    pub fn new() -> Self {
        Self::default()
    }

    /// Взять модель, загрузив при первом обращении.
    ///
    /// Ошибка загрузки НЕ кэшируется: слот остаётся пустым, и следующий вызов
    /// попробует снова (пользователь мог скачать модель между вызовами).
    /// Сообщение об ошибке логируется на каждой неудаче.
    pub fn get_or_load(&self, model_id: &str) -> Result<Arc<System1Model>, String> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|error| format!("system1: слот модели заблокирован: {}", error))?;

        if let Some(model) = guard.as_ref() {
            if model.model_id() == model_id {
                return Ok(Arc::clone(model));
            }
            // Запрошена другая модель: прежняя больше не актуальна.
            log::info!(
                "[system1] смена модели: {} -> {}",
                model.model_id(),
                model_id
            );
            *guard = None;
        }

        // Доставка артефактов — ДО `System1Model::load`, и это не порядок
        // вежливости, а требование корректности: `runtime::init_environment`
        // кэширует ошибку в `OnceLock` навсегда («первая попытка провалилась —
        // повторять нельзя»). Поэтому вариант «сначала попытаться загрузить,
        // потом скачать, потом повторить» физически нерабочий: вторая попытка
        // получила бы тот же закешированный Err, даже если DLL уже на диске.
        if let Err(error) = crate::provisioning::ensure_artifacts(model_id) {
            return Err(error);
        }

        let loaded = Arc::new(System1Model::load(model_id)?);
        *guard = Some(Arc::clone(&loaded));
        Ok(loaded)
    }

    /// Выгрузить модель из памяти, не удаляя файлы.
    pub fn release(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            if guard.take().is_some() {
                log::info!("[system1] модель выгружена из памяти");
            }
        }
    }

    /// Модель уже в памяти — нужно UI, чтобы не врать «готово» при файлах,
    /// которые на месте, но ещё ни разу не открывались.
    pub fn is_loaded(&self) -> bool {
        self.inner
            .lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false)
    }
}

static SHARED: OnceLock<ModelSlot> = OnceLock::new();

/// Общий слот модели — тот же, что лежит в состоянии Tauri после `init()`.
///
/// Вызывать из слоёв, где нет `AppHandle` (движок workflow).
pub fn shared() -> ModelSlot {
    SHARED.get_or_init(ModelSlot::new).clone()
}

/// Запись каталога по идентификатору, либо внятная ошибка со списком.
pub fn resolve_model(model_id: &str) -> Result<ModelEntry, String> {
    catalog::find(model_id).ok_or_else(|| {
        let known = catalog::catalog()
            .iter()
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "неизвестная модель System-1: {}. Известные: {}",
            model_id, known
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_model_lists_known_ones() {
        let error = resolve_model("нет-такой").expect_err("должно быть ошибкой");
        assert!(
            error.contains("laya-multilingual-onnx"),
            "в ошибке нет известных: {}",
            error
        );
    }

    #[test]
    fn known_model_resolves() {
        let entry = resolve_model("laya-multilingual-onnx").expect("модель есть");
        assert_eq!(entry.repo, "mizchi/laya-multilingual-onnx");
    }

    /// Слот, который отдаёт `shared()`, и слот в состоянии Tauri — один и тот же
    /// объект. Иначе UI и граф разойдутся: панель скажет «не загружена», а
    /// граф будет работать с моделью в памяти.
    #[test]
    fn shared_slot_is_the_managed_slot() {
        let managed = shared();
        assert!(!managed.is_loaded(), "в тесте модель не должна быть загружена");
        // Проверка идентичности: release() на копии виден через shared().
        managed.release();
        assert!(!shared().is_loaded());
    }

    /// Поведение автоскачивания. Тесты меняют глобальный флаг, поэтому
    /// выполняются по одному — иначе они влияли бы друг на друга.
    mod auto_download {
        use super::*;

        static SERIAL: Mutex<()> = Mutex::new(());

        /// Выполнить проверку с заданным флагом и вернуть его как было.
        fn with_flag(value: bool, check: impl FnOnce()) {
            let guard = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
            let previous = auto_download();
            set_auto_download(value);
            check();
            set_auto_download(previous);
            drop(guard);
        }

        #[test]
        fn flag_round_trips() {
            with_flag(true, || {
                assert!(auto_download(), "флаг не включился");
            });
            with_flag(false, || {
                assert!(!auto_download(), "флаг не выключился");
            });
        }

        /// Выключенный флаг = нулевая работа: даже невалидный идентификатор
        /// проходит без ошибки. Это и есть «поведение как раньше»: путь доставки
        /// не трогается, ошибку о нескачанной модели даёт сама загрузка.
        #[test]
        fn disabled_flag_does_not_enter_delivery_path() {
            with_flag(false, || {
                let outcome = crate::provisioning::ensure_artifacts("нет-такой");
                assert!(
                    outcome.is_ok(),
                    "при выключенном флаге доставка не должна вызываться: {:?}",
                    outcome.err()
                );
            });
        }

        /// Включённый флаг действительно входит в путь доставки: неизвестная
        /// модель отвергается на разрешении каталога, до всякой сети.
        #[test]
        fn enabled_flag_enters_delivery_path() {
            with_flag(true, || {
                let error = crate::provisioning::ensure_artifacts("нет-такой")
                    .expect_err("включённый флаг обязан проверять модель");
                assert!(
                    error.contains("неизвестная модель"),
                    "неожиданная ошибка: {}",
                    error
                );
            });
        }
    }
}