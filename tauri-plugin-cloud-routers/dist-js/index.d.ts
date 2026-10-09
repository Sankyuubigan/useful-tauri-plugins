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
/** Идентификаторы роутеров — единственный источник истины (тип выводится из списка). */
export declare const ROUTER_IDS: readonly ["9router", "extremerouter", "omniroute", "gateway"];
/** Идентификатор роутера. */
export type RouterId = (typeof ROUTER_IDS)[number];
/** Вид роутера: node-бандл из npm либо наш собственный бинарь шлюза. */
export type RouterKind = 'node-bundle' | 'native-gateway';
export interface RouterStatus {
    /** Роутер установлен (для шлюза — наличие бинаря). */
    installed: boolean;
    /** Порт отвечает (сервер жив). */
    running: boolean;
    version: string | null;
    /** Версия портативного Node.js. У шлюза — `null`: у него нет Node. */
    node_version: string | null;
    port: number;
    base_url: string;
    /** Папка установки. */
    path: string;
    data_dir: string;
    db_present: boolean;
    node_present: boolean;
    server_present: boolean;
    /** Человеко-читаемое сообщение для UI. */
    message: string;
    /**
     * Вид роутера.
     *
     * UI обязан смотреть на это поле, а не угадывать вид по наличию
     * `node_present`: у шлюза Node нет, и «Node —» выглядит как у node-роутера с
     * не установленным Node, хотя кнопки у них разные.
     */
    kind: RouterKind;
    /** Нужна ли кнопка установки. */
    can_install: boolean;
    /** Есть ли смысл в проверке обновления (только у npm-роутеров). */
    can_check_update: boolean;
    /** Показывать ли строки Node и БД. */
    has_npm_runtime: boolean;
}
/**
 * Историческое имя интерфейса статуса.
 *
 * Переименовано в `RouterStatus` вместе с добавлением шлюза: у роутеров больше
 * одного вида, и имя про один из них вводило в заблуждение. Экспорт оставлен
 * для совместимости с существующим кодом хоста.
 */
export type NineRouterStatus = RouterStatus;
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
export declare function getStatus(router: RouterId): Promise<RouterStatus>;
/**
 * Установить / обновить роутер (портативный Node.js + npm-бандл).
 * @param force переустановить даже при совпадающей версии
 */
export declare function installOrUpdate(router: RouterId, force?: boolean): Promise<RouterStatus>;
/** Ленивый автозапуск по требованию (no-op, если сервер уже жив). */
export declare function ensureStarted(router: RouterId): Promise<RouterStatus>;
/** Остановить сервер. */
export declare function stop(router: RouterId): Promise<RouterStatus>;
/**
 * Сменить папку установки и проверить наличие роутера по новому пути.
 * Возвращает статус, пересчитанный под выбранную папку (installed/running).
 */
export declare function setRouterDir(router: RouterId, path: string): Promise<RouterStatus>;
/** Список LLM-комбо (лениво стартует сервер, если нужно). */
export declare function getCombos(router: RouterId): Promise<ComboInfo[]>;
/**
 * Сохранить API-ключ роутера (нужен для `/v1/chat/completions` через комбо).
 * Пустая строка — очистить сохранённый ключ. Возвращает статус шлюза.
 */
export declare function setApiKey(router: RouterId, key: string): Promise<RouterStatus>;
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
export { getUpdateState, onUpdateState, setUpdateState, checkUpdate, initUpdateWatcher, } from './updates';
export type { PluginUpdateState } from './updates';
