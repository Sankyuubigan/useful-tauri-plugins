import { getFfmpegStatus } from './index';
const g = window;
if ('__TAURI__' in window) {
    const tauri = g.__TAURI__;
    if (tauri && !tauri['ffmpeg']) {
        Object.defineProperty(tauri, 'ffmpeg', {
            configurable: true,
            value: { getFfmpegStatus },
        });
    }
}
