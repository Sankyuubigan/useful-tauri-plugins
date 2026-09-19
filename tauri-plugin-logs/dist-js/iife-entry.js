// Точка входа vanilla-канала плагина (esbuild -> api-iife.js, см. PLUGIN_STANDARD.md §4.4).
// Не содержит голых импортов '@tauri-apps/*' — они заменены алиасами esbuild на shims/,
// которые биндятся на window.__TAURI__ (вшит Tauri при withGlobalTauri: true).
import { getLastLogsPath, initFrontendErrorCapture, logFront, logFrontendEvent, onLogMessage, trackError, } from './index';
const g = window;
if ('__TAURI__' in window) {
    const tauri = g.__TAURI__;
    if (tauri && !tauri.logs) {
        Object.defineProperty(tauri, 'logs', {
            configurable: true,
            value: { getLastLogsPath, logFront, logFrontendEvent, onLogMessage, trackError },
        });
    }
    initFrontendErrorCapture();
}
