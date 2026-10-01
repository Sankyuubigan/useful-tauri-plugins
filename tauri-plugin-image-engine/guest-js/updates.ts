import { checkImageEngineUpdate } from './index'

export interface PluginUpdateState {
  hasUpdate: boolean
  tag?: string
}

let state: PluginUpdateState = { hasUpdate: false }
const subs = new Set<(s: PluginUpdateState) => void>()
let started = false

export function getUpdateState(): PluginUpdateState {
  return state
}

export function onUpdateState(fn: (s: PluginUpdateState) => void): () => void {
  subs.add(fn)
  fn(state)
  return () => { subs.delete(fn) }
}

/** Единственная точка записи состояния апдейта — от неё зависят и панель, и бейдж хоста. */
export function setUpdateState(next: PluginUpdateState): void {
  state = next
  subs.forEach((fn) => fn(state))
}

/**
 * Проверка апдейта движка.
 *
 * При сетевой ошибке состояние НЕ трогаем: «не смогли проверить» ≠ «обновлений нет»
 * (core/rules.md §2.2 — запрет лжи). Ошибку поднимаем наверх: панель показывает её
 * юзеру, фоновый watcher глотает молча, не переписывая состояние.
 */
export async function checkUpdate(): Promise<PluginUpdateState> {
  const tag = await checkImageEngineUpdate()
  const next: PluginUpdateState = tag ? { hasUpdate: true, tag } : { hasUpdate: false }
  setUpdateState(next)
  return next
}

export function initUpdateWatcher(): void {
  if (started) return
  started = true
  void checkUpdate().catch(() => {
    // Фоновая проверка при старте: результат не запрашивал юзер, состояние не портим.
    // Ручная проверка кнопкой «Проверить обновление» ошибку показывает явно.
  })
}
