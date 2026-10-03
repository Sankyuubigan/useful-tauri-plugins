// Точка входа vanilla-канала плагина (esbuild -> api-iife.js, см. PLUGIN_STANDARD.md §5.4).
// Не содержит голых импортов '@tauri-apps/*' — они заменены алиасами esbuild на shims/,
// которые биндятся на window.__TAURI__ (вшит Tauri при withGlobalTauri: true).
// Импорт './index' тянет './web-components' — Web Component регистрируется автоматически.
import { decide, downloadModel, getStatus, onDownloadProgress, removeModel } from './index';
const g = window;
if ('__TAURI__' in window) {
    const tauri = g.__TAURI__;
    if (tauri && !tauri['system1']) {
        Object.defineProperty(tauri, 'system1', {
            configurable: true,
            value: {
                decide,
                downloadModel,
                getStatus,
                onDownloadProgress,
                removeModel,
            },
        });
    }
}
