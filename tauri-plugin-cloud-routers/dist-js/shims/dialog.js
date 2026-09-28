// Шим '@tauri-apps/plugin-dialog' для vanilla-канала (выбор файла/папки).
const g = window;
export async function open(opts) {
    const dialog = g.__TAURI__?.dialog;
    if (!dialog?.open)
        throw new Error('window.__TAURI__.dialog.open недоступен');
    return dialog.open(opts);
}
export async function save(opts) {
    const dialog = g.__TAURI__?.dialog;
    if (!dialog?.save)
        throw new Error('window.__TAURI__.dialog.save недоступен');
    return dialog.save(opts);
}
