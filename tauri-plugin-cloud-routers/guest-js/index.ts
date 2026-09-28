import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

/**
 * Плагин `tauri-plugin-cloud-routers` — шлюз облачных LLM.
 *
 * Все вызовы идут через `plugin:cloud-routers|<command>` (identifier из `Builder::new("cloud-routers")`).
 * Установка самодостаточна (портативный node.exe + npm-бандл), сервер стартует
 * лениво по требованию и гасится при выходе приложения.
 */

/** Tauri-событие прогресса установки: `{ router, stage, done, total, text }`. */
export const PROGRESS_EVENT = 'cloud-routers-progress'
/** Tauri-событие порции стриминга: `{ router, text, author, kind }`. */
export const CHUNK_EVENT = 'cloud-routers-chunk'

/** Идентификатор роутера. */
export type RouterId = '9router' | 'extremerouter' | 'omniroute'

export interface NineRouterStatus {
  /** Установлены и node.exe, и бандл роутера. */
  installed: boolean
  /** Порт отвечает (сервер жив). */
  running: boolean
  version: string | null
  node_version: string | null
  port: number
  base_url: string
  /** Папка установки (по умолчанию `<exe>/cloud_routers/<id>`). */
  path: string
  data_dir: string
  db_present: boolean
  node_present: boolean
  server_present: boolean
  /** Человеко-читаемое сообщение для UI. */
  message: string
}

/** Комбо роутера (LLM-набор провайдеров с авто-fallback). */
export interface ComboInfo {
  name: string
  kind?: string | null
  models: string[]
}

export interface ChatMessage {
  role: 'system' | 'user' | 'assistant' | string
  content: string
}

export interface ChatCompletionOptions {
  /** Имя комбо (значение из `getCombos`). */
  model: string
  messages: ChatMessage[]
  maxTokens?: number
  temperature?: number
  /** Автор-метка для события `cloud-routers-chunk` (проброс в UI хоста). */
  author?: string
}

export interface ProgressPayload {
  router: RouterId
  stage: string
  done: number
  total: number
  text: string
}

export interface ChunkPayload {
  router: RouterId
  text: string
  author: string
  kind: string
}

// ─────────────────────────────── Команды ───────────────────────────────

/** Статус шлюза (для индикатора). Сервер НЕ запускает. */
export function getStatus(router: RouterId): Promise<NineRouterStatus> {
  return invoke<NineRouterStatus>('plugin:cloud-routers|get_status', { router })
}

/**
 * Установить / обновить роутер (портативный Node.js + npm-бандл).
 * @param force переустановить даже при совпадающей версии
 */
export function installOrUpdate(router: RouterId, force = false): Promise<NineRouterStatus> {
  return invoke<NineRouterStatus>('plugin:cloud-routers|install_or_update', { router, force })
}

/** Ленивый автозапуск по требованию (no-op, если сервер уже жив). */
export function ensureStarted(router: RouterId): Promise<NineRouterStatus> {
  return invoke<NineRouterStatus>('plugin:cloud-routers|ensure_started', { router })
}

/** Остановить сервер. */
export function stop(router: RouterId): Promise<NineRouterStatus> {
  return invoke<NineRouterStatus>('plugin:cloud-routers|stop', { router })
}

/**
 * Сменить папку установки и проверить наличие роутера по новому пути.
 * Возвращает статус, пересчитанный под выбранную папку (installed/running).
 */
export function setRouterDir(router: RouterId, path: string): Promise<NineRouterStatus> {
  return invoke<NineRouterStatus>('plugin:cloud-routers|set_router_dir', { router, path })
}

/** Список LLM-комбо (лениво стартует сервер, если нужно). */
export function getCombos(router: RouterId): Promise<ComboInfo[]> {
  return invoke<ComboInfo[]>('plugin:cloud-routers|get_combos', { router })
}

/**
 * Сохранить API-ключ роутера (нужен для `/v1/chat/completions` через комбо).
 * Пустая строка — очистить сохранённый ключ. Возвращает статус шлюза.
 */
export function setApiKey(router: RouterId, key: string): Promise<NineRouterStatus> {
  return invoke<NineRouterStatus>('plugin:cloud-routers|set_api_key', { router, key })
}

/** Проверить наличие обновления роутера (npm registry). Возвращает версию или null. */
export function checkRouterUpdate(router: RouterId): Promise<string | null> {
  return invoke<string | null>('plugin:cloud-routers|check_router_update', { router })
}

/** Открыть веб-дашборд роутера в браузере по умолчанию. */
export function openDashboard(router: RouterId): Promise<void> {
  return invoke<void>('plugin:cloud-routers|open_dashboard', { router })
}

/**
 * Чат через роутер (OpenAI-совместимый, стриминг).
 * Порции текста летят событием `cloud-routers-chunk`; полный ответ тоже возвращается.
 */
export function chatCompletion(router: RouterId, opts: ChatCompletionOptions): Promise<string> {
  return invoke<string>('plugin:cloud-routers|chat_completion', {
    router,
    model: opts.model,
    messages: opts.messages,
    maxTokens: opts.maxTokens,
    temperature: opts.temperature,
    author: opts.author,
  })
}

// ─────────────────────────────── События ───────────────────────────────

/** Подписка на прогресс установки. Возвращает функцию отписки. */
export function onProgress(cb: (p: ProgressPayload) => void): Promise<() => void> {
  return listen<ProgressPayload>(PROGRESS_EVENT, (e) => cb(e.payload))
}

/** Подписка на порции стриминга чата. Возвращает функцию отписки. */
export function onChunk(cb: (c: ChunkPayload) => void): Promise<() => void> {
  return listen<ChunkPayload>(CHUNK_EVENT, (e) => cb(e.payload))
}

// Side-effect: импорт пакета регистрирует Web Component <cloud-routers-panel>.
import './web-components'
