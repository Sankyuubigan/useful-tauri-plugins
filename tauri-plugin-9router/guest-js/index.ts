import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

/**
 * Плагин `tauri-plugin-9router` — шлюз облачных LLM через 9Router.
 *
 * Все вызовы идут через `plugin:9router|<command>` (identifier из `Builder::new("9router")`).
 * Установка самодостаточна (портативный node.exe + npm-бандл), сервер стартует
 * лениво по требованию и гасится при выходе приложения.
 */

/** Tauri-событие прогресса установки: `{ stage, done, total, text }`. */
export const PROGRESS_EVENT = '9router-progress'
/** Tauri-событие порции стриминга: `{ text, author, kind }`. */
export const CHUNK_EVENT = '9router-chunk'

export interface NineRouterStatus {
  /** Установлены и node.exe, и бандл 9router. */
  installed: boolean
  /** Порт отвечает (сервер жив). */
  running: boolean
  version: string | null
  node_version: string | null
  port: number
  base_url: string
  /** Папка установки (по умолчанию `<exe>/9router`). */
  path: string
  node_present: boolean
  server_present: boolean
  /** Человеко-читаемое сообщение для UI. */
  message: string
}

/** Комбо 9router (LLM-набор провайдеров с авто-fallback). */
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
  /** Автор-метка для события `9router-chunk` (проброс в UI хоста). */
  author?: string
}

export interface ProgressPayload {
  stage: string
  done: number
  total: number
  text: string
}

export interface ChunkPayload {
  text: string
  author: string
  kind: string
}

// ─────────────────────────────── Команды ───────────────────────────────

/** Статус шлюза (для индикатора). Сервер НЕ запускает. */
export function getStatus(): Promise<NineRouterStatus> {
  return invoke<NineRouterStatus>('plugin:9router|get_status')
}

/**
 * Установить / обновить 9router (портативный Node.js + npm-бандл).
 * @param force переустановить даже при совпадающей версии
 */
export function installOrUpdate(force = false): Promise<NineRouterStatus> {
  return invoke<NineRouterStatus>('plugin:9router|install_or_update', { force })
}

/** Ленивый автозапуск по требованию (no-op, если сервер уже жив). */
export function ensureStarted(): Promise<NineRouterStatus> {
  return invoke<NineRouterStatus>('plugin:9router|ensure_started')
}

/** Остановить сервер. */
export function stop(): Promise<NineRouterStatus> {
  return invoke<NineRouterStatus>('plugin:9router|stop')
}

/**
 * Сменить папку установки и проверить наличие 9Router по новому пути.
 * Возвращает статус, пересчитанный под выбранную папку (installed/running).
 */
export function setRouterDir(path: string): Promise<NineRouterStatus> {
  return invoke<NineRouterStatus>('plugin:9router|set_router_dir', { path })
}

/** Список LLM-комбо (лениво стартует сервер, если нужно). */
export function getCombos(): Promise<ComboInfo[]> {
  return invoke<ComboInfo[]>('plugin:9router|get_combos')
}

/**
 * Сохранить API-ключ 9router (нужен для `/v1/chat/completions` через комбо).
 * Пустая строка — очистить сохранённый ключ. Возвращает статус шлюза.
 */
export function setApiKey(key: string): Promise<NineRouterStatus> {
  return invoke<NineRouterStatus>('plugin:9router|set_api_key', { key })
}

/** Проверить наличие обновления 9router (npm registry). Возвращает версию или null. */
export function checkRouterUpdate(): Promise<string | null> {
  return invoke<string | null>('plugin:9router|check_router_update')
}

/** Открыть веб-дашборд 9router в браузере по умолчанию. */
export function openDashboard(): Promise<void> {
  return invoke<void>('plugin:9router|open_dashboard')
}

/**
 * Чат через 9router (OpenAI-совместимый, стриминг).
 * Порции текста летят событием `9router-chunk`; полный ответ тоже возвращается.
 */
export function chatCompletion(opts: ChatCompletionOptions): Promise<string> {
  return invoke<string>('plugin:9router|chat_completion', {
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

// Side-effect: импорт пакета регистрирует Web Component <nine-router-panel>.
import './web-components'
