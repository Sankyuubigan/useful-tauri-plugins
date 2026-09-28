import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
/**
 * Плагин `tauri-plugin-cloud-routers` — шлюз облачных LLM.
 *
 * Все вызовы идут через `plugin:cloud-routers|<command>` (identifier из `Builder::new("cloud-routers")`).
 * Установка самодостаточна (портативный node.exe + npm-бандл), сервер стартует
 * лениво по требованию и гасится при выходе приложения.
 */
/** Tauri-событие прогресса установки: `{ router, stage, done, total, text }`. */
export const PROGRESS_EVENT = 'cloud-routers-progress';
/** Tauri-событие порции стриминга: `{ router, text, author, kind }`. */
export const CHUNK_EVENT = 'cloud-routers-chunk';
// ─────────────────────────────── Команды ───────────────────────────────
/** Статус шлюза (для индикатора). Сервер НЕ запускает. */
export function getStatus(router) {
    return invoke('plugin:cloud-routers|get_status', { router });
}
/**
 * Установить / обновить роутер (портативный Node.js + npm-бандл).
 * @param force переустановить даже при совпадающей версии
 */
export function installOrUpdate(router, force = false) {
    return invoke('plugin:cloud-routers|install_or_update', { router, force });
}
/** Ленивый автозапуск по требованию (no-op, если сервер уже жив). */
export function ensureStarted(router) {
    return invoke('plugin:cloud-routers|ensure_started', { router });
}
/** Остановить сервер. */
export function stop(router) {
    return invoke('plugin:cloud-routers|stop', { router });
}
/**
 * Сменить папку установки и проверить наличие роутера по новому пути.
 * Возвращает статус, пересчитанный под выбранную папку (installed/running).
 */
export function setRouterDir(router, path) {
    return invoke('plugin:cloud-routers|set_router_dir', { router, path });
}
/** Список LLM-комбо (лениво стартует сервер, если нужно). */
export function getCombos(router) {
    return invoke('plugin:cloud-routers|get_combos', { router });
}
/**
 * Сохранить API-ключ роутера (нужен для `/v1/chat/completions` через комбо).
 * Пустая строка — очистить сохранённый ключ. Возвращает статус шлюза.
 */
export function setApiKey(router, key) {
    return invoke('plugin:cloud-routers|set_api_key', { router, key });
}
/** Проверить наличие обновления роутера (npm registry). Возвращает версию или null. */
export function checkRouterUpdate(router) {
    return invoke('plugin:cloud-routers|check_router_update', { router });
}
/** Открыть веб-дашборд роутера в браузере по умолчанию. */
export function openDashboard(router) {
    return invoke('plugin:cloud-routers|open_dashboard', { router });
}
/**
 * Чат через роутер (OpenAI-совместимый, стриминг).
 * Порции текста летят событием `cloud-routers-chunk`; полный ответ тоже возвращается.
 */
export function chatCompletion(router, opts) {
    return invoke('plugin:cloud-routers|chat_completion', {
        router,
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
// Side-effect: импорт пакета регистрирует Web Component <cloud-routers-panel>.
import './web-components';
