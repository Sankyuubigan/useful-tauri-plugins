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

export function getImageBundleInfo(): Promise<ImageBundleInfo> {
  return invoke<ImageBundleInfo>('plugin:image-engine|get_image_bundle_info')
}

export function downloadImageBundle(saveDir: string): Promise<ImageBundleInfo> {
  return invoke<ImageBundleInfo>('plugin:image-engine|download_image_bundle', { saveDir })
}

export interface ImageBundleValidate {
  valid: boolean
  dir: string
  present: string[]
  missing: string[]
}

export function validateImageBundleDir(path: string): Promise<ImageBundleValidate> {
  return invoke<ImageBundleValidate>('plugin:image-engine|validate_image_bundle_dir', { path })
}

export function setImageBundleDir(path: string): Promise<ImageBundleInfo> {
  return invoke<ImageBundleInfo>('plugin:image-engine|set_image_bundle_dir', { path })
}

export function removeImageBundle(): Promise<ImageBundleInfo> {
  return invoke<ImageBundleInfo>('plugin:image-engine|remove_image_bundle')
}

export function estimateImageMemory(): Promise<ImageMemoryInfo> {
  return invoke<ImageMemoryInfo>('plugin:image-engine|estimate_image_memory')
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
  })
}

// Регистрируем Web Components при импорте пакета.
import './web-components'
