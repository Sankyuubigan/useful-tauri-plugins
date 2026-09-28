// Шим '@tauri-apps/plugin-dialog' для vanilla-канала (выбор файла/папки).
const g = window as unknown as {
  __TAURI__?: { dialog?: { open?: (opts: unknown) => Promise<unknown>; save?: (opts: unknown) => Promise<unknown> } }
}

export async function open(opts: unknown): Promise<string | string[] | null> {
  const dialog = g.__TAURI__?.dialog
  if (!dialog?.open) throw new Error('window.__TAURI__.dialog.open недоступен')
  return dialog.open(opts) as Promise<string | string[] | null>
}

export async function save(opts: unknown): Promise<string | null> {
  const dialog = g.__TAURI__?.dialog
  if (!dialog?.save) throw new Error('window.__TAURI__.dialog.save недоступен')
  return dialog.save(opts) as Promise<string | null>
}