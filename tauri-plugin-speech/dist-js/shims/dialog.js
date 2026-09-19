// Шим '@tauri-apps/plugin-dialog' для vanilla-канала.
// Воспроизводится только open() — выбора папок/файлов в панелях Speech.
const g = window;
export async function open(opts) {
    const dlg = g.__TAURI__?.dialog;
    if (!dlg?.open)
        return null;
    return dlg.open(opts);
}
