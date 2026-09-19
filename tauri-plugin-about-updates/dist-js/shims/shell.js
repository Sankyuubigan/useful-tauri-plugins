// Шим '@tauri-apps/plugin-shell' для vanilla-канала (открыть ссылку системным браузером).
const g = window;
export async function open(path, openWith) {
    const shell = g.__TAURI__?.shell;
    if (!shell?.open) {
        throw new Error('window.__TAURI__.shell.open недоступен');
    }
    return shell.open(path, openWith);
}
