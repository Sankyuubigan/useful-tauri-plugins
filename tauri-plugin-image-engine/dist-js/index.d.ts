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
    /** Инвариант Turbo: len(custom_sigmas) === steps, последняя сигма 0.0. */
    custom_sigmas?: number[] | null;
    scheduler?: string | null;
    native_sigma_preset?: string | null;
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
export declare function getImageBundleInfo(bundleName?: string | null): Promise<ImageBundleInfo>;
/** Выбрать активный бандл каталога (пустая строка => сброс на дефолт). */
export declare function setSelectedImageBundle(bundleName: string): Promise<ImageBundleInfo>;
export declare function downloadImageBundle(saveDir: string, bundleName?: string | null): Promise<ImageBundleInfo>;
export interface ImageBundleValidate {
    valid: boolean;
    dir: string;
    present: string[];
    missing: string[];
}
export declare function validateImageBundleDir(path: string, bundleName?: string | null): Promise<ImageBundleValidate>;
export declare function setImageBundleDir(path: string): Promise<ImageBundleInfo>;
export declare function removeImageBundle(bundleName?: string | null): Promise<ImageBundleInfo>;
export declare function removeImageBundleFromList(): Promise<ImageBundleInfo>;
export declare function estimateImageMemory(bundleName?: string | null): Promise<ImageMemoryInfo>;
export interface GenerateOptions {
    prompt: string;
    width?: number | null;
    height?: number | null;
    steps?: number | null;
    cfgScale?: number | null;
    seed?: number | null;
    outPath?: string | null;
    bundleName?: string | null;
    /** Имена файлов LoRA (стем без расширения) из папки LoRA настроек. */
    loraNames?: string[] | null;
    /** Общая сила LoRA (0–2, по умолчанию 1.0): действует на все имена из loraNames. */
    loraStrength?: number | null;
}
export declare function generateImage(opts: GenerateOptions): Promise<ImageGenResult>;
export interface EditOptions extends GenerateOptions {
    refPaths: string[];
}
export declare function editImage(opts: EditOptions): Promise<ImageGenResult>;
export interface VideoFileEntry {
    role: string;
    filename: string;
    download_url: string;
    size_bytes?: number | null;
}
export interface VideoPreset {
    steps_draft: number;
    steps_draft_nolora: number;
    steps_final: number;
    fps: number;
    max_seconds: number;
    cfg_scale: number;
    sampler: string;
    seed: number;
    width: number;
    height: number;
}
export interface VideoBundleEntry {
    name: string;
    label: string;
    note: string;
    is_default: boolean;
    files: VideoFileEntry[];
    spawn_flags?: string | null;
    model_args?: string | null;
    preset: VideoPreset;
    size_gb?: string | null;
    vram_fast_gb?: number | null;
    vram_min_gb?: number | null;
    ram_min_gb?: number | null;
}
export declare function getVideoBundles(): Promise<VideoBundleEntry[]>;
export declare function getVideoBundleInfo(bundleName?: string | null): Promise<ImageBundleInfo>;
/** Выбрать активный видео-бандл (пустая строка => сброс на дефолт). */
export declare function setSelectedVideoBundle(bundleName: string): Promise<ImageBundleInfo>;
export declare function downloadVideoBundle(saveDir: string, bundleName?: string | null): Promise<ImageBundleInfo>;
export declare function validateVideoBundleDir(path: string, bundleName?: string | null): Promise<ImageBundleValidate>;
/** Custom-папка видео-бандла (пустая строка/null => сброс на дефолт). */
export declare function setVideoBundleDir(path?: string | null): Promise<ImageBundleInfo>;
export declare function removeVideoBundleFiles(bundleName?: string | null): Promise<ImageBundleInfo>;
export interface VideoGenResult {
    path: string;
    fps: number;
    frame_count: number;
    mime_type: string;
    time_sec: number;
}
export interface GenerateVideoOptions {
    prompt: string;
    negativePrompt?: string | null;
    /** Первый кадр (i2va/fl2va): локальный путь или data-URL. */
    initPath?: string | null;
    /** Последний кадр (fl2va): локальный путь или data-URL. */
    endPath?: string | null;
    /** Референсы (ref2va): локальные пути или data-URL. НЕ смешивать с init/end. */
    refPaths?: string[];
    /** Длительность клипа, сек (frames = seconds × fps; сервер нормализует сеткой). */
    seconds?: number | null;
    width?: number | null;
    height?: number | null;
    /** Явные шаги — приоритетнее finalQuality. */
    steps?: number | null;
    cfgScale?: number | null;
    seed?: number | null;
    /** false/по умолчанию = draft; true = steps_final. */
    finalQuality?: boolean | null;
    /** image-to-video сила; по умолчанию 0.75. */
    strength?: number | null;
    vaeTiling?: boolean | null;
    outPath?: string | null;
    bundleName?: string | null;
}
export declare function generateVideo(opts: GenerateVideoOptions): Promise<VideoGenResult>;
import './web-components';
export { getUpdateState, onUpdateState, setUpdateState, checkUpdate, initUpdateWatcher, } from './updates';
export type { PluginUpdateState } from './updates';
