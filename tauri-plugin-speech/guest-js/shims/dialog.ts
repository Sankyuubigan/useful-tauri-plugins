// Шим '@tauri-apps/plugin-dialog' для vanilla-канала.
// Воспроизводится только open() — выбора папок/файлов в панелях Speech.
const g = window as unknown as {
  __TAURI__?: {
    dialog?: {
      open?: (opts?: {
        directory?: boolean
        multiple?: boolean
        filters?: Array<{ name: string; extensions: string[] }>
      }) => Promise<string | string[] | null>
    }
  }
}

export async function open(opts?: {
  directory?: boolean
  multiple?: boolean
  filters?: Array<{ name: string; extensions: string[] }>
}): Promise<string | string[] | null> {
  const dlg = g.__TAURI__?.dialog
  if (!dlg?.open) return null
  return dlg.open(opts)
}