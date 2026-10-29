// Точка входа vanilla-канала плагина (esbuild -> api-iife.js, см. PLUGIN_STANDARD.md §5.4).
// Не содержит голых импортов '@tauri-apps/*' — они заменены алиасами esbuild на shims/,
// которые биндятся на window.__TAURI__ (вшит Tauri при withGlobalTauri: true).
// Импорт './index' тянет './web-components' — Web Components регистрируются автоматически.
import { checkImageEngineUpdate, downloadImageBundle, downloadRemoteLora, downloadVideoBundle, editImage, estimateImageMemory, generateImage, generateVideo, getImageBundleInfo, getImageEngineStatus, getImageModelsCatalog, getVideoBundleInfo, getVideoBundles, installImageEngine, installImageEngineUpdate, listRemoteLoras, notifyBundleChanged, removeImageBundle, removeImageBundleFromList, removeVideoBundleFiles, removeImageEngine, setImageBundleDir, setVideoBundleDir, setImageEngineDir, setImageEngineVariant, setSelectedImageBundle, setSelectedVideoBundle, validateImageBundleDir, validateVideoBundleDir, } from './index';
const g = window;
if ('__TAURI__' in window) {
    const tauri = g.__TAURI__;
    if (tauri && !tauri['image-engine']) {
        Object.defineProperty(tauri, 'image-engine', {
            configurable: true,
            value: {
                checkImageEngineUpdate,
                downloadImageBundle,
                downloadRemoteLora,
                downloadVideoBundle,
                editImage,
                estimateImageMemory,
                generateImage,
                generateVideo,
                getImageBundleInfo,
                getImageEngineStatus,
                getImageModelsCatalog,
                getVideoBundleInfo,
                getVideoBundles,
                installImageEngine,
                installImageEngineUpdate,
                listRemoteLoras,
                notifyBundleChanged,
                removeImageBundle,
                removeImageBundleFromList,
                removeVideoBundleFiles,
                removeImageEngine,
                setImageBundleDir,
                setVideoBundleDir,
                setImageEngineDir,
                setImageEngineVariant,
                setSelectedImageBundle,
                setSelectedVideoBundle,
                validateImageBundleDir,
                validateVideoBundleDir,
            },
        });
    }
}
