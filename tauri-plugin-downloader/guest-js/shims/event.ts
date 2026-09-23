// Шим '@tauri-apps/api/event' для vanilla-канала (guest-js -> api-iife.js).
export type UnlistenFn = () => void

export interface Event<T> {
  event: string
  payload: T
}

const g = window as unknown as {
  __TAURI__?: { event?: { listen?: (...args: unknown[]) => Promise<unknown> } }
}

export async function listen<T>(
  event: string,
  handler: (event: Event<T>) => void,
): Promise<UnlistenFn> {
  const tauriEvent = g.__TAURI__?.event
  if (!tauriEvent?.listen) {
    throw new Error('window.__TAURI__.event.listen недоступен')
  }
  const unlisten = (await tauriEvent.listen(event, handler)) as UnlistenFn | (() => void)
  return typeof unlisten === 'function' ? unlisten : () => {}
}
