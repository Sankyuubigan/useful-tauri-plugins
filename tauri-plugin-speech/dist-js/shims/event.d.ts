export type UnlistenFn = () => void;
export declare function listen<T = unknown>(event: string, handler: (event: {
    payload: T;
}) => void): Promise<UnlistenFn>;
