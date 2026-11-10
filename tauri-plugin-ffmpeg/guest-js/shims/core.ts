// Шим IIFE-канала: @tauri-apps/api/core → глобалы окна (PLUGIN_STANDARD §5.4).
const core =
  (window as unknown as Record<string, any>).__TAURI__?.core ??
  (window as unknown as Record<string, any>).__TAURI_INTERNALS__

export const invoke = core.invoke.bind(core)
