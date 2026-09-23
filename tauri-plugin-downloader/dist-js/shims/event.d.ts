export type UnlistenFn = () => void;
export interface Event<T> {
    event: string;
    payload: T;
}
export declare function listen<T>(event: string, handler: (event: Event<T>) => void): Promise<UnlistenFn>;
