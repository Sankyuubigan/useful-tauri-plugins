// Шим '@tauri-apps/api/event' для vanilla-канала (guest-js -> api-iife.js).
const g = window;
export async function listen(event, handler) {
    const ev = g.__TAURI__?.event;
    if (!ev?.listen) {
        throw new Error('window.__TAURI__.event.listen недоступен');
    }
    return ev.listen(event, handler);
}
