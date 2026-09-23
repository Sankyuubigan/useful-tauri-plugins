import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
/** Скачать файл. Прогресс — событие `downloader:progress`. */
export function downloadFile(url, dest, opts) {
    return invoke('plugin:downloader|download_file', { url, dest, opts: opts ?? null });
}
/** Скачать во временный файл и вернуть байты (маленькие ответы). */
export function downloadBytes(url, opts) {
    return invoke('plugin:downloader|download_bytes', { url, opts: opts ?? null });
}
/** Отменить активную загрузку. true — задача найдена. */
export function cancelDownload(taskId) {
    return invoke('plugin:downloader|cancel_download', { taskId });
}
/** Список активных загрузок. */
export function listActive() {
    return invoke('plugin:downloader|list_active');
}
/** Подписка на единый прогресс-событие. */
export function onProgress(cb) {
    return listen('downloader:progress', (e) => cb(e.payload));
}
import './web-components';
