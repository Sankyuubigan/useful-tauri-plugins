/** Результат синтеза: WAV-байты + время генерации (сек). */
export interface TtsSpeakResult {
    wav: number[];
    seconds: number;
}
/** Настройки TTS-движка (сериализуется в tts_settings.json, app-config dir). */
export interface TtsSettings {
    engine_dir?: string;
    models_dir?: string;
    engine_backend?: string;
    preset?: string;
}
/** Пресет TTS-модели из tts_models.json. */
export interface TtsPreset {
    id: string;
    label: string;
    backend: string;
    has_codec: boolean;
    has_voice: boolean;
    voice_type: string;
    builtin_voices: string[];
    supports_instruct: boolean;
    supports_russian: boolean;
    size: string;
}
/** Информация о доступном бинаре движка CrispASR. */
export interface EngineBackendInfo {
    id: string;
    label: string;
    asset_name: string;
    url: string;
    tag: string;
}
/** Статус установки пресета в папке моделей. */
export interface InstalledModel {
    id: string;
    label: string;
    installed: boolean;
    has_model: boolean;
    has_codec: boolean;
    has_voice: boolean;
    voice_type: string;
    size: string;
    supports_russian: boolean;
}
/** Запись хранилища голосов. */
export interface VoiceInfo {
    id: string;
    name: string;
    path: string;
    ref_text: string;
    has_avatar: boolean;
    created_at: string;
}
/** Результат проверки обновлений движка. */
export interface CheckUpdateResult {
    ok: boolean;
    error?: string;
    latest?: string;
    engines?: Array<{
        id: string;
        label: string;
        installed: boolean;
        installed_version: string | null;
        latest_version: string;
        update_available: boolean;
    }>;
}
/** Пути по умолчанию (относительно exe хоста). */
export interface DefaultDirs {
    engine_dir: string;
    models_dir: string;
}
/** Возможности загруженного движка (адаптивно по серверу CrispASR). */
export interface Capabilities {
    language: boolean;
}
/** Настройки STT (stt_settings.json рядом с exe). */
export interface SttSettings {
    hotkey_code?: number;
    hotkey_name?: string;
    engine_exe?: string;
    backend?: string;
    model?: string;
    ws_port?: number;
    stream_step_ms?: number;
    stream_length_ms?: number;
    vad?: boolean;
}
export declare function ttsSpeak(preset: string, voice: string, instruct: string, speed: number, text: string, language: string): Promise<TtsSpeakResult>;
export declare function ttsPresets(): Promise<TtsPreset[]>;
export declare function ttsCapabilities(): Promise<Capabilities>;
export declare function ttsUnload(): Promise<void>;
export declare function ttsSaveWav(path: string, data: number[]): Promise<void>;
export declare function ttsDownloadEngine(backendId: string, dest: string): Promise<string>;
export declare function ttsDownloadModel(preset: string, dest: string): Promise<{
    model: string;
    codec: string;
    voice: string;
}>;
export declare function ttsEngineBackends(): Promise<EngineBackendInfo[]>;
export declare function ttsListModels(modelsDir: string): Promise<InstalledModel[]>;
export declare function ttsListVoices(modelsDir: string): Promise<VoiceInfo[]>;
export declare function ttsAddVoice(args: {
    modelsDir: string;
    name: string;
    srcAudio: string;
    refText: string;
    avatar: string;
    denoise: boolean;
    denoiseStrength: number;
}): Promise<VoiceInfo>;
export declare function ttsDeleteVoice(modelsDir: string, id: string): Promise<void>;
export declare function ttsUpdateVoice(args: {
    modelsDir: string;
    id: string;
    name: string;
    refText: string;
    avatar: string;
    srcAudio: string;
    denoise: boolean;
    denoiseStrength: number;
}): Promise<VoiceInfo>;
export declare function ttsVoiceAvatar(modelsDir: string, id: string): Promise<number[] | null>;
export declare function ttsVoiceAudio(modelsDir: string, id: string): Promise<number[]>;
export declare function ttsVoiceTrimmedAudio(modelsDir: string, id: string, backend: string): Promise<number[]>;
export declare function ttsCheckUpdate(): Promise<CheckUpdateResult>;
export declare function ttsDefaultDirs(): Promise<DefaultDirs>;
export declare function ttsGetSettings(): Promise<TtsSettings>;
export declare function ttsSaveSettings(settings: TtsSettings): Promise<void>;
export declare function sttGetSettings(): Promise<SttSettings>;
export declare function sttSaveSettings(settings: SttSettings): Promise<void>;
export declare function sttStart(): Promise<number>;
export declare function sttStop(): Promise<void>;
export declare function sttGetStatus(): Promise<string>;
export declare function sttInjectText(text: string): Promise<void>;
import './web-components';
