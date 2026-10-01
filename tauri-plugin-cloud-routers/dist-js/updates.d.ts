import { type RouterId } from './index';
export interface PluginUpdateState {
    hasUpdate: boolean;
    tag?: string;
}
/** Состояние апдейта: конкретного роутера, либо агрегат по всем (если router не передан). */
export declare function getUpdateState(router?: RouterId): PluginUpdateState;
/** Подписка на агрегат (для хоста). Панель читает своё состояние через `getUpdateState(router)`. */
export declare function onUpdateState(fn: (s: PluginUpdateState) => void): () => void;
/** Единственная точка записи состояния апдейта. */
export declare function setUpdateState(router: RouterId, next: PluginUpdateState): void;
/**
 * Проверка апдейта одного роутера (npm registry).
 *
 * При сетевой ошибке состояние НЕ трогаем: «не смогли проверить» ≠ «обновлений нет»
 * (core/rules.md §2.2 — запрет лжи). Ошибку поднимаем наверх: панель показывает её
 * юзеру, фоновый watcher глотает молча, не переписывая состояние.
 */
export declare function checkUpdate(router: RouterId): Promise<PluginUpdateState>;
export declare function initUpdateWatcher(): void;
