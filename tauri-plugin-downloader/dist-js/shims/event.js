const g = window;
export async function listen(event, handler) {
    const tauriEvent = g.__TAURI__?.event;
    if (!tauriEvent?.listen) {
        throw new Error('window.__TAURI__.event.listen недоступен');
    }
    const unlisten = (await tauriEvent.listen(event, handler));
    return typeof unlisten === 'function' ? unlisten : () => { };
}
