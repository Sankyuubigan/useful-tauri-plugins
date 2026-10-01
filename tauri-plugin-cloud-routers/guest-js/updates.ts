import { checkRouterUpdate, ROUTER_IDS, type RouterId } from './index'

export interface PluginUpdateState {
  hasUpdate: boolean
  tag?: string
}

/**
 * Состояние апдейта — keyed по роутеру: апдейт 9Router не должен светиться
 * на вкладках ExtremeRouter/OmniRoute. Агрегат (без аргумента) нужен хосту,
 * который рисует одну точку «у плагина есть обновление».
 */
const states = new Map<RouterId, PluginUpdateState>()
const subs = new Set<(s: PluginUpdateState) => void>()
/** Роутеры с уже идущей проверкой — защита от дублей (таймер панели + ручной клик). */
const pending = new Set<RouterId>()

/** Состояние апдейта: конкретного роутера, либо агрегат по всем (если router не передан). */
export function getUpdateState(router?: RouterId): PluginUpdateState {
  if (router) return states.get(router) ?? { hasUpdate: false }
  return { hasUpdate: Array.from(states.values()).some((s) => s.hasUpdate) }
}

/** Подписка на агрегат (для хоста). Панель читает своё состояние через `getUpdateState(router)`. */
export function onUpdateState(fn: (s: PluginUpdateState) => void): () => void {
  subs.add(fn)
  fn(getUpdateState())
  return () => { subs.delete(fn) }
}

/** Единственная точка записи состояния апдейта. */
export function setUpdateState(router: RouterId, next: PluginUpdateState): void {
  states.set(router, next)
  const aggregate = getUpdateState()
  subs.forEach((fn) => fn(aggregate))
}

/**
 * Проверка апдейта одного роутера (npm registry).
 *
 * При сетевой ошибке состояние НЕ трогаем: «не смогли проверить» ≠ «обновлений нет»
 * (core/rules.md §2.2 — запрет лжи). Ошибку поднимаем наверх: панель показывает её
 * юзеру, фоновый watcher глотает молча, не переписывая состояние.
 */
export async function checkUpdate(router: RouterId): Promise<PluginUpdateState> {
  if (pending.has(router)) return getUpdateState(router)
  pending.add(router)
  try {
    const tag = await checkRouterUpdate(router)
    const next: PluginUpdateState = tag ? { hasUpdate: true, tag } : { hasUpdate: false }
    setUpdateState(router, next)
    return next
  } finally {
    pending.delete(router)
  }
}

export function initUpdateWatcher(): void {
  for (const router of ROUTER_IDS) {
    void checkUpdate(router).catch(() => {
      // Фоновая проверка при старте: результат не запрашивал юзер, состояние не портим.
      // Ручная проверка кнопкой «Проверить обновление» ошибку показывает явно.
    })
  }
}