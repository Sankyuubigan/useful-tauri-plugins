import { invoke } from '@tauri-apps/api/core'

/** Результат синтеза: WAV-байты + время генерации (сек). */
export interface TtsSpeakResult {
  wav: number[]
  seconds: number
}

/** Настройки TTS-движка (сериализуется в tts_settings.json, app-config dir). */
export interface TtsSettings {
  engine_dir?: string
  models_dir?: string
  engine_backend?: string
  preset?: string
}

/** Пресет модели (TTS или STT) из speech_models.json. */
export interface TtsPreset {
  id: string
  label: string
  backend: string
  has_codec: boolean
  has_voice: boolean
  voice_type: string
  builtin_voices: string[]
  supports_instruct: boolean
  supports_russian: boolean
  size: string
}

/** Информация о доступном бинаре движка CrispASR. */
export interface EngineBackendInfo {
  id: string
  label: string
  asset_name: string
  url: string
  tag: string
}

/** Статус установки пресета в папке моделей. */
export interface InstalledModel {
  id: string
  label: string
  installed: boolean
  has_model: boolean
  has_codec: boolean
  has_voice: boolean
  voice_type: string
  size: string
  supports_russian: boolean
}

/** Запись хранилища голосов. */
export interface VoiceInfo {
  id: string
  name: string
  path: string
  ref_text: string
  has_avatar: boolean
  created_at: string
}

/** Результат проверки обновлений движка. */
export interface CheckUpdateResult {
  ok: boolean
  error?: string
  latest?: string
  engines?: Array<{
    id: string
    label: string
    installed: boolean
    installed_version: string | null
    latest_version: string
    update_available: boolean
  }>
}

/** Пути по умолчанию (относительно exe хоста). */
export interface DefaultDirs {
  engine_dir: string
  models_dir: string
}

/** Возможности загруженного движка (адаптивно по серверу CrispASR). */
export interface Capabilities {
  language: boolean
}

/** Настройки STT (stt_settings.json рядом с exe). */
export interface SttSettings {
  hotkey_code?: number
  hotkey_name?: string
  engine_exe?: string
  backend?: string
  model?: string
  ws_port?: number
  stream_step_ms?: number
  stream_length_ms?: number
  vad?: boolean
}

// ─── TTS commands ───────────────────────────────────────────────────────

export async function ttsSpeak(
  preset: string,
  voice: string,
  instruct: string,
  speed: number,
  text: string,
  language: string,
): Promise<TtsSpeakResult> {
  return invoke<TtsSpeakResult>('plugin:speech|tts_speak', {
    preset,
    voice,
    instruct,
    speed,
    text,
    language,
  })
}

export async function ttsPresets(): Promise<TtsPreset[]> {
  return invoke<TtsPreset[]>('plugin:speech|tts_presets')
}

export async function ttsCapabilities(): Promise<Capabilities> {
  return invoke<Capabilities>('plugin:speech|tts_capabilities')
}

export async function ttsUnload(): Promise<void> {
  return invoke('plugin:speech|tts_unload')
}

export async function ttsSaveWav(path: string, data: number[]): Promise<void> {
  return invoke('plugin:speech|tts_save_wav', { path, data })
}

export async function ttsDownloadEngine(
  backendId: string,
  dest: string,
): Promise<string> {
  return invoke<string>('plugin:speech|tts_download_engine', {
    backendId,
    dest,
  })
}

export async function ttsDownloadModel(
  preset: string,
  dest: string,
): Promise<{ model: string; codec: string; voice: string }> {
  return invoke<{ model: string; codec: string; voice: string }>(
    'plugin:speech|tts_download_model',
    { preset, dest },
  )
}

export async function ttsEngineBackends(): Promise<EngineBackendInfo[]> {
  return invoke<EngineBackendInfo[]>('plugin:speech|tts_engine_backends')
}

export async function ttsListModels(modelsDir: string): Promise<InstalledModel[]> {
  return invoke<InstalledModel[]>('plugin:speech|tts_list_models', { modelsDir })
}

export async function ttsListVoices(modelsDir: string): Promise<VoiceInfo[]> {
  return invoke<VoiceInfo[]>('plugin:speech|tts_list_voices', { modelsDir })
}

export async function ttsAddVoice(args: {
  modelsDir: string
  name: string
  srcAudio: string
  refText: string
  avatar: string
  denoise: boolean
  denoiseStrength: number
}): Promise<VoiceInfo> {
  return invoke<VoiceInfo>('plugin:speech|tts_add_voice', {
    modelsDir: args.modelsDir,
    name: args.name,
    srcAudio: args.srcAudio,
    refText: args.refText,
    avatar: args.avatar,
    denoise: args.denoise,
    denoiseStrength: args.denoiseStrength,
  })
}

export async function ttsDeleteVoice(
  modelsDir: string,
  id: string,
): Promise<void> {
  return invoke('plugin:speech|tts_delete_voice', { modelsDir, id })
}

export async function ttsUpdateVoice(args: {
  modelsDir: string
  id: string
  name: string
  refText: string
  avatar: string
  srcAudio: string
  denoise: boolean
  denoiseStrength: number
}): Promise<VoiceInfo> {
  return invoke<VoiceInfo>('plugin:speech|tts_update_voice', {
    modelsDir: args.modelsDir,
    id: args.id,
    name: args.name,
    refText: args.refText,
    avatar: args.avatar,
    srcAudio: args.srcAudio,
    denoise: args.denoise,
    denoiseStrength: args.denoiseStrength,
  })
}

export async function ttsVoiceAvatar(
  modelsDir: string,
  id: string,
): Promise<number[] | null> {
  return invoke<number[] | null>('plugin:speech|tts_voice_avatar', {
    modelsDir,
    id,
  })
}

export async function ttsVoiceAudio(
  modelsDir: string,
  id: string,
): Promise<number[]> {
  return invoke<number[]>('plugin:speech|tts_voice_audio', { modelsDir, id })
}

export async function ttsVoiceTrimmedAudio(
  modelsDir: string,
  id: string,
  backend: string,
): Promise<number[]> {
  return invoke<number[]>('plugin:speech|tts_voice_trimmed_audio', {
    modelsDir,
    id,
    backend,
  })
}

export async function ttsCheckUpdate(): Promise<CheckUpdateResult> {
  return invoke<CheckUpdateResult>('plugin:speech|tts_check_update')
}

export async function ttsDefaultDirs(): Promise<DefaultDirs> {
  return invoke<DefaultDirs>('plugin:speech|tts_default_dirs')
}

export async function ttsGetSettings(): Promise<TtsSettings> {
  return invoke<TtsSettings>('plugin:speech|tts_get_settings')
}

export async function ttsSaveSettings(settings: TtsSettings): Promise<void> {
  return invoke('plugin:speech|tts_save_settings', { settings })
}

// ─── STT commands ───────────────────────────────────────────────────────

export async function sttGetSettings(): Promise<SttSettings> {
  return invoke<SttSettings>('plugin:speech|stt_get_settings')
}

export async function sttSaveSettings(settings: SttSettings): Promise<void> {
  return invoke('plugin:speech|stt_save_settings', { settings })
}

export async function sttStart(): Promise<number> {
  return invoke<number>('plugin:speech|stt_start')
}

export async function sttStop(): Promise<void> {
  return invoke('plugin:speech|stt_stop')
}

export async function sttGetStatus(): Promise<string> {
  return invoke<string>('plugin:speech|stt_get_status')
}

export async function sttInjectText(text: string): Promise<void> {
  return invoke('plugin:speech|stt_inject_text', { text })
}

// Регистрируем Web Components при импорте пакета.
import './web-components'