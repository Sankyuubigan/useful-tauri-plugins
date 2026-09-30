import { invoke } from '@tauri-apps/api/core';
export function getYtdlpStatus() {
    return invoke('plugin:ytdlp|get_ytdlp_status');
}
export function installYtdlp() {
    return invoke('plugin:ytdlp|install_ytdlp');
}
export function checkYtdlpUpdate() {
    return invoke('plugin:ytdlp|check_ytdlp_update');
}
export function installYtdlpUpdate() {
    return invoke('plugin:ytdlp|install_ytdlp_update');
}
export function setYtdlpDir(path) {
    return invoke('plugin:ytdlp|set_ytdlp_dir', { path });
}
export function setFfmpegPath(path) {
    return invoke('plugin:ytdlp|set_ffmpeg_path', { path });
}
export function installDeno() {
    return invoke('plugin:ytdlp|install_deno');
}
export function fetchVideoInfo(url) {
    return invoke('plugin:ytdlp|fetch_video_info', { url });
}
export function downloadVideo(params) {
    return invoke('plugin:ytdlp|download_video', {
        url: params.url,
        format: params.format,
        quality: params.quality,
        outputDir: params.outputDir,
        authMode: params.authMode,
        browser: params.browser ?? null,
        cookiesFile: params.cookiesFile ?? null,
    });
}
export function cancelDownload() {
    return invoke('plugin:ytdlp|cancel_download');
}
import './web-components';
