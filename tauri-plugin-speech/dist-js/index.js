import { invoke } from '@tauri-apps/api/core';
// ─── TTS commands ───────────────────────────────────────────────────────
export async function ttsSpeak(preset, voice, instruct, speed, text, language) {
    return invoke('plugin:speech|tts_speak', {
        preset,
        voice,
        instruct,
        speed,
        text,
        language,
    });
}
export async function ttsPresets() {
    return invoke('plugin:speech|tts_presets');
}
export async function ttsCapabilities() {
    return invoke('plugin:speech|tts_capabilities');
}
export async function ttsUnload() {
    return invoke('plugin:speech|tts_unload');
}
export async function ttsSaveWav(path, data) {
    return invoke('plugin:speech|tts_save_wav', { path, data });
}
export async function ttsDownloadEngine(backendId, dest) {
    return invoke('plugin:speech|tts_download_engine', {
        backend_id: backendId,
        dest,
    });
}
export async function ttsDownloadModel(preset, dest) {
    return invoke('plugin:speech|tts_download_model', { preset, dest });
}
export async function ttsEngineBackends() {
    return invoke('plugin:speech|tts_engine_backends');
}
export async function ttsListModels(modelsDir) {
    return invoke('plugin:speech|tts_list_models', { models_dir: modelsDir });
}
export async function ttsListVoices(modelsDir) {
    return invoke('plugin:speech|tts_list_voices', { models_dir: modelsDir });
}
export async function ttsAddVoice(args) {
    return invoke('plugin:speech|tts_add_voice', {
        models_dir: args.modelsDir,
        name: args.name,
        src_audio: args.srcAudio,
        ref_text: args.refText,
        avatar: args.avatar,
        denoise: args.denoise,
        denoise_strength: args.denoiseStrength,
    });
}
export async function ttsDeleteVoice(modelsDir, id) {
    return invoke('plugin:speech|tts_delete_voice', { models_dir: modelsDir, id });
}
export async function ttsUpdateVoice(args) {
    return invoke('plugin:speech|tts_update_voice', {
        models_dir: args.modelsDir,
        id: args.id,
        name: args.name,
        ref_text: args.refText,
        avatar: args.avatar,
        src_audio: args.srcAudio,
        denoise: args.denoise,
        denoise_strength: args.denoiseStrength,
    });
}
export async function ttsVoiceAvatar(modelsDir, id) {
    return invoke('plugin:speech|tts_voice_avatar', {
        models_dir: modelsDir,
        id,
    });
}
export async function ttsVoiceAudio(modelsDir, id) {
    return invoke('plugin:speech|tts_voice_audio', { models_dir: modelsDir, id });
}
export async function ttsVoiceTrimmedAudio(modelsDir, id, backend) {
    return invoke('plugin:speech|tts_voice_trimmed_audio', {
        models_dir: modelsDir,
        id,
        backend,
    });
}
export async function ttsCheckUpdate() {
    return invoke('plugin:speech|tts_check_update');
}
export async function ttsDefaultDirs() {
    return invoke('plugin:speech|tts_default_dirs');
}
export async function ttsGetSettings() {
    return invoke('plugin:speech|tts_get_settings');
}
export async function ttsSaveSettings(settings) {
    return invoke('plugin:speech|tts_save_settings', { settings });
}
// ─── STT commands ───────────────────────────────────────────────────────
export async function sttGetSettings() {
    return invoke('plugin:speech|stt_get_settings');
}
export async function sttSaveSettings(settings) {
    return invoke('plugin:speech|stt_save_settings', { settings });
}
export async function sttStart() {
    return invoke('plugin:speech|stt_start');
}
export async function sttStop() {
    return invoke('plugin:speech|stt_stop');
}
export async function sttGetStatus() {
    return invoke('plugin:speech|stt_get_status');
}
export async function sttInjectText(text) {
    return invoke('plugin:speech|stt_inject_text', { text });
}
// Регистрируем Web Components при импорте пакета.
import './web-components';
