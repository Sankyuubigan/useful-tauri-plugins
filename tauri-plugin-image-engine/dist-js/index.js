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
export function getImageBundleInfo(bundleName) {
    return invoke('plugin:image-engine|get_image_bundle_info', { bundleName: bundleName ?? null });
}
/** Выбрать активный бандл каталога (пустая строка => сброс на дефолт). */
export function setSelectedImageBundle(bundleName) {
    return invoke('plugin:image-engine|set_selected_image_bundle', { bundleName });
}
export function downloadImageBundle(saveDir, bundleName) {
    return invoke('plugin:image-engine|download_image_bundle', { saveDir, bundleName: bundleName ?? null });
}
export function validateImageBundleDir(path, bundleName) {
    return invoke('plugin:image-engine|validate_image_bundle_dir', { path, bundleName: bundleName ?? null });
}
export function setImageBundleDir(path) {
    return invoke('plugin:image-engine|set_image_bundle_dir', { path });
}
export function removeImageBundle(bundleName) {
    return invoke('plugin:image-engine|remove_image_bundle', { bundleName: bundleName ?? null });
}
export function removeImageBundleFromList() {
    return invoke('plugin:image-engine|remove_image_bundle_from_list');
}
/** Неиспользуемые файлы пула image-бандлов (остатки удалённых из каталога
 * бандлов + сторонний мусор). Только перечисление — удаление отдельной командой. */
export function getImagePoolOrphans() {
    return invoke('plugin:image-engine|get_image_pool_orphans');
}
/** Удалить перечисленные неиспользуемые файлы пула (явное действие юзера). */
export function removeImagePoolOrphans(filenames) {
    return invoke('plugin:image-engine|remove_image_pool_orphans', { filenames });
}
export function estimateImageMemory(bundleName) {
    return invoke('plugin:image-engine|estimate_image_memory', { bundleName: bundleName ?? null });
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
        bundleName: opts.bundleName ?? null,
        loraNames: opts.loraNames ?? null,
        loraStrength: opts.loraStrength ?? null,
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
        bundleName: opts.bundleName ?? null,
        loraNames: opts.loraNames ?? null,
        loraStrength: opts.loraStrength ?? null,
    });
}
export function getVideoBundles() {
    return invoke('plugin:image-engine|get_video_bundles');
}
export function getVideoBundleInfo(bundleName) {
    return invoke('plugin:image-engine|get_video_bundle_info', { bundleName: bundleName ?? null });
}
/** Выбрать активный видео-бандл (пустая строка => сброс на дефолт). */
export function setSelectedVideoBundle(bundleName) {
    return invoke('plugin:image-engine|set_selected_video_bundle', { bundleName });
}
export function downloadVideoBundle(saveDir, bundleName) {
    return invoke('plugin:image-engine|download_video_bundle', { saveDir, bundleName: bundleName ?? null });
}
export function validateVideoBundleDir(path, bundleName) {
    return invoke('plugin:image-engine|validate_video_bundle_dir', { path, bundleName: bundleName ?? null });
}
/** Custom-папка видео-бандла (пустая строка/null => сброс на дефолт). */
export function setVideoBundleDir(path) {
    return invoke('plugin:image-engine|set_video_bundle_dir', { path: path ?? null });
}
export function removeVideoBundleFiles(bundleName) {
    return invoke('plugin:image-engine|remove_video_bundle_files', { bundleName: bundleName ?? null });
}
export function generateVideo(opts) {
    return invoke('plugin:image-engine|generate_video', {
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
    });
}
/** Список файлов LoRA-каталога из HF-репозитория (подпапки = базы). */
export function listRemoteLoras() {
    return invoke('plugin:image-engine|list_remote_loras');
}
/** Скачать один файл каталога в единую папку loras (веса или sidecar). */
export function downloadRemoteLora(path) {
    return invoke('plugin:image-engine|download_remote_lora', { path });
}
// Регистрируем Web Components при импорте пакета.
import './web-components';
export { getUpdateState, onUpdateState, setUpdateState, checkUpdate, initUpdateWatcher, } from './updates';
