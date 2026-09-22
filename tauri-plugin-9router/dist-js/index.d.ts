/**
 * Плагин `tauri-plugin-9router` — шлюз облачных LLM через 9Router.
 *
 * Все вызовы идут через `plugin:9router|<command>` (identifier из `Builder::new("9router")`).
 * Установка самодостаточна (портативный node.exe + npm-бандл), сервер стартует
 * лениво по требованию и гасится при выходе приложения.
 */
/** Tauri-событие прогресса установки: `{ stage, done, total, text }`. */
export declare const PROGRESS_EVENT = "9router-progress";
/** Tauri-событие порции стриминга: `{ text, author, kind }`. */
export declare const CHUNK_EVENT = "9router-chunk";
export interface NineRouterStatus {
    /** Установлены и node.exe, и бандл 9router. */
    installed: boolean;
    /** Порт отвечает (сервер жив). */
    running: boolean;
    version: string | null;
    node_version: string | null;
    port: number;
    base_url: string;
    /** Папка установки (по умолчанию `<exe>/9router`). */
    path: string;
    node_present: boolean;
    server_present: boolean;
    /** Человеко-читаемое сообщение для UI. */
    message: string;
}
/** Комбо 9router (LLM-набор провайдеров с авто-fallback). */
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
    /** Автор-метка для события `9router-chunk` (проброс в UI хоста). */
    author?: string;
}
export interface ProgressPayload {
    stage: string;
    done: number;
    total: number;
    text: string;
}
export interface ChunkPayload {
    text: string;
    author: string;
    kind: string;
}
/** Статус шлюза (для индикатора). Сервер НЕ запускает. */
export declare function getStatus(): Promise<NineRouterStatus>;
/**
 * Установить / обновить 9router (портативный Node.js + npm-бандл).
 * @param force переустановить даже при совпадающей версии
 */
export declare function installOrUpdate(force?: boolean): Promise<NineRouterStatus>;
/** Ленивый автозапуск по требованию (no-op, если сервер уже жив). */
export declare function ensureStarted(): Promise<NineRouterStatus>;
/** Остановить сервер. */
export declare function stop(): Promise<NineRouterStatus>;
/**
 * Сменить папку установки и проверить наличие 9Router по новому пути.
 * Возвращает статус, пересчитанный под выбранную папку (installed/running).
 */
export declare function setRouterDir(path: string): Promise<NineRouterStatus>;
/** Список LLM-комбо (лениво стартует сервер, если нужно). */
export declare function getCombos(): Promise<ComboInfo[]>;
/**
 * Сохранить API-ключ 9router (нужен для `/v1/chat/completions` через комбо).
 * Пустая строка — очистить сохранённый ключ. Возвращает статус шлюза.
 */
export declare function setApiKey(key: string): Promise<NineRouterStatus>;
/** Проверить наличие обновления 9router (npm registry). Возвращает версию или null. */
export declare function checkRouterUpdate(): Promise<string | null>;
/** Открыть веб-дашборд 9router в браузере по умолчанию. */
export declare function openDashboard(): Promise<void>;
/**
 * Чат через 9router (OpenAI-совместимый, стриминг).
 * Порции текста летят событием `9router-chunk`; полный ответ тоже возвращается.
 */
export declare function chatCompletion(opts: ChatCompletionOptions): Promise<string>;
/** Подписка на прогресс установки. Возвращает функцию отписки. */
export declare function onProgress(cb: (p: ProgressPayload) => void): Promise<() => void>;
/** Подписка на порции стриминга чата. Возвращает функцию отписки. */
export declare function onChunk(cb: (c: ChunkPayload) => void): Promise<() => void>;
import './web-components';
