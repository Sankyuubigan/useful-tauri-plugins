import { invoke } from '@tauri-apps/api/core';
// Изменение списка моделей/движка: диспатчится на document. Хост слушает и
// перечитывает свой get_config (см. King Orch src/main.ts).
export const MODELS_CHANGED_EVENT = 'llama:models-changed';
/** Уведомить хост (и другие компоненты плагина), что список моделей изменился. */
export function notifyModelsChanged() {
    document.dispatchEvent(new CustomEvent(MODELS_CHANGED_EVENT));
}
// ─────────────────────────── Движок llama.cpp ───────────────────────────
export function getEngineStatus() {
    return invoke('plugin:llama-engine|get_engine_status');
}
export function installLlamaCpp() {
    return invoke('plugin:llama-engine|install_llamacpp');
}
export function setEngineVariant(variant) {
    return invoke('plugin:llama-engine|set_engine_variant', { variant });
}
/** Новый тег релиза или null, если движок актуален. */
export function checkEngineUpdate() {
    return invoke('plugin:llama-engine|check_engine_update');
}
export function installEngineUpdate() {
    return invoke('plugin:llama-engine|install_engine_update');
}
export function removeEngine() {
    return invoke('plugin:llama-engine|remove_engine');
}
export function setEngineDir(path) {
    return invoke('plugin:llama-engine|set_engine_dir', { path });
}
// ─────────────────────────── Каталог и модели ───────────────────────────
export function getModelsCatalog() {
    return invoke('plugin:llama-engine|get_models_catalog');
}
export function getEngineConfig() {
    return invoke('plugin:llama-engine|get_engine_config');
}
export function getAutoDownloadInfo() {
    return invoke('plugin:llama-engine|get_auto_download_info');
}
export function autoDownloadDefaultModel(savePath) {
    return invoke('plugin:llama-engine|auto_download_default_model', { savePath });
}
export function addModel(path, flags) {
    return invoke('plugin:llama-engine|add_model', { path, flags });
}
export function removeModel(path) {
    return invoke('plugin:llama-engine|remove_model', { path });
}
export function deleteModelFile(path) {
    return invoke('plugin:llama-engine|delete_model_file', { path });
}
/** Скачивание .gguf/mmproj. Прогресс — событие `download_progress` (Tauri). */
export function downloadModel(url, savePath) {
    return invoke('plugin:llama-engine|download_model', { url, savePath });
}
export function getMmprojPath(modelPath) {
    return invoke('plugin:llama-engine|get_mmproj_path', { modelPath });
}
export function ensureMmproj(modelPath) {
    return invoke('plugin:llama-engine|ensure_mmproj', { modelPath });
}
export function getModelCapabilities(modelPath) {
    return invoke('plugin:llama-engine|get_model_capabilities', { modelPath });
}
export function getAllCapabilities() {
    return invoke('plugin:llama-engine|get_all_capabilities');
}
/** Live-превью: прогноз VRAM (модель + KV-кэш) на эффективный контекст + факты NVML. */
export function estimatePromptMemory(modelPath, contextSize, kvQuantKeys, kvQuantValues, promptTokens, maxGen) {
    return invoke('plugin:llama-engine|estimate_prompt_memory', {
        modelPath,
        contextSize,
        kvQuantKeys,
        kvQuantValues,
        promptTokens,
        maxGen,
    });
}
// ─────────────────────────── Параметры сэмплинга ───────────────────────────
export function getModelParams(modelPath) {
    return invoke('plugin:llama-engine|get_model_params', { modelPath });
}
export function setModelParams(modelPath, params) {
    return invoke('plugin:llama-engine|set_model_params', { modelPath, params });
}
export function resetModelParams(modelPath) {
    return invoke('plugin:llama-engine|reset_model_params', { modelPath });
}
// Регистрируем Web Components при импорте пакета.
import './web-components';
