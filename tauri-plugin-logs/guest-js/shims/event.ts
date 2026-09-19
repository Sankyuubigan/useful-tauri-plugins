// Шим '@tauri-apps/api/event' для vanilla-канала (guest-js -> api-iife.js).
const g = window as unknown as {
  __TAURI__?: {
    event?: {
      listen?: (event: string, handler: (e: { payload: unknown }) => void) => Promise<() => void>
    }
  }
}

export type UnlistenFn = () => void

export async function listen<T = unknown>(
  event: string,
  handler: (event: { payload: T }) => void,
): Promise<UnlistenFn> {
  const ev = g.__TAURI__?.event
  if (!ev?.listen) {
    throw new Error('window.__TAURI__.event.listen недоступен')
  }
  return ev.listen(event, handler as (e: { payload: unknown }) => void)
}