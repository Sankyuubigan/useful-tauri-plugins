// Точка входа vanilla-канала плагина (esbuild -> api-iife.js, см. PLUGIN_STANDARD.md §4.4).
// Не содержит голых импортов '@tauri-apps/*' — они заменены алиасами esbuild на shims/,
// которые биндятся на window.__TAURI__ (вшит Tauri при withGlobalTauri: true).
// Импорт './index' тянет './web-components' — Web Components регистрируются автоматически.
import { checkImageEngineUpdate, downloadImageBundle, editImage, estimateImageMemory, generateImage, getImageBundleInfo, getImageEngineStatus, getImageModelsCatalog, installImageEngine, installImageEngineUpdate, notifyBundleChanged, removeImageBundle, removeImageEngine, setImageBundleDir, setImageEngineDir, setImageEngineVariant, validateImageBundleDir, } from './index';
const g = window;
if ('__TAURI__' in window) {
    const tauri = g.__TAURI__;
    if (tauri && !tauri['image-engine']) {
        Object.defineProperty(tauri, 'image-engine', {
            configurable: true,
            value: {
                checkImageEngineUpdate,
                downloadImageBundle,
                editImage,
                estimateImageMemory,
                generateImage,
                getImageBundleInfo,
                getImageEngineStatus,
                getImageModelsCatalog,
                installImageEngine,
                installImageEngineUpdate,
                notifyBundleChanged,
                removeImageBundle,
                removeImageEngine,
                setImageBundleDir,
                setImageEngineDir,
                setImageEngineVariant,
                validateImageBundleDir,
            },
        });
    }
}
