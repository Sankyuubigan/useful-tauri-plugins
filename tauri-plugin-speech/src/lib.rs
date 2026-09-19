use serde::Deserialize;
use std::sync::OnceLock;
use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

/// Глобальная копия конфига плагина — нужна свободным функциям
/// (`default_models_dir`/`default_engine_dir` и т.п.), у которых нет доступа
/// к `AppHandle`/`State`. Конфиг попадает сюда в `setup`.
static CONFIG: OnceLock<Config> = OnceLock::new();

const DEFAULT_CONFIG: Config = Config {
    default_engine_dir: None,
    default_models_dir: None,
};

pub(crate) fn set_config(c: Config) {
    let _ = CONFIG.set(c);
}

/// Текущий конфиг плагина (или пустой по умолчанию до инициализации).
pub(crate) fn config() -> &'static Config {
    CONFIG.get().unwrap_or(&DEFAULT_CONFIG)
}

/// Конфиг плагина. Задаётся в `tauri.conf.json` хоста под ключом `plugins.speech`:
/// ```json
/// "plugins": {
///   "speech": {
///     "default_engine_dir": "D:\\nn\\crispasr",
///     "default_models_dir": "D:\\nn\\tts_models"
///   }
/// }
/// ```
///
/// Поля опциональны. Без них используются exe-относительные пути по умолчанию
/// (`<exe_dir>/crispasr` и `<exe_dir>/tts_models` — как в исходном SpeechLab).
#[derive(Deserialize, Clone, Default)]
pub struct Config {
    /// Оверрайд папки движка CrispASR (по умолчанию `<exe_dir>/crispasr`).
    #[serde(default)]
    pub default_engine_dir: Option<String>,
    /// Оверрайд общей папки моделей TTS (по умолчанию `<exe_dir>/tts_models`).
    #[serde(default)]
    pub default_models_dir: Option<String>,
}

/// Состояние плагина, доступное командам и хосту во время выполнения.
///
/// Публично, чтобы хост (напр. SpeechLab) мог делегировать движковую логику STT
/// в плагин, сохранив собственную оверлей-оркестрацию (hotkey/mic/web-push).
pub struct PluginState {
    pub config: Config,
    pub tts: TtsEngine,
    pub stt: SttEngine,
}

mod audio;
mod clone;
mod commands;
pub mod download;
mod inject;
mod log;
mod process_util;
mod stt;
mod stt_events;
pub mod stt_settings;
mod tts;
pub mod tts_settings;
mod voices;
pub mod ws_client;

pub use stt::SttEngine;
pub use stt::SttStatus;
pub use stt_settings::SttSettings;
pub use tts::TtsEngine;
pub use tts_settings::TtsSettings;

/// Инициализация плагина. Вызывается из хоста: `.plugin(tauri_plugin_speech::init())`.
pub fn init<R: Runtime>() -> TauriPlugin<R, Config> {
    Builder::<R, Config>::new("speech")
        .setup(|app, api| {
            set_config(api.config().clone());
            let state = PluginState {
                config: api.config().clone(),
                tts: TtsEngine::new(),
                stt: SttEngine::new(),
            };
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::tts_speak,
            commands::tts_presets,
            commands::tts_capabilities,
            commands::tts_unload,
            commands::tts_save_mp3,
            commands::tts_download_engine,
            commands::tts_download_model,
            commands::tts_engine_backends,
            commands::tts_list_models,
            commands::tts_list_voices,
            commands::tts_add_voice,
            commands::tts_delete_voice,
            commands::tts_update_voice,
            commands::tts_voice_avatar,
            commands::tts_voice_audio,
            commands::tts_voice_trimmed_audio,
            commands::tts_check_update,
            commands::tts_default_dirs,
            commands::tts_get_settings,
            commands::tts_save_settings,
            commands::stt_get_settings,
            commands::stt_save_settings,
            commands::stt_start,
            commands::stt_stop,
            commands::stt_get_status,
            commands::stt_inject_text,
        ])
        .build()
}