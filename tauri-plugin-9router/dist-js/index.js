import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
/**
 * Плагин `tauri-plugin-9router` — шлюз облачных LLM через 9Router.
 *
 * Все вызовы идут через `plugin:9router|<command>` (identifier из `Builder::new("9router")`).
 * Установка самодостаточна (портативный node.exe + npm-бандл), сервер стартует
 * лениво по требованию и гасится при выходе приложения.
 */
/** Tauri-событие прогресса установки: `{ stage, done, total, text }`. */
export const PROGRESS_EVENT = '9router-progress';
/** Tauri-событие порции стриминга: `{ text, author, kind }`. */
export const CHUNK_EVENT = '9router-chunk';
// ─────────────────────────────── Команды ───────────────────────────────
/** Статус шлюза (для индикатора). Сервер НЕ запускает. */
export function getStatus() {
    return invoke('plugin:9router|get_status');
}
/**
 * Установить / обновить 9router (портативный Node.js + npm-бандл).
 * @param force переустановить даже при совпадающей версии
 */
export function installOrUpdate(force = false) {
    return invoke('plugin:9router|install_or_update', { force });
}
/** Ленивый автозапуск по требованию (no-op, если сервер уже жив). */
export function ensureStarted() {
    return invoke('plugin:9router|ensure_started');
}
/** Остановить сервер. */
export function stop() {
    return invoke('plugin:9router|stop');
}
/**
 * Сменить папку установки и проверить наличие 9Router по новому пути.
 * Возвращает статус, пересчитанный под выбранную папку (installed/running).
 */
export function setRouterDir(path) {
    return invoke('plugin:9router|set_router_dir', { path });
}
/** Список LLM-комбо (лениво стартует сервер, если нужно). */
export function getCombos() {
    return invoke('plugin:9router|get_combos');
}
/** Открыть веб-дашборд 9router в браузере по умолчанию. */
export function openDashboard() {
    return invoke('plugin:9router|open_dashboard');
}
/**
 * Чат через 9router (OpenAI-совместимый, стриминг).
 * Порции текста летят событием `9router-chunk`; полный ответ тоже возвращается.
 */
export function chatCompletion(opts) {
    return invoke('plugin:9router|chat_completion', {
        model: opts.model,
        messages: opts.messages,
        maxTokens: opts.maxTokens,
        temperature: opts.temperature,
        author: opts.author,
    });
}
// ─────────────────────────────── События ───────────────────────────────
/** Подписка на прогресс установки. Возвращает функцию отписки. */
export function onProgress(cb) {
    return listen(PROGRESS_EVENT, (e) => cb(e.payload));
}
/** Подписка на порции стриминга чата. Возвращает функцию отписки. */
export function onChunk(cb) {
    return listen(CHUNK_EVENT, (e) => cb(e.payload));
}
// Side-effect: импорт пакета регистрирует Web Component <nine-router-panel>.
import './web-components';
