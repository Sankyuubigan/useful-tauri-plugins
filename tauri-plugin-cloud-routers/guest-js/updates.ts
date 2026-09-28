import { checkRouterUpdate } from './index'

export interface PluginUpdateState {
  hasUpdate: boolean
  tag?: string
}

const ROUTER_ID = '9router'

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

export function setUpdateState(next: PluginUpdateState): void {
  state = next
  subs.forEach((fn) => fn(state))
}

export async function checkUpdate(): Promise<PluginUpdateState> {
  try {
    const tag = await checkRouterUpdate(ROUTER_ID)
    setUpdateState(tag ? { hasUpdate: true, tag } : { hasUpdate: false })
  } catch {
    setUpdateState({ hasUpdate: false })
  }
  return state
}

export function initUpdateWatcher(): void {
  if (started) return
  started = true
  void checkUpdate()
}
