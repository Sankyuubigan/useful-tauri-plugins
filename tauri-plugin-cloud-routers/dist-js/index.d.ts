/**
 * Плагин `tauri-plugin-cloud-routers` — шлюз облачных LLM.
 *
 * Все вызовы идут через `plugin:cloud-routers|<command>` (identifier из `Builder::new("cloud-routers")`).
 * Установка самодостаточна (портативный node.exe + npm-бандл), сервер стартует
 * лениво по требованию и гасится при выходе приложения.
 */
/** Tauri-событие прогресса установки: `{ router, stage, done, total, text }`. */
export declare const PROGRESS_EVENT = "cloud-routers-progress";
/** Tauri-событие порции стриминга: `{ router, text, author, kind }`. */
export declare const CHUNK_EVENT = "cloud-routers-chunk";
/** Идентификатор роутера. */
export type RouterId = '9router' | 'extremerouter' | 'omniroute';
export interface NineRouterStatus {
    /** Установлены и node.exe, и бандл роутера. */
    installed: boolean;
    /** Порт отвечает (сервер жив). */
    running: boolean;
    version: string | null;
    node_version: string | null;
    port: number;
    base_url: string;
    /** Папка установки (по умолчанию `<exe>/cloud_routers/<id>`). */
    path: string;
    data_dir: string;
    db_present: boolean;
    node_present: boolean;
    server_present: boolean;
    /** Человеко-читаемое сообщение для UI. */
    message: string;
}
/** Комбо роутера (LLM-набор провайдеров с авто-fallback). */
export interface ComboInfo {
    name: string;
    kind?: string | null;
    models: string[];
}
export interface ChatMessage {
    role: 'system' | 'user' | 'assistant' | string;
    content: string;
}
export interface ChatCompletionOptions {
    /** Имя комбо (значение из `getCombos`). */
    model: string;
    messages: ChatMessage[];
    maxTokens?: number;
    temperature?: number;
    /** Автор-метка для события `cloud-routers-chunk` (проброс в UI хоста). */
    author?: string;
}
export interface ProgressPayload {
    router: RouterId;
    stage: string;
    done: number;
    total: number;
    text: string;
}
export interface ChunkPayload {
    router: RouterId;
    text: string;
    author: string;
    kind: string;
}
/** Статус шлюза (для индикатора). Сервер НЕ запускает. */
export declare function getStatus(router: RouterId): Promise<NineRouterStatus>;
/**
 * Установить / обновить роутер (портативный Node.js + npm-бандл).
 * @param force переустановить даже при совпадающей версии
 */
export declare function installOrUpdate(router: RouterId, force?: boolean): Promise<NineRouterStatus>;
/** Ленивый автозапуск по требованию (no-op, если сервер уже жив). */
export declare function ensureStarted(router: RouterId): Promise<NineRouterStatus>;
/** Остановить сервер. */
export declare function stop(router: RouterId): Promise<NineRouterStatus>;
/**
 * Сменить папку установки и проверить наличие роутера по новому пути.
 * Возвращает статус, пересчитанный под выбранную папку (installed/running).
 */
export declare function setRouterDir(router: RouterId, path: string): Promise<NineRouterStatus>;
/** Список LLM-комбо (лениво стартует сервер, если нужно). */
export declare function getCombos(router: RouterId): Promise<ComboInfo[]>;
/**
 * Сохранить API-ключ роутера (нужен для `/v1/chat/completions` через комбо).
 * Пустая строка — очистить сохранённый ключ. Возвращает статус шлюза.
 */
export declare function setApiKey(router: RouterId, key: string): Promise<NineRouterStatus>;
/** Проверить наличие обновления роутера (npm registry). Возвращает версию или null. */
export declare function checkRouterUpdate(router: RouterId): Promise<string | null>;
/** Открыть веб-дашборд роутера в браузере по умолчанию. */
export declare function openDashboard(router: RouterId): Promise<void>;
/**
 * Чат через роутер (OpenAI-совместимый, стриминг).
 * Порции текста летят событием `cloud-routers-chunk`; полный ответ тоже возвращается.
 */
export declare function chatCompletion(router: RouterId, opts: ChatCompletionOptions): Promise<string>;
/** Подписка на прогресс установки. Возвращает функцию отписки. */
export declare function onProgress(cb: (p: ProgressPayload) => void): Promise<() => void>;
/** Подписка на порции стриминга чата. Возвращает функцию отписки. */
export declare function onChunk(cb: (c: ChunkPayload) => void): Promise<() => void>;
import './web-components';
