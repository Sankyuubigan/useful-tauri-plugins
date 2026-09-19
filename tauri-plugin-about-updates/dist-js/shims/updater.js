// Шим '@tauri-apps/plugin-updater' для vanilla-канала.
// Глобальный апдейтер (window.__TAURI__.updater) возвращает объект Update
// с методом downloadAndInstall() — совместимо с реальным типом из npm-пакета.
const g = window;
export async function check(options) {
    const updater = g.__TAURI__?.updater;
    if (!updater?.check) {
        throw new Error('window.__TAURI__.updater.check недоступен');
    }
    return updater.check(options);
}
