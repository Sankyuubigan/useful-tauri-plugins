use std::time::Instant;

use serde_json::{json, Value};

use crate::clone;
use crate::download;
use crate::log;
use crate::stt_settings::SttSettings;
use crate::tts;
use crate::tts_settings::{
    load as load_tts_settings, save as save_tts_settings, TtsSettings,
};
use crate::voices;
use crate::PluginState;

use tauri::{AppHandle, Manager, Runtime};

/// Результат синтеза: WAV-байты + время генерации (сек) для отображения в UI.
#[derive(serde::Serialize)]
pub struct TtsSpeakResult {
    wav: Vec<u8>,
    seconds: f64,
}

/// Длительность WAV в секундах по заголовку (приблизительно, игнорируем вложенные
/// чанки). Нужна для оценки скорости синтеза (RTF) в логах.
fn wav_duration_secs(wav: &[u8]) -> Option<f64> {
    if wav.len() < 44 {
        return None;
    }
    let sr = u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]) as f64;
    let ch = u16::from_le_bytes([wav[22], wav[23]]) as f64;
    let bits = u16::from_le_bytes([wav[34], wav[35]]) as f64;
    if sr <= 0.0 || ch <= 0.0 || bits <= 0.0 {
        return None;
    }
    let data_bytes = (wav.len() - 44) as f64;
    Some(data_bytes / (sr * ch * (bits / 8.0)))
}

#[tauri::command]
pub async fn tts_speak<R: Runtime>(
    app: AppHandle<R>,
    preset: String,
    voice: String,
    instruct: String,
    speed: f32,
    text: String,
    language: String,
) -> Result<TtsSpeakResult, String> {
    let start = Instant::now();
    let state = app.state::<PluginState>();
    let settings = load_tts_settings(&app);
    let backend_id = if settings.engine_backend.is_empty() {
        "cpu".to_string()
    } else {
        settings.engine_backend.clone()
    };
    let engine_exe = download::resolve_engine_exe(&settings.engine_dir, &backend_id);
    let engine_exe_str = engine_exe.to_string_lossy().to_string();
    let backend = download::preset_backend(&preset)
        .unwrap_or("qwen3-tts")
        .to_string();

    let preset_def = download::preset_by_id(&preset);
    let voice_type = preset_def
        .map(|p| p.voice_type.clone())
        .unwrap_or_else(|| "none".to_string());
    let supports_instruct = preset_def.map(|p| p.supports_instruct).unwrap_or(false);

    let backend_clone = clone::clone_reference_limits(&backend).is_some();
    let mut clone_ref_text = String::new();

    // Резолвим `voice` в одну из ситуаций (без копирования файлов в хранилище).
    let voice_input = voice.trim();
    let looks_path = !voice_input.is_empty()
        && (voice_input.contains('/')
            || voice_input.contains('\\')
            || std::path::Path::new(voice_input).is_absolute()
            || voice_input.to_ascii_lowercase().ends_with(".wav")
            || voice_input.to_ascii_lowercase().ends_with(".gguf"));

    let mut clone_source: Option<(String, String)> = None;
    let mut named_voice = String::new();

    if looks_path {
        if !std::path::Path::new(voice_input).exists() {
            let e = format!("файл голоса не найден: {voice_input}");
            log::app_log(&app, &format!("ТТС ОШИБКА: {e}"));
            return Err(e);
        }
        clone_source = Some((
            voice_input.to_string(),
            tts::ascii_voice_name(voice_input),
        ));
    } else if !voice_input.is_empty() {
        let wav = voices::voices_root(&settings.models_dir)
            .join(&voice_input)
            .join("voice.wav");
        if wav.exists() {
            if backend_clone {
                clone_source = Some((wav.to_string_lossy().to_string(), voice_input.to_string()));
            } else {
                named_voice = voice_input.to_string();
            }
        } else {
            named_voice = voice_input.to_string();
        }
    }

    let needs_ref_text = clone::backend_needs_ref_text(&backend);
    let is_stored_clone = clone_source
        .as_ref()
        .map(|(_, id)| {
            voices::voices_root(&settings.models_dir)
                .join(id)
                .join("voice.wav")
                .exists()
        })
        .unwrap_or(false);
    let use_clone = if needs_ref_text {
        false
    } else {
        backend_clone && clone_source.is_some()
    };

    let startup_voice = if use_clone {
        let (src, id) = clone_source.clone().unwrap();
        let cr = clone::prepare_clone_reference(
            &settings.models_dir,
            &src,
            &id,
            &backend,
        )
        .map_err(|e| {
            log::app_log(&app, &format!("ТТС ОШИБКА: {e}"));
            e
        })?;
        clone_ref_text = cr.ref_text;
        cr.voice_path
    } else if (voice_type == "ggupack" || voice_type == "clone" || voice_type == "clone_named")
        && !needs_ref_text
    {
        let base = if settings.models_dir.is_empty() {
            download::default_models_dir()
        } else {
            std::path::PathBuf::from(&settings.models_dir)
        };
        if let Some(p) = preset_def {
            if let Some((vf, _)) = &p.voice {
                let vp = base.join(&preset).join(vf);
                if vp.exists() {
                    vp.to_string_lossy().to_string()
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // Сохраняем текущие настройки TTS, чтобы они пережили перезапуск приложения.
    let mut s = settings;
    s.preset = preset.clone();
    s.engine_backend = backend_id.clone();
    let _ = save_tts_settings(&app, &s);

    log::app_log(&app, "ТТС: подготовка движка CrispASR...");
    state
        .tts
        .ensure(&app, &engine_exe_str, &backend, &s.models_dir, &preset, &startup_voice)
        .await
        .map_err(|e| {
            log::app_log(&app, &format!("ТТС ОШИБКА: {e}"));
            e
        })?;

    // Голос для тела запроса — только для named (ggupack/WAV-clone уже загружены).
    let body_voice = if voice_type == "ggupack" {
        named_voice.clone()
    } else if needs_ref_text && is_stored_clone {
        clone_source
            .as_ref()
            .map(|(_, id)| id.clone())
            .unwrap_or_default()
    } else if !use_clone && !named_voice.is_empty() {
        named_voice.clone()
    } else {
        String::new()
    };
    let body_instruct = if supports_instruct && !instruct.is_empty() {
        instruct.clone()
    } else {
        String::new()
    };

    let (server_voice, voice_uploaded) = if voice_type == "ggupack" || body_voice.is_empty() {
        (String::new(), false)
    } else {
        state
            .tts
            .ensure_voice(&app, &s.models_dir, &body_voice)
            .await
            .map_err(|e| {
                log::app_log(&app, &format!("ТТС ОШИБКА: {e}"));
                e
            })?
    };

    let clone = use_clone || voice_uploaded;

    let language = if language.trim().is_empty()
        && tts::backend_supports_language_param(&backend)
        && preset_def.map(|p| p.supports_russian).unwrap_or(false)
    {
        "ru".to_string()
    } else {
        language.clone()
    };

    log::app_log(&app, "ТТС: синтез речи...");
    let wav = state
        .tts
        .speak(&text, &server_voice, &body_instruct, &clone_ref_text, speed, clone, &language)
        .await
        .map_err(|e| {
            log::app_log(&app, &format!("ТТС ОШИБКА: {e}"));
            e
        })?;

    let secs = start.elapsed().as_secs_f64();
    let rtf = wav_duration_secs(&wav).map(|d| d / secs);
    let mut msg = format!(
        "ТТС: синтез завершён за {:.2} с ({} байт WAV)",
        secs,
        wav.len()
    );
    if let Some(r) = rtf {
        msg.push_str(&format!(", скорость {:.2}x реального времени", r));
    }
    log::app_log(&app, &msg);
    Ok(TtsSpeakResult {
        wav,
        seconds: secs,
    })
}

#[tauri::command]
pub async fn tts_presets() -> Result<Vec<Value>, String> {
    Ok(download::list_presets())
}

/// Возвращает актуальные возможности загруженного движка TTS (адаптивно
/// определённые по самому серверу CrispASR, без хардкода по пресетам).
#[tauri::command]
pub fn tts_capabilities<R: Runtime>(app: AppHandle<R>) -> Result<Value, String> {
    let state = app.state::<PluginState>();
    let language = state.tts.supports_language();
    Ok(json!({ "language": language }))
}

#[tauri::command]
pub async fn tts_unload<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let state = app.state::<PluginState>();
    log::app_log(&app, "ТТС: выгрузка движка (освобождение VRAM)...");
    state.tts.stop().await;
    Ok(())
}

/// Сохраняет синтезированный WAV (байты из фронта) по выбранному пользователем пути.
#[tauri::command]
pub async fn tts_save_wav(path: String, data: Vec<u8>) -> Result<(), String> {
    std::fs::write(&path, &data)
        .map_err(|e| format!("не удалось сохранить WAV в {path}: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn tts_download_engine<R: Runtime>(
    app: AppHandle<R>,
    backend_id: String,
    dest: String,
) -> Result<String, String> {
    log::app_log(&app, "ТТС: получаю список бинарей CrispASR...");
    let backends = download::engine_backends().await.map_err(|e| {
        log::app_log(&app, &format!("ТТС ОШИБКА: {e}"));
        e
    })?;
    let b = backends
        .iter()
        .find(|x| x.id == backend_id)
        .ok_or_else(|| format!("неизвестный бэкенд движка: {backend_id}"))?;
    log::app_log(
        &app,
        &format!("ТТС: скачивание движка CrispASR ({}) в {}...", b.label, dest),
    );
    let path = download::download_engine(&app, &dest, &backend_id, &b.url, &b.tag)
        .await
        .map_err(|e| {
            log::app_log(&app, &format!("ТТС ОШИБКА загрузки движка: {e}"));
            e
        })?;
    let mut s = load_tts_settings(&app);
    s.engine_dir = dest.clone();
    s.engine_backend = backend_id.clone();
    let _ = save_tts_settings(&app, &s);
    log::app_log(&app, &format!("ТТС: движок скачан: {path}"));
    Ok(path)
}

#[tauri::command]
pub async fn tts_download_model<R: Runtime>(
    app: AppHandle<R>,
    preset: String,
    dest: String,
) -> Result<Value, String> {
    log::app_log(&app, &format!("ТТС: скачивание GGUF модели ({preset}) в {dest}..."));
    let res = download::download_model(&app, &preset, &dest)
        .await
        .map_err(|e| {
            log::app_log(&app, &format!("ТТС ОШИБКА загрузки модели: {e}"));
            e
        })?;
    let mut s = load_tts_settings(&app);
    s.models_dir = dest.clone();
    s.preset = preset.clone();
    let _ = save_tts_settings(&app, &s);
    log::app_log(&app, "ТТС: модель скачана");
    Ok(res)
}

#[tauri::command]
pub async fn tts_engine_backends() -> Result<Vec<download::EngineBackendInfo>, String> {
    download::engine_backends().await
}

#[tauri::command]
pub async fn tts_list_models(models_dir: String) -> Vec<Value> {
    download::list_installed_models(&models_dir)
}

#[tauri::command]
pub async fn tts_list_voices(models_dir: String) -> Vec<voices::VoiceInfo> {
    voices::list_voices(&models_dir)
}

#[tauri::command]
pub async fn tts_add_voice<R: Runtime>(
    app: AppHandle<R>,
    models_dir: String,
    name: String,
    src_audio: String,
    ref_text: String,
    avatar: String,
    denoise: bool,
    denoise_strength: f32,
) -> Result<voices::VoiceInfo, String> {
    voices::add_voice(
        &app,
        &models_dir,
        &name,
        &src_audio,
        &ref_text,
        &avatar,
        denoise,
        denoise_strength,
    )
}

#[tauri::command]
pub async fn tts_delete_voice(models_dir: String, id: String) -> Result<(), String> {
    voices::delete_voice(&models_dir, &id)
}

#[tauri::command]
pub async fn tts_update_voice<R: Runtime>(
    app: AppHandle<R>,
    models_dir: String,
    id: String,
    name: String,
    ref_text: String,
    avatar: String,
    src_audio: String,
    denoise: bool,
    denoise_strength: f32,
) -> Result<voices::VoiceInfo, String> {
    voices::update_voice(
        &app,
        &models_dir,
        &id,
        &name,
        &ref_text,
        &avatar,
        &src_audio,
        denoise,
        denoise_strength,
    )
}

#[tauri::command]
pub async fn tts_voice_avatar(models_dir: String, id: String) -> Option<Vec<u8>> {
    voices::voice_avatar(&models_dir, &id)
}

#[tauri::command]
pub async fn tts_voice_audio(models_dir: String, id: String) -> Result<Vec<u8>, String> {
    voices::voice_audio(&models_dir, &id)
}

#[tauri::command]
pub async fn tts_voice_trimmed_audio(
    models_dir: String,
    id: String,
    backend: String,
) -> Result<Vec<u8>, String> {
    clone::voice_trimmed_audio(&models_dir, &id, &backend)
}

#[tauri::command]
pub async fn tts_check_update<R: Runtime>(app: AppHandle<R>) -> Value {
    let settings = load_tts_settings(&app);
    match download::engine_backends().await {
        Ok(backends) => {
            let tag = backends.first().map(|b| b.tag.clone()).unwrap_or_default();
            let engines: Vec<Value> = backends
                .iter()
                .map(|b| {
                    let installed =
                        download::resolve_engine_exe(&settings.engine_dir, &b.id).exists();
                    let iv = download::installed_engine_version(&settings.engine_dir, &b.id);
                    let update_available = iv.as_ref().map(|v| v != &tag).unwrap_or(false);
                    json!({
                        "id": b.id,
                        "label": b.label,
                        "installed": installed,
                        "installed_version": iv,
                        "latest_version": tag,
                        "update_available": update_available,
                    })
                })
                .collect();
            json!({ "ok": true, "latest": tag, "engines": engines })
        }
        Err(e) => json!({ "ok": false, "error": e }),
    }
}

#[tauri::command]
pub async fn tts_default_dirs() -> Value {
    json!({
        "engine_dir": download::default_engine_dir().to_string_lossy(),
        "models_dir": download::default_models_dir().to_string_lossy(),
    })
}

#[tauri::command]
pub fn tts_get_settings<R: Runtime>(app: AppHandle<R>) -> TtsSettings {
    load_tts_settings(&app)
}

#[tauri::command]
pub fn tts_save_settings<R: Runtime>(
    app: AppHandle<R>,
    settings: TtsSettings,
) -> Result<(), String> {
    save_tts_settings(&app, &settings)
}

// ─── STT: engine-level commands (без оверлея hotkey/mic) ─────────────

#[tauri::command]
pub async fn stt_get_settings() -> SttSettings {
    SttSettings::load()
}

#[tauri::command]
pub async fn stt_save_settings<R: Runtime>(
    app: AppHandle<R>,
    settings: SttSettings,
) -> Result<(), String> {
    settings.save()?;
    log::app_log(
        &app,
        &format!(
            "[stt] горячая клавиша сохранена: {} (code {})",
            settings.hotkey_name, settings.hotkey_code
        ),
    );
    Ok(())
}

#[tauri::command]
pub async fn stt_start<R: Runtime>(app: AppHandle<R>) -> Result<u16, String> {
    let state = app.state::<PluginState>();
    let settings = SttSettings::load();
    let ws_port = state.stt.ensure(&app, &settings).await?;
    Ok(ws_port)
}

#[tauri::command]
pub async fn stt_stop<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let state = app.state::<PluginState>();
    state.stt.stop(&app).await;
    Ok(())
}

#[tauri::command]
pub async fn stt_get_status<R: Runtime>(app: AppHandle<R>) -> Result<String, String> {
    let state = app.state::<PluginState>();
    Ok(match state.stt.status() {
        crate::stt::SttStatus::Stopped => "stopped",
        crate::stt::SttStatus::Starting => "starting",
        crate::stt::SttStatus::Listening => "listening",
        crate::stt::SttStatus::Recording => "recording",
        crate::stt::SttStatus::Transcribing => "transcribing",
        crate::stt::SttStatus::Error(_) => "error",
    }
    .to_string())
}

#[tauri::command]
pub async fn stt_inject_text(text: String) -> Result<(), String> {
    crate::inject::inject_text(&text)
}