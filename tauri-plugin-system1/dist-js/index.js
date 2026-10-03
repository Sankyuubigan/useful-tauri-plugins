import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
/** Статус System-1: модель, рантайм ONNX, устройство. */
export function getStatus() {
    return invoke('plugin:system1|get_status');
}
/**
 * Скачать весь комплект System-1: сначала ONNX Runtime, затем модель.
 *
 * Единственная команда установки с нуля. Порядок зафиксирован на бэкенде:
 * инференс без DLL невозможен, поэтому качать 646 МБ модели при отсутствии
 * рантайма — тратить время впустую.
 */
export function downloadAll() {
    return invoke('plugin:system1|download_all');
}
/** Скачать модель в `KingOrchData/system1/models/<id>`. */
export function downloadModel(modelId) {
    return invoke('plugin:system1|download_model', { modelId });
}
/** Удалить скачанную модель с диска. */
export function removeModel(modelId) {
    return invoke('plugin:system1|remove_model', { modelId });
}
/**
 * Выполнить типизированные вопросы и вернуть вероятности.
 *
 * Плагин не решает, какой вердикт «правильный» — пороги и критерии живут в хосте.
 */
export function decide(request) {
    return invoke('plugin:system1|decide', { request });
}
/** Подписаться на прогресс скачивания модели. */
export function onDownloadProgress(cb) {
    return listen('downloader:progress', (event) => cb(event.payload));
}
import './web-components';
