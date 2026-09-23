export declare const MODELS_CHANGED_EVENT = "llama:models-changed";
export interface ModelMeta {
    uncen?: boolean;
    vision?: boolean;
    audio?: boolean;
}
export interface ModelParams {
    temperature: number;
    top_k: number;
    top_p: number;
    min_p: number;
    repetition_penalty: number;
    presence_penalty: number;
    dry_multiplier: number;
    dry_base: number;
    dry_allowed_length: number;
    dry_penalty_last_n: number;
    xtc_probability: number;
    xtc_threshold: number;
}
export interface EngineConfig {
    models: string[];
    last_model?: string | null;
    models_dir?: string | null;
    model_params: Record<string, ModelParams>;
    mmproj_files: Record<string, string>;
    model_meta: Record<string, ModelMeta>;
    llamacpp_dir?: string | null;
    engine_source?: string | null;
    engine_variant?: string | null;
}
export interface CatalogEntry {
    name: string;
    download_url: string;
    size_gb?: string | null;
    tokenizer_id?: string | null;
    is_default?: boolean;
    mmproj_url?: string | null;
    uncen?: boolean | null;
    vision?: boolean | null;
    audio?: boolean | null;
}
export interface VariantInfo {
    id: string;
    label: string;
    note: string;
    recommended: boolean;
    installed: boolean;
}
export interface SourceInfo {
    id: string;
    label: string;
    note: string;
    installed: boolean;
    is_default: boolean;
}
export interface EngineStatus {
    installed: boolean;
    tag?: string | null;
    cuda?: string | null;
    path: string;
    has_nvidia: boolean;
    requires_driver_update: boolean;
    cuda_major: number;
    cuda_minor: number;
    gpu_name: string;
    compute_cap: string;
    required_variant: string;
    selected_variant: string;
    resolved_variant: string;
    selected_source: string;
    available_sources: SourceInfo[];
    installed_variants: string[];
    available_variants: VariantInfo[];
    message: string;
}
export interface ModelCapabilities {
    vision: boolean;
    audio: boolean;
    uncen: boolean;
}
export interface AutoDownloadInfo {
    model_name: string;
    model_url: string;
    size_gb?: string | null;
    save_path: string;
    free_space_gb: number;
    drive_letter: string;
}
export interface AddModelOutcome {
    config: EngineConfig;
    warning?: string | null;
}
/** Уведомить хост (и другие компоненты плагина), что список моделей изменился. */
export declare function notifyModelsChanged(): void;
export declare function getEngineStatus(): Promise<EngineStatus>;
export declare function installLlamaCpp(): Promise<EngineStatus>;
export declare function setEngineVariant(variant: string): Promise<EngineStatus>;
export declare function listEngineSources(): Promise<SourceInfo[]>;
export declare function setEngineSource(source: string): Promise<EngineStatus>;
/** Новый тег релиза или null, если движок актуален. */
export declare function checkEngineUpdate(): Promise<string | null>;
export declare function installEngineUpdate(): Promise<EngineStatus>;
export declare function removeEngine(): Promise<EngineStatus>;
export declare function setEngineDir(path: string): Promise<EngineStatus>;
export declare function getModelsCatalog(): Promise<CatalogEntry[]>;
export declare function getEngineConfig(): Promise<EngineConfig>;
export declare function getAutoDownloadInfo(): Promise<AutoDownloadInfo>;
export declare function autoDownloadDefaultModel(savePath: string): Promise<void>;
export declare function addModel(path: string, flags?: ModelMeta | null): Promise<AddModelOutcome>;
export declare function removeModel(path: string): Promise<EngineConfig>;
export declare function deleteModelFile(path: string): Promise<EngineConfig>;
/** Скачивание .gguf/mmproj. Прогресс — событие `download_progress` (Tauri). */
export declare function downloadModel(url: string, savePath: string): Promise<void>;
export declare function getMmprojPath(modelPath: string): Promise<string | null>;
export declare function ensureMmproj(modelPath: string): Promise<string | null>;
export declare function getModelCapabilities(modelPath: string): Promise<ModelCapabilities>;
export declare function getAllCapabilities(): Promise<Record<string, ModelCapabilities>>;
export interface PromptMemoryInfo {
    need_mb: number;
    vram_used_mb: number;
    vram_total_mb: number;
}
/** Live-превью: прогноз VRAM (модель + KV-кэш) на эффективный контекст + факты NVML. */
export declare function estimatePromptMemory(modelPath: string, contextSize: number, kvQuantKeys: boolean, kvQuantValues: boolean, promptTokens: number, maxGen: number): Promise<PromptMemoryInfo>;
export declare function getModelParams(modelPath: string): Promise<ModelParams>;
export declare function setModelParams(modelPath: string, params: ModelParams): Promise<void>;
export declare function resetModelParams(modelPath: string): Promise<ModelParams>;
import './web-components';
