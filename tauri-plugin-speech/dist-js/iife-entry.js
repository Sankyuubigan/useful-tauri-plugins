// Точка входа vanilla-канала плагина (esbuild -> api-iife.js, см. PLUGIN_STANDARD.md §4.4).
// Не содержит голых импортов '@tauri-apps/*' — они заменены алиасами esbuild на shims/,
// которые биндятся на window.__TAURI__ (вшит Tauri при withGlobalTauri: true).
import { sttGetSettings, sttGetStatus, sttInjectText, sttSaveSettings, sttStart, sttStop, ttsAddVoice, ttsCapabilities, ttsCheckUpdate, ttsDefaultDirs, ttsDeleteVoice, ttsDownloadEngine, ttsDownloadModel, ttsEngineBackends, ttsGetSettings, ttsListModels, ttsListVoices, ttsPresets, ttsSaveSettings, ttsSaveWav, ttsSpeak, ttsUnload, ttsUpdateVoice, ttsVoiceAudio, ttsVoiceAvatar, ttsVoiceTrimmedAudio, } from './index';
const g = window;
if ('__TAURI__' in window) {
    const tauri = g.__TAURI__;
    if (tauri && !tauri.speech) {
        Object.defineProperty(tauri, 'speech', {
            configurable: true,
            value: {
                sttGetSettings,
                sttGetStatus,
                sttInjectText,
                sttSaveSettings,
                sttStart,
                sttStop,
                ttsAddVoice,
                ttsCapabilities,
                ttsCheckUpdate,
                ttsDefaultDirs,
                ttsDeleteVoice,
                ttsDownloadEngine,
                ttsDownloadModel,
                ttsEngineBackends,
                ttsGetSettings,
                ttsListModels,
                ttsListVoices,
                ttsPresets,
                ttsSaveSettings,
                ttsSaveWav,
                ttsSpeak,
                ttsUnload,
                ttsUpdateVoice,
                ttsVoiceAudio,
                ttsVoiceAvatar,
                ttsVoiceTrimmedAudio,
            },
        });
    }
}
