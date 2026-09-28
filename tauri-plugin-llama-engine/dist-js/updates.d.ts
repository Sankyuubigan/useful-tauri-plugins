export interface PluginUpdateState {
    hasUpdate: boolean;
    tag?: string;
}
export declare function getUpdateState(): PluginUpdateState;
export declare function onUpdateState(fn: (s: PluginUpdateState) => void): () => void;
export declare function setUpdateState(next: PluginUpdateState): void;
export declare function checkUpdate(): Promise<PluginUpdateState>;
export declare function initUpdateWatcher(): void;
