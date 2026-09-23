import { invoke } from '@tauri-apps/api/core';
// Изменение бандла/движка: диспатчится на document. Хост слушает и
// перечитывает свой конфиг.
export const BUNDLE_CHANGED_EVENT = 'image:bundle-changed';
/** Уведомить хост (и другие компоненты плагина), что бандл изменился. */
export function notifyBundleChanged() {
    document.dispatchEvent(new CustomEvent(BUNDLE_CHANGED_EVENT));
}
// ─────────────────────────── Движок sd.cpp ───────────────────────────
export function getImageEngineStatus() {
    return invoke('plugin:image-engine|get_image_engine_status');
}
export function installImageEngine() {
    return invoke('plugin:image-engine|install_image_engine');
}
export function setImageEngineVariant(variant) {
    return invoke('plugin:image-engine|set_image_engine_variant', { variant });
}
/** Новый тег релиза или null, если движок актуален. */
export function checkImageEngineUpdate() {
    return invoke('plugin:image-engine|check_image_engine_update');
}
export function installImageEngineUpdate() {
    return invoke('plugin:image-engine|install_image_engine_update');
}
export function removeImageEngine() {
    return invoke('plugin:image-engine|remove_image_engine');
}
export function setImageEngineDir(path) {
    return invoke('plugin:image-engine|set_image_engine_dir', { path });
}
// ─────────────────────────── Каталог бандлов ───────────────────────────
export function getImageModelsCatalog() {
    return invoke('plugin:image-engine|get_image_models_catalog');
}
export function getImageBundleInfo() {
    return invoke('plugin:image-engine|get_image_bundle_info');
}
export function downloadImageBundle(saveDir) {
    return invoke('plugin:image-engine|download_image_bundle', { saveDir });
}
export function validateImageBundleDir(path) {
    return invoke('plugin:image-engine|validate_image_bundle_dir', { path });
}
export function setImageBundleDir(path) {
    return invoke('plugin:image-engine|set_image_bundle_dir', { path });
}
export function removeImageBundle() {
    return invoke('plugin:image-engine|remove_image_bundle');
}
export function estimateImageMemory() {
    return invoke('plugin:image-engine|estimate_image_memory');
}
export function generateImage(opts) {
    return invoke('plugin:image-engine|generate_image', {
        prompt: opts.prompt,
        width: opts.width ?? null,
        height: opts.height ?? null,
        steps: opts.steps ?? null,
        cfgScale: opts.cfgScale ?? null,
        seed: opts.seed ?? null,
        outPath: opts.outPath ?? null,
    });
}
export function editImage(opts) {
    return invoke('plugin:image-engine|edit_image', {
        prompt: opts.prompt,
        refPaths: opts.refPaths,
        width: opts.width ?? null,
        height: opts.height ?? null,
        steps: opts.steps ?? null,
        cfgScale: opts.cfgScale ?? null,
        seed: opts.seed ?? null,
        outPath: opts.outPath ?? null,
    });
}
// Регистрируем Web Components при импорте пакета.
import './web-components';
