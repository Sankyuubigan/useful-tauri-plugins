// Точка входа vanilla-канала плагина (esbuild -> api-iife.js).
import { cancelDownload, downloadBytes, downloadFile, listActive, onProgress, } from './index';
const g = window;
if ('__TAURI__' in window) {
    const tauri = g.__TAURI__;
    if (tauri && !tauri.downloader) {
        Object.defineProperty(tauri, 'downloader', {
            configurable: true,
            value: { downloadFile, downloadBytes, cancelDownload, listActive, onProgress },
        });
    }
}
