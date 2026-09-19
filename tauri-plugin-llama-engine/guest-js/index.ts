import { invoke } from '@tauri-apps/api/core'

// Изменение списка моделей/движка: диспатчится на document. Хост слушает и
// перечитывает свой get_config (см. King Orch src/main.ts).
export const MODELS_CHANGED_EVENT = 'llama:models-changed'

export interface ModelMeta {
  uncen?: boolean
  vision?: boolean
  audio?: boolean
}

export interface ModelParams {
  temperature: number
  top_k: number
  top_p: number
  min_p: number
  repetition_penalty: number
  presence_penalty: number
  dry_multiplier: number
  dry_base: number
  dry_allowed_length: number
  dry_penalty_last_n: number
  xtc_probability: number
  xtc_threshold: number
}

export interface EngineConfig {
  models: string[]
  last_model?: string | null
  models_dir?: string | null
  model_params: Record<string, ModelParams>
  mmproj_files: Record<string, string>
  model_meta: Record<string, ModelMeta>
  llamacpp_dir?: string | null
  engine_variant?: string | null
}

export interface CatalogEntry {
  name: string
  download_url: string
  size_gb?: string | null
  tokenizer_id?: string | null
  is_default?: boolean
  mmproj_url?: string | null
  uncen?: boolean | null
  vision?: boolean | null
  audio?: boolean | null
}

export interface VariantInfo {
  id: string
  label: string
  note: string
  recommended: boolean
  installed: boolean
}

export interface EngineStatus {
  installed: boolean
  tag?: string | null
  cuda?: string | null
  path: string
  has_nvidia: boolean
  requires_driver_update: boolean
  cuda_major: number
  cuda_minor: number
  gpu_name: string
  compute_cap: string
  required_variant: string
  selected_variant: string
  resolved_variant: string
  installed_variants: string[]
  available_variants: VariantInfo[]
  message: string
}

export interface ModelCapabilities {
  vision: boolean
  audio: boolean
  uncen: boolean
}

export interface AutoDownloadInfo {
  model_name: string
  model_url: string
  size_gb?: string | null
  save_path: string
  free_space_gb: number
  drive_letter: string
}

export interface AddModelOutcome {
  config: EngineConfig
  warning?: string | null
}

/** Уведомить хост (и другие компоненты плагина), что список моделей изменился. */
export function notifyModelsChanged(): void {
  document.dispatchEvent(new CustomEvent(MODELS_CHANGED_EVENT))
}

// ─────────────────────────── Движок llama.cpp ───────────────────────────

export function getEngineStatus(): Promise<EngineStatus> {
  return invoke<EngineStatus>('plugin:llama-engine|get_engine_status')
}

export function installLlamaCpp(): Promise<EngineStatus> {
  return invoke<EngineStatus>('plugin:llama-engine|install_llamacpp')
}

export function setEngineVariant(variant: string): Promise<EngineStatus> {
  return invoke<EngineStatus>('plugin:llama-engine|set_engine_variant', { variant })
}

/** Новый тег релиза или null, если движок актуален. */
export function checkEngineUpdate(): Promise<string | null> {
  return invoke<string | null>('plugin:llama-engine|check_engine_update')
}

export function installEngineUpdate(): Promise<EngineStatus> {
  return invoke<EngineStatus>('plugin:llama-engine|install_engine_update')
}

export function removeEngine(): Promise<EngineStatus> {
  return invoke<EngineStatus>('plugin:llama-engine|remove_engine')
}

export function setEngineDir(path: string): Promise<EngineStatus> {
  return invoke<EngineStatus>('plugin:llama-engine|set_engine_dir', { path })
}

// ─────────────────────────── Каталог и модели ───────────────────────────

export function getModelsCatalog(): Promise<CatalogEntry[]> {
  return invoke<CatalogEntry[]>('plugin:llama-engine|get_models_catalog')
}

export function getEngineConfig(): Promise<EngineConfig> {
  return invoke<EngineConfig>('plugin:llama-engine|get_engine_config')
}

export function getAutoDownloadInfo(): Promise<AutoDownloadInfo> {
  return invoke<AutoDownloadInfo>('plugin:llama-engine|get_auto_download_info')
}

export function autoDownloadDefaultModel(savePath: string): Promise<void> {
  return invoke<void>('plugin:llama-engine|auto_download_default_model', { savePath })
}

export function addModel(path: string, flags?: ModelMeta | null): Promise<AddModelOutcome> {
  return invoke<AddModelOutcome>('plugin:llama-engine|add_model', { path, flags })
}

export function removeModel(path: string): Promise<EngineConfig> {
  return invoke<EngineConfig>('plugin:llama-engine|remove_model', { path })
}

export function deleteModelFile(path: string): Promise<EngineConfig> {
  return invoke<EngineConfig>('plugin:llama-engine|delete_model_file', { path })
}

/** Скачивание .gguf/mmproj. Прогресс — событие `download_progress` (Tauri). */
export function downloadModel(url: string, savePath: string): Promise<void> {
  return invoke<void>('plugin:llama-engine|download_model', { url, savePath })
}

export function getMmprojPath(modelPath: string): Promise<string | null> {
  return invoke<string | null>('plugin:llama-engine|get_mmproj_path', { modelPath })
}

export function ensureMmproj(modelPath: string): Promise<string | null> {
  return invoke<string | null>('plugin:llama-engine|ensure_mmproj', { modelPath })
}

export function getModelCapabilities(modelPath: string): Promise<ModelCapabilities> {
  return invoke<ModelCapabilities>('plugin:llama-engine|get_model_capabilities', { modelPath })
}

export function getAllCapabilities(): Promise<Record<string, ModelCapabilities>> {
  return invoke<Record<string, ModelCapabilities>>('plugin:llama-engine|get_all_capabilities')
}

export interface PromptMemoryInfo {
  need_mb: number
  vram_used_mb: number
  vram_total_mb: number
}

/** Live-превью: прогноз VRAM (модель + KV-кэш) на эффективный контекст + факты NVML. */
export function estimatePromptMemory(
  modelPath: string,
  contextSize: number,
  kvQuantKeys: boolean,
  kvQuantValues: boolean,
  promptTokens: number,
  maxGen: number,
): Promise<PromptMemoryInfo> {
  return invoke<PromptMemoryInfo>('plugin:llama-engine|estimate_prompt_memory', {
    modelPath,
    contextSize,
    kvQuantKeys,
    kvQuantValues,
    promptTokens,
    maxGen,
  })
}

// ─────────────────────────── Параметры сэмплинга ───────────────────────────

export function getModelParams(modelPath: string): Promise<ModelParams> {
  return invoke<ModelParams>('plugin:llama-engine|get_model_params', { modelPath })
}

export function setModelParams(modelPath: string, params: ModelParams): Promise<void> {
  return invoke<void>('plugin:llama-engine|set_model_params', { modelPath, params })
}

export function resetModelParams(modelPath: string): Promise<ModelParams> {
  return invoke<ModelParams>('plugin:llama-engine|reset_model_params', { modelPath })
}

// Регистрируем Web Components при импорте пакета.
import './web-components'