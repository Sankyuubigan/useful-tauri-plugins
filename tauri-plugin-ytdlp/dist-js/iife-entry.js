import { cancelDownload, checkYtdlpUpdate, downloadVideo, fetchVideoInfo, getYtdlpStatus, installDeno, installYtdlp, installYtdlpUpdate, setFfmpegPath, setYtdlpDir, } from './index';
const g = window;
if ('__TAURI__' in window) {
    const tauri = g.__TAURI__;
    if (tauri && !tauri['ytdlp']) {
        Object.defineProperty(tauri, 'ytdlp', {
            configurable: true,
            value: {
                cancelDownload,
                checkYtdlpUpdate,
                downloadVideo,
                fetchVideoInfo,
                getYtdlpStatus,
                installDeno,
                installYtdlp,
                installYtdlpUpdate,
                setFfmpegPath,
                setYtdlpDir,
            },
        });
    }
}
