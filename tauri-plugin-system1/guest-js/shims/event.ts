// Шим '@tauri-apps/api/event' для vanilla-канала (listen на Tauri-события).
const g = window as unknown as {
  __TAURI__?: { event?: { listen?: (name: string, cb: (e: { payload: unknown }) => void) => Promise<() => void> } }
  __TAURI_INTERNALS__?: { event?: { listen?: (name: string, cb: (e: { payload: unknown }) => void) => Promise<() => void> } }
}

/**
 * Функция отписки. Ошибка в официальном API, на которую полагаются подписчики:
 * без неё `return () => unlisten()` не типизируется, и любой вызов в UI
 * пришлось бы игнорировать тип через `as any`.
 */
export type UnlistenFn = () => void

export async function listen<T = unknown>(
  name: string,
  cb: (e: { payload: T }) => void,
): Promise<UnlistenFn> {
  const event = g.__TAURI__?.event ?? g.__TAURI_INTERNALS__?.event
  if (!event?.listen) throw new Error('window.__TAURI__.event.listen недоступен')
  const unlisten = await event.listen(name, (e) => cb(e as { payload: T }))
  return unlisten as UnlistenFn
}