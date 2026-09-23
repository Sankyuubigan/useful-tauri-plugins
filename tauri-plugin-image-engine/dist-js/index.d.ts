export declare const BUNDLE_CHANGED_EVENT = "image:bundle-changed";
export interface VariantInfo {
    id: string;
    label: string;
    note: string;
    recommended: boolean;
    installed: boolean;
}
export interface ImageEngineStatus {
    installed: boolean;
    tag?: string | null;
    variant?: string | null;
    path: string;
    has_nvidia: boolean;
    gpu_name: string;
    required_variant: string;
    selected_variant: string;
    resolved_variant: string;
    installed_variants: string[];
    available_variants: VariantInfo[];
    message: string;
}
export interface BundleFileInfo {
    role: string;
    filename: string;
    size_bytes?: number | null;
    save_path: string;
    exists: boolean;
}
export interface ImageBundleInfo {
    bundle_name: string;
    label: string;
    note: string;
    files: BundleFileInfo[];
    total_bytes: number;
    size_gb?: string | null;
    vram_fast_gb?: number | null;
    vram_min_gb?: number | null;
    ram_min_gb?: number | null;
    save_dir: string;
    free_space_gb: number;
    fully_downloaded: boolean;
}
export interface MemoryEstimate {
    disk_mb: number;
    full_mb: number;
    min_mb: number;
    ram_need_mb: number;
    vram_free_mb: number;
    vram_total_mb: number;
    ram_free_mb: number;
}
export type PreflightVerdict = 'fast' | 'offload' | 'insufficient';
export interface ImageMemoryInfo {
    estimate: MemoryEstimate;
    verdict: PreflightVerdict;
    message: string;
}
export interface ImageGenResult {
    path: string;
    seed: number;
    time_sec: number;
}
export interface ImagePreset {
    cfg_scale: number;
    sampler: string;
    width: number;
    height: number;
    steps: number;
    seed: number;
}
export interface BundleFile {
    role: string;
    filename: string;
    download_url: string;
    size_bytes?: number | null;
}
export interface ImageBundleEntry {
    name: string;
    label: string;
    note: string;
    is_default: boolean;
    files: BundleFile[];
    preset: ImagePreset;
    size_gb?: string | null;
    vram_fast_gb?: number | null;
    vram_min_gb?: number | null;
    ram_min_gb?: number | null;
}
/** Уведомить хост (и другие компоненты плагина), что бандл изменился. */
export declare function notifyBundleChanged(): void;
export declare function getImageEngineStatus(): Promise<ImageEngineStatus>;
export declare function installImageEngine(): Promise<ImageEngineStatus>;
export declare function setImageEngineVariant(variant: string): Promise<ImageEngineStatus>;
/** Новый тег релиза или null, если движок актуален. */
export declare function checkImageEngineUpdate(): Promise<string | null>;
export declare function installImageEngineUpdate(): Promise<ImageEngineStatus>;
export declare function removeImageEngine(): Promise<ImageEngineStatus>;
export declare function setImageEngineDir(path: string): Promise<ImageEngineStatus>;
export declare function getImageModelsCatalog(): Promise<ImageBundleEntry[]>;
export declare function getImageBundleInfo(): Promise<ImageBundleInfo>;
export declare function downloadImageBundle(saveDir: string): Promise<ImageBundleInfo>;
export interface ImageBundleValidate {
    valid: boolean;
    dir: string;
    present: string[];
    missing: string[];
}
export declare function validateImageBundleDir(path: string): Promise<ImageBundleValidate>;
export declare function setImageBundleDir(path: string): Promise<ImageBundleInfo>;
export declare function removeImageBundle(): Promise<ImageBundleInfo>;
export declare function estimateImageMemory(): Promise<ImageMemoryInfo>;
export interface GenerateOptions {
    prompt: string;
    width?: number | null;
    height?: number | null;
    steps?: number | null;
    cfgScale?: number | null;
    seed?: number | null;
    outPath?: string | null;
}
export declare function generateImage(opts: GenerateOptions): Promise<ImageGenResult>;
export interface EditOptions extends GenerateOptions {
    refPaths: string[];
}
export declare function editImage(opts: EditOptions): Promise<ImageGenResult>;
import './web-components';
