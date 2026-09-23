// Шим '@tauri-apps/api/core' для vanilla-канала (guest-js -> api-iife.js).
// В IIFE голые импорты '@tauri-apps/*' запрещены: биндимся на window.__TAURI__.
const g = window as unknown as {
  __TAURI__?: { core?: { invoke?: (...args: unknown[]) => Promise<unknown> } }
  __TAURI_INTERNALS__?: { invoke?: (...args: unknown[]) => Promise<unknown> }
}

function coreInvoke(...args: unknown[]): Promise<unknown> {
  const core = g.__TAURI__?.core
  if (core?.invoke) return core.invoke(...args)
  const internals = g.__TAURI_INTERNALS__
  if (internals?.invoke) return internals.invoke(...args)
  return Promise.reject(new Error('window.__TAURI__.core.invoke недоступен'))
}

export function invoke<T = unknown>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return coreInvoke(cmd, args ?? {}) as Promise<T>
}
