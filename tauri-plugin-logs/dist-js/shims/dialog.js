// Шим '@tauri-apps/plugin-dialog' для vanilla-канала.
// Диалог плагина вхосте доступен как window.__TAURI__.dialog (см. PLUGIN_STANDARD.md §4.4).
// Воспроизводится только save(): единственная используемая функция lогового панели.
const g = window;
export async function save(opts) {
    const dlg = g.__TAURI__?.dialog;
    if (!dlg?.save)
        return null;
    return dlg.save(opts);
}
