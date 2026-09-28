import { checkRouterUpdate } from './index';
const ROUTER_ID = '9router';
let state = { hasUpdate: false };
const subs = new Set();
let started = false;
export function getUpdateState() {
    return state;
}
export function onUpdateState(fn) {
    subs.add(fn);
    fn(state);
    return () => { subs.delete(fn); };
}
export function setUpdateState(next) {
    state = next;
    subs.forEach((fn) => fn(state));
}
export async function checkUpdate() {
    try {
        const tag = await checkRouterUpdate(ROUTER_ID);
        setUpdateState(tag ? { hasUpdate: true, tag } : { hasUpdate: false });
    }
    catch {
        setUpdateState({ hasUpdate: false });
    }
    return state;
}
export function initUpdateWatcher() {
    if (started)
        return;
    started = true;
    void checkUpdate();
}
