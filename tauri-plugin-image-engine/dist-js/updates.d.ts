export interface PluginUpdateState {
    hasUpdate: boolean;
    tag?: string;
}
export declare function getUpdateState(): PluginUpdateState;
export declare function onUpdateState(fn: (s: PluginUpdateState) => void): () => void;
/** Единственная точка записи состояния апдейта — от неё зависят и панель, и бейдж хоста. */
export declare function setUpdateState(next: PluginUpdateState): void;
/**
 * Проверка апдейта движка.
 *
 * При сетевой ошибке состояние НЕ трогаем: «не смогли проверить» ≠ «обновлений нет»
 * (core/rules.md §2.2 — запрет лжи). Ошибку поднимаем наверх: панель показывает её
 * юзеру, фоновый watcher глотает молча, не переписывая состояние.
 */
export declare function checkUpdate(): Promise<PluginUpdateState>;
export declare function initUpdateWatcher(): void;
