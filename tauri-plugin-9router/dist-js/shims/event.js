// Шим '@tauri-apps/api/event' для vanilla-канала (listen на Tauri-события).
const g = window;
export async function listen(name, cb) {
    const event = g.__TAURI__?.event ?? g.__TAURI_INTERNALS__?.event;
    if (!event?.listen)
        throw new Error('window.__TAURI__.event.listen недоступен');
    return event.listen(name, (e) => cb(e));
}
