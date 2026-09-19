// Точка входа vanilla-канала плагина (esbuild -> api-iife.js, см. PLUGIN_STANDARD.md §4.4).
// Не содержит голых импортов '@tauri-apps/*' — они заменены алиасами esbuild на shims/,
// которые биндятся на window.__TAURI__ (вшит Tauri при withGlobalTauri: true).
// Импорт './index' тянет './web-components' — Web Components регистрируются автоматически.
import { addModel, autoDownloadDefaultModel, checkEngineUpdate, deleteModelFile, downloadModel, ensureMmproj, estimatePromptMemory, getAllCapabilities, getAutoDownloadInfo, getEngineConfig, getEngineStatus, getMmprojPath, getModelCapabilities, getModelParams, getModelsCatalog, installEngineUpdate, installLlamaCpp, notifyModelsChanged, removeEngine, removeModel, resetModelParams, setEngineDir, setEngineVariant, setModelParams, } from './index';
const g = window;
if ('__TAURI__' in window) {
    const tauri = g.__TAURI__;
    if (tauri && !tauri['llama-engine']) {
        Object.defineProperty(tauri, 'llama-engine', {
            configurable: true,
            value: {
                addModel,
                autoDownloadDefaultModel,
                checkEngineUpdate,
                deleteModelFile,
                downloadModel,
                ensureMmproj,
                estimatePromptMemory,
                getAllCapabilities,
                getAutoDownloadInfo,
                getEngineConfig,
                getEngineStatus,
                getMmprojPath,
                getModelCapabilities,
                getModelParams,
                getModelsCatalog,
                installEngineUpdate,
                installLlamaCpp,
                notifyModelsChanged,
                removeEngine,
                removeModel,
                resetModelParams,
                setEngineDir,
                setEngineVariant,
                setModelParams,
            },
        });
    }
}
