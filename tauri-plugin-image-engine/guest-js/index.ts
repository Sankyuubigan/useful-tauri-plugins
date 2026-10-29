import { invoke } from '@tauri-apps/api/core'

// Изменение бандла/движка: диспатчится на document. Хост слушает и
// перечитывает свой конфиг.
export const BUNDLE_CHANGED_EVENT = 'image:bundle-changed'

export interface VariantInfo {
  id: string
  label: string
  note: string
  recommended: boolean
  installed: boolean
}

export interface ImageEngineStatus {
  installed: boolean
  tag?: string | null
  variant?: string | null
  path: string
  has_nvidia: boolean
  gpu_name: string
  required_variant: string
  selected_variant: string
  resolved_variant: string
  installed_variants: string[]
  available_variants: VariantInfo[]
  message: string
}

export interface BundleFileInfo {
  role: string
  filename: string
  size_bytes?: number | null
  save_path: string
  exists: boolean
}

export interface ImageBundleInfo {
  bundle_name: string
  label: string
  note: string
  files: BundleFileInfo[]
  total_bytes: number
  size_gb?: string | null
  vram_fast_gb?: number | null
  vram_min_gb?: number | null
  ram_min_gb?: number | null
  save_dir: string
  free_space_gb: number
  fully_downloaded: boolean
}

export interface MemoryEstimate {
  disk_mb: number
  full_mb: number
  min_mb: number
  ram_need_mb: number
  vram_free_mb: number
  vram_total_mb: number
  ram_free_mb: number
}

export type PreflightVerdict = 'fast' | 'offload' | 'insufficient'

export interface ImageMemoryInfo {
  estimate: MemoryEstimate
  verdict: PreflightVerdict
  message: string
}

export interface ImageGenResult {
  path: string
  seed: number
  time_sec: number
}

export interface ImagePreset {
  cfg_scale: number
  sampler: string
  width: number
  height: number
  steps: number
  seed: number
  /** Инвариант Turbo: len(custom_sigmas) === steps, последняя сигма 0.0. */
  custom_sigmas?: number[] | null
  scheduler?: string | null
  native_sigma_preset?: string | null
}

export interface BundleFile {
  role: string
  filename: string
  download_url: string
  size_bytes?: number | null
}

export interface ImageBundleEntry {
  name: string
  label: string
  note: string
  is_default: boolean
  files: BundleFile[]
  preset: ImagePreset
  size_gb?: string | null
  vram_fast_gb?: number | null
  vram_min_gb?: number | null
  ram_min_gb?: number | null
}

/** Уведомить хост (и другие компоненты плагина), что бандл изменился. */
export function notifyBundleChanged(): void {
  document.dispatchEvent(new CustomEvent(BUNDLE_CHANGED_EVENT))
}

// ─────────────────────────── Движок sd.cpp ───────────────────────────

export function getImageEngineStatus(): Promise<ImageEngineStatus> {
  return invoke<ImageEngineStatus>('plugin:image-engine|get_image_engine_status')
}

export function installImageEngine(): Promise<ImageEngineStatus> {
  return invoke<ImageEngineStatus>('plugin:image-engine|install_image_engine')
}

export function setImageEngineVariant(variant: string): Promise<ImageEngineStatus> {
  return invoke<ImageEngineStatus>('plugin:image-engine|set_image_engine_variant', { variant })
}

/** Новый тег релиза или null, если движок актуален. */
export function checkImageEngineUpdate(): Promise<string | null> {
  return invoke<string | null>('plugin:image-engine|check_image_engine_update')
}

export function installImageEngineUpdate(): Promise<ImageEngineStatus> {
  return invoke<ImageEngineStatus>('plugin:image-engine|install_image_engine_update')
}

export function removeImageEngine(): Promise<ImageEngineStatus> {
  return invoke<ImageEngineStatus>('plugin:image-engine|remove_image_engine')
}

export function setImageEngineDir(path: string): Promise<ImageEngineStatus> {
  return invoke<ImageEngineStatus>('plugin:image-engine|set_image_engine_dir', { path })
}

// ─────────────────────────── Каталог бандлов ───────────────────────────

export function getImageModelsCatalog(): Promise<ImageBundleEntry[]> {
  return invoke<ImageBundleEntry[]>('plugin:image-engine|get_image_models_catalog')
}

export function getImageBundleInfo(bundleName?: string | null): Promise<ImageBundleInfo> {
  return invoke<ImageBundleInfo>('plugin:image-engine|get_image_bundle_info', { bundleName: bundleName ?? null })
}

/** Выбрать активный бандл каталога (пустая строка => сброс на дефолт). */
export function setSelectedImageBundle(bundleName: string): Promise<ImageBundleInfo> {
  return invoke<ImageBundleInfo>('plugin:image-engine|set_selected_image_bundle', { bundleName })
}

export function downloadImageBundle(saveDir: string, bundleName?: string | null): Promise<ImageBundleInfo> {
  return invoke<ImageBundleInfo>('plugin:image-engine|download_image_bundle', { saveDir, bundleName: bundleName ?? null })
}

export interface ImageBundleValidate {
  valid: boolean
  dir: string
  present: string[]
  missing: string[]
}

export function validateImageBundleDir(path: string, bundleName?: string | null): Promise<ImageBundleValidate> {
  return invoke<ImageBundleValidate>('plugin:image-engine|validate_image_bundle_dir', { path, bundleName: bundleName ?? null })
}

export function setImageBundleDir(path: string): Promise<ImageBundleInfo> {
  return invoke<ImageBundleInfo>('plugin:image-engine|set_image_bundle_dir', { path })
}

export function removeImageBundle(bundleName?: string | null): Promise<ImageBundleInfo> {
  return invoke<ImageBundleInfo>('plugin:image-engine|remove_image_bundle', { bundleName: bundleName ?? null })
}

export function removeImageBundleFromList(): Promise<ImageBundleInfo> {
  return invoke<ImageBundleInfo>('plugin:image-engine|remove_image_bundle_from_list')
}

// ─────────────────────────── Пул: неиспользуемые файлы ───────────────────────────

/** Файл пула, не нужный ни одному бандлу каталога. */
export interface PoolOrphan {
  filename: string
  size_bytes: number
}

export interface PoolOrphansInfo {
  dir: string
  orphans: PoolOrphan[]
  total_bytes: number
}

/** Неиспользуемые файлы пула image-бандлов (остатки удалённых из каталога
 * бандлов + сторонний мусор). Только перечисление — удаление отдельной командой. */
export function getImagePoolOrphans(): Promise<PoolOrphansInfo> {
  return invoke<PoolOrphansInfo>('plugin:image-engine|get_image_pool_orphans')
}

/** Удалить перечисленные неиспользуемые файлы пула (явное действие юзера). */
export function removeImagePoolOrphans(filenames: string[]): Promise<PoolOrphansInfo> {
  return invoke<PoolOrphansInfo>('plugin:image-engine|remove_image_pool_orphans', { filenames })
}

export function estimateImageMemory(bundleName?: string | null): Promise<ImageMemoryInfo> {
  return invoke<ImageMemoryInfo>('plugin:image-engine|estimate_image_memory', { bundleName: bundleName ?? null })
}

// ─────────────────────────── Генерация ───────────────────────────

export interface GenerateOptions {
  prompt: string
  width?: number | null
  height?: number | null
  steps?: number | null
  cfgScale?: number | null
  seed?: number | null
  outPath?: string | null
  bundleName?: string | null
  /** Имена файлов LoRA (стем без расширения) из папки LoRA настроек. */
  loraNames?: string[] | null
  /** Общая сила LoRA (0–2, по умолчанию 1.0): действует на все имена из loraNames. */
  loraStrength?: number | null
}

export function generateImage(opts: GenerateOptions): Promise<ImageGenResult> {
  return invoke<ImageGenResult>('plugin:image-engine|generate_image', {
    prompt: opts.prompt,
    width: opts.width ?? null,
    height: opts.height ?? null,
    steps: opts.steps ?? null,
    cfgScale: opts.cfgScale ?? null,
    seed: opts.seed ?? null,
    outPath: opts.outPath ?? null,
    bundleName: opts.bundleName ?? null,
    loraNames: opts.loraNames ?? null,
    loraStrength: opts.loraStrength ?? null,
  })
}

export interface EditOptions extends GenerateOptions {
  refPaths: string[]
}

export function editImage(opts: EditOptions): Promise<ImageGenResult> {
  return invoke<ImageGenResult>('plugin:image-engine|edit_image', {
    prompt: opts.prompt,
    refPaths: opts.refPaths,
    width: opts.width ?? null,
    height: opts.height ?? null,
    steps: opts.steps ?? null,
    cfgScale: opts.cfgScale ?? null,
    seed: opts.seed ?? null,
    outPath: opts.outPath ?? null,
    bundleName: opts.bundleName ?? null,
    loraNames: opts.loraNames ?? null,
    loraStrength: opts.loraStrength ?? null,
  })
}

// ─────────────────────────── Видео (MiniMax H3, PRP 21.10.26) ───────────────────────────

export interface VideoFileEntry {
  role: string
  filename: string
  download_url: string
  size_bytes?: number | null
}

export interface VideoPreset {
  steps_draft: number
  steps_draft_nolora: number
  steps_final: number
  fps: number
  max_seconds: number
  cfg_scale: number
  sampler: string
  seed: number
  width: number
  height: number
}

export interface VideoBundleEntry {
  name: string
  label: string
  note: string
  is_default: boolean
  files: VideoFileEntry[]
  spawn_flags?: string | null
  model_args?: string | null
  preset: VideoPreset
  size_gb?: string | null
  vram_fast_gb?: number | null
  vram_min_gb?: number | null
  ram_min_gb?: number | null
}

export function getVideoBundles(): Promise<VideoBundleEntry[]> {
  return invoke<VideoBundleEntry[]>('plugin:image-engine|get_video_bundles')
}

export function getVideoBundleInfo(bundleName?: string | null): Promise<ImageBundleInfo> {
  return invoke<ImageBundleInfo>('plugin:image-engine|get_video_bundle_info', { bundleName: bundleName ?? null })
}

/** Выбрать активный видео-бандл (пустая строка => сброс на дефолт). */
export function setSelectedVideoBundle(bundleName: string): Promise<ImageBundleInfo> {
  return invoke<ImageBundleInfo>('plugin:image-engine|set_selected_video_bundle', { bundleName })
}

export function downloadVideoBundle(saveDir: string, bundleName?: string | null): Promise<ImageBundleInfo> {
  return invoke<ImageBundleInfo>('plugin:image-engine|download_video_bundle', { saveDir, bundleName: bundleName ?? null })
}

export function validateVideoBundleDir(path: string, bundleName?: string | null): Promise<ImageBundleValidate> {
  return invoke<ImageBundleValidate>('plugin:image-engine|validate_video_bundle_dir', { path, bundleName: bundleName ?? null })
}

/** Custom-папка видео-бандла (пустая строка/null => сброс на дефолт). */
export function setVideoBundleDir(path?: string | null): Promise<ImageBundleInfo> {
  return invoke<ImageBundleInfo>('plugin:image-engine|set_video_bundle_dir', { path: path ?? null })
}

export function removeVideoBundleFiles(bundleName?: string | null): Promise<ImageBundleInfo> {
  return invoke<ImageBundleInfo>('plugin:image-engine|remove_video_bundle_files', { bundleName: bundleName ?? null })
}

export interface VideoGenResult {
  path: string
  fps: number
  frame_count: number
  mime_type: string
  time_sec: number
}

export interface GenerateVideoOptions {
  prompt: string
  negativePrompt?: string | null
  /** Первый кадр (i2va/fl2va): локальный путь или data-URL. */
  initPath?: string | null
  /** Последний кадр (fl2va): локальный путь или data-URL. */
  endPath?: string | null
  /** Референсы (ref2va): локальные пути или data-URL. НЕ смешивать с init/end. */
  refPaths?: string[]
  /** Длительность клипа, сек (frames = seconds × fps; сервер нормализует сеткой). */
  seconds?: number | null
  width?: number | null
  height?: number | null
  /** Явные шаги — приоритетнее finalQuality. */
  steps?: number | null
  cfgScale?: number | null
  seed?: number | null
  /** false/по умолчанию = draft; true = steps_final. */
  finalQuality?: boolean | null
  /** image-to-video сила; по умолчанию 0.75. */
  strength?: number | null
  vaeTiling?: boolean | null
  outPath?: string | null
  bundleName?: string | null
}

export function generateVideo(opts: GenerateVideoOptions): Promise<VideoGenResult> {
  return invoke<VideoGenResult>('plugin:image-engine|generate_video', {
    prompt: opts.prompt,
    negativePrompt: opts.negativePrompt ?? null,
    initPath: opts.initPath ?? null,
    endPath: opts.endPath ?? null,
    refPaths: opts.refPaths ?? [],
    seconds: opts.seconds ?? null,
    width: opts.width ?? null,
    height: opts.height ?? null,
    steps: opts.steps ?? null,
    cfgScale: opts.cfgScale ?? null,
    seed: opts.seed ?? null,
    finalQuality: opts.finalQuality ?? null,
    strength: opts.strength ?? null,
    vaeTiling: opts.vaeTiling ?? null,
    outPath: opts.outPath ?? null,
    bundleName: opts.bundleName ?? null,
  })
}

// Регистрируем Web Components при импорте пакета.
import './web-components'

export {
  getUpdateState,
  onUpdateState,
  setUpdateState,
  checkUpdate,
  initUpdateWatcher,
} from './updates'
export type { PluginUpdateState } from './updates'
