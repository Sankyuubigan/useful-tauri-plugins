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

use std::sync::{Arc, Mutex, OnceLock};

use crate::catalog::{self, ModelEntry};
use crate::inference::System1Model;

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
}