export declare function listen<T = unknown>(name: string, cb: (e: {
    payload: T;
}) => void): Promise<() => void>;
