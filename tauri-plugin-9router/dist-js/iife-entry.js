// Точка входа vanilla-канала плагина (esbuild -> api-iife.js, см. PLUGIN_STANDARD.md §4.4).
// Не содержит голых импортов '@tauri-apps/*' — они заменены алиасами esbuild на shims/,
// которые биндятся на window.__TAURI__ (вшит Tauri при withGlobalTauri: true).
// Импорт './index' тянет './web-components' — Web Components регистрируются автоматически.
import { chatCompletion, ensureStarted, getCombos, getStatus, installOrUpdate, onChunk, onProgress, openDashboard, setRouterDir, stop, } from './index';
const g = window;
if ('__TAURI__' in window) {
    const tauri = g.__TAURI__;
    if (tauri && !tauri['9router']) {
        Object.defineProperty(tauri, '9router', {
            configurable: true,
            value: {
                chatCompletion,
                ensureStarted,
                getCombos,
                getStatus,
                installOrUpdate,
                onChunk,
                onProgress,
                openDashboard,
                setRouterDir,
                stop,
            },
        });
    }
}
