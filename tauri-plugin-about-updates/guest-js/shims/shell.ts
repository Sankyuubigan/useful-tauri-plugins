// Шим '@tauri-apps/plugin-shell' для vanilla-канала (открыть ссылку системным браузером).
const g = window as unknown as {
  __TAURI__?: { shell?: { open?: (path: string, openWith?: string) => Promise<void> } }
}

export async function open(path: string, openWith?: string): Promise<void> {
  const shell = g.__TAURI__?.shell
  if (!shell?.open) {
    throw new Error('window.__TAURI__.shell.open недоступен')
  }
  return shell.open(path, openWith)
}