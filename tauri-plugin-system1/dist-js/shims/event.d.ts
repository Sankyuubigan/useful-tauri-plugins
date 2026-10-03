/**
 * Функция отписки. Ошибка в официальном API, на которую полагаются подписчики:
 * без неё `return () => unlisten()` не типизируется, и любой вызов в UI
 * пришлось бы игнорировать тип через `as any`.
 */
export type UnlistenFn = () => void;
export declare function listen<T = unknown>(name: string, cb: (e: {
    payload: T;
}) => void): Promise<UnlistenFn>;
