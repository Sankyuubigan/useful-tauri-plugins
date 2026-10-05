//! `tauri-plugin-system1` — переиспользуемый рантайм быстрых решений (System-1).
//!
//! # Что это
//!
//! Плагин владеет **моделью и инференсом** и ничего не знает о предметной области.
//! На вход — типизированные вопросы, на выход — вероятности. Решение «верно ли
//! это для элемента e3 при пороге 0.36» принимает хост, потому что критерии и
//! пороги — его данные.
//!
//! # Граница ответственности
//!
//! | В плагине | В хосте |
//! |---|---|
//! | пути к артефактам, загрузка ONNX Runtime | что валидировать и по какому критерию |
//! | жизненный цикл сессии и модели | YAML критериев, пороги элементов |
//! | скачивание модели в `KingOrchData` + sha256 | правило `p_true >= threshold` |
//! | построение последовательности и батча | имя сигнала, маршрутизация графа |
//! | усреднение по перестановкам вариантов | UI поверх панели плагина |
//!
//! # Модель
//!
//! Одна: Laya (`mizchi/laya-multilingual-onnx`, ONNX fp16, 322M). Каталог и
//! контрольные суммы — в [`catalog`]. Ни модель, ни ONNX Runtime не бандлятся
//! в инсталлер: оба скачиваются пользователем в данные хоста
//! (`KingOrchData/system1/`). Основание — `llama_cpp_engine.md:44,70-72` и
//! `crispasr_engine.md:41,56-58`: движок «не бандлится», `bundle.resources`
//! содержит только `agents/`, `mcp_servers/`, каталоги и `*.json`.
//!
//! # Пользовательский путь
//!
//! Настройки → «Модель быстрых решений (System-1 / Laya)» → панель
//! `<system1-panel>`: пофайловая проверка комплекта, кнопка «Скачать всё»,
//! удаление.
//!
//! # Устройство
//!
//! Только CPU. Ort-CUDA требует CUDA 12 + cuDNN на машине пользователя, то есть
//! системную зависимость, которую хост запрещает; DirectML уже откачен после
//! падения `STATUS_DLL_NOT_FOUND`. См. `contract::Device`.

pub mod catalog;
pub mod commands;
pub mod contract;
pub mod inference;
pub mod paths;
pub mod provisioning;
pub mod runtime;
pub mod state;

use std::sync::Arc;

use serde::Deserialize;
use tauri::plugin::{Builder, TauriPlugin};
use tauri::Manager;


/// Ошибка плагина с кодом, пригодным для показа пользователю.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum System1Error {
    /// Модель не скачана.
    ModelMissing { path: String },
    /// ONNX Runtime DLL не найдена.
    RuntimeMissing { tried: Vec<String> },
    /// Модель не загрузилась.
    LoadFailed { reason: String },
    /// Ошибка инференса.
    InferenceFailed { reason: String },
}

impl System1Error {
    pub fn code(&self) -> &'static str {
        match self {
            System1Error::ModelMissing { .. } => "model_missing",
            System1Error::RuntimeMissing { .. } => "runtime_missing",
            System1Error::LoadFailed { .. } => "load_failed",
            System1Error::InferenceFailed { .. } => "inference_failed",
        }
    }

    /// Текст для пользователя. Без «возможно, попробуйте ещё раз» — только
    /// причина и что делать.
    pub fn user_message(&self) -> String {
        match self {
            System1Error::ModelMissing { path } => format!(
                "Модель System-1 не скачана ({}). Скачайте её во вкладке настроек.",
                path
            ),
            System1Error::RuntimeMissing { tried } => format!(
                "Не найден onnxruntime.dll. Проверенные пути: {}",
                tried.join(", ")
            ),
            System1Error::LoadFailed { reason } => {
                format!("Не удалось загрузить модель System-1: {}", reason)
            }
            System1Error::InferenceFailed { reason } => {
                format!("Ошибка инференса System-1: {}", reason)
            }
        }
    }

    /// Собрать ошибку из произвольной строки.
    ///
    /// Классифицирует по содержанию, потому что `ort` отдаёт текст, а не код.
    /// Это эвристика — но лучше, чем терять причину: сообщение пользователю
    /// остаётся точным, а код остаётся полезным для телеметрии.
    pub fn from_message(message: String) -> Self {
        if message.contains("onnxruntime.dll") {
            System1Error::RuntimeMissing {
                tried: vec![message.clone()],
            }
        } else if message.contains("не найдена") || message.contains("не скачана") {
            System1Error::ModelMissing { path: message }
        } else {
            System1Error::InferenceFailed { reason: message }
        }
    }
}

impl std::fmt::Display for System1Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.user_message())
    }
}

/// Конфигурация плагина из `tauri.conf.json` хоста:
///
/// ```json
/// { "plugins": { "system1": { "auto_download": true } } }
/// ```
#[derive(Clone, Debug, Default)]
pub struct Config {
    /// Скачивать модель автоматически при первом обращении.
    ///
    /// Выключено по умолчанию: молча качать 646 МБ без согласия пользователя —
    /// тоже невежливо, а «модель появилась сама» хуже, чем «нажмите кнопку».
    pub auto_download: bool,
}

impl<'de> Deserialize<'de> for Config {
    /// `null` вместо секции — не паника, а значения по умолчанию.
    ///
    /// Наивный `#[derive(Deserialize)]` здесь роняет хост на старте с
    /// `invalid type: null, expected struct Config`, если плагин подключён, а
    /// секция в конфиге не описана (§4.3 PLUGIN_STANDARD).
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize, Default)]
        #[serde(rename_all = "camelCase", default)]
        struct Inner {
            auto_download: bool,
        }

        let inner = Option::<Inner>::deserialize(deserializer)?;
        Ok(inner.map_or_else(Config::default, |inner| Config {
            auto_download: inner.auto_download,
        }))
    }
}

/// Задать корень данных для модели System-1.
///
/// Хост вызывает это на старте, передав `app_config.data_dir`: каталог
/// `KingOrchData` лежит РЯДОМ с папкой программы, а не внутри неё, и без
/// этого модель скачалась бы в несуществующее место. Единственный источник
/// правды — `app_config.json`, второй копии пути не создаётся.
pub fn set_data_dir(path: std::path::PathBuf) {
    paths::set_data_root(path);
}

/// Подключить плагин: `.plugin(tauri_plugin_system1::init())`.
pub fn init<R: tauri::Runtime>() -> TauriPlugin<R, Config> {
    Builder::<R, Config>::new("system1")
        .setup(|app, api| {
            // Флаг автоскачивания — единственный рычаг доставки. Пока он был
            // мёртвым полем (конфиг читался как `_config` и выбрасывался),
            // первое обращение к модели падало с «модель не скачана», хотя
            // комплект из 646 МБ лежал рядом в данных пользователя.
            state::set_auto_download(api.config().auto_download);
            // Именно `shared()`, а не `ModelSlot::new()`: команды и workflow
            // должны видеть ОДНУ модель. Два слота означали бы, что панель
            // показывает «не загружена» при загруженной графе модели.
            app.manage(state::shared());
            log::info!(
                "[system1] плагин инициализирован, каталог моделей: {}",
                paths::models_dir().display()
            );
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::download_all,
            commands::download_model,
            commands::remove_model,
            commands::decide,
        ])
        .build()
}

/// Тип для удобства хостов, которым нужен доступ к состоянию.
pub type SharedModel = Arc<inference::System1Model>;