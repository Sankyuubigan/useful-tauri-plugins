/**
 * Адаптеры BundleService для вкладок «Картинки» / «Видео».
 *
 * Одна вкладка (image-bundle-tab) работает с обоими бэкендами через этот
 * интерфейс: дубли ImageBundlePanel/ImageModelsPanel вынесены структурно —
 * рендер один, меняются только команды бэкенда.
 */
import { downloadImageBundle, downloadVideoBundle, estimateImageMemory, getImageBundleInfo, getImageModelsCatalog, getImagePoolOrphans, getVideoBundleInfo, getVideoBundles, removeImageBundle, removeImageBundleFromList, removeImagePoolOrphans, removeVideoBundleFiles, setImageBundleDir, setSelectedImageBundle, setSelectedVideoBundle, setVideoBundleDir, validateImageBundleDir, validateVideoBundleDir, } from '../index';
export const imageService = {
    kind: 'image',
    kindLabel: 'Бандл',
    progressKind: 'image-model',
    info: (name) => getImageBundleInfo(name).then((i) => i),
    entries: () => getImageModelsCatalog().then((list) => list.map((e) => ({ name: e.name, label: e.label, is_default: e.is_default }))),
    activate: (name) => setSelectedImageBundle(name).then((i) => i),
    validateDir: (path, name) => validateImageBundleDir(path, name),
    setCustomDir: (path) => setImageBundleDir(path).then((i) => i),
    clearCustomDir: () => removeImageBundleFromList().then((i) => i),
    download: (saveDir, name) => downloadImageBundle(saveDir, name).then((i) => i),
    removeFiles: (name) => removeImageBundle(name ?? null).then((i) => i),
    poolOrphans: () => getImagePoolOrphans(),
    removePoolOrphans: (filenames) => removeImagePoolOrphans(filenames),
    memoryProbe: async (name) => {
        const cur = await getImageBundleInfo(name);
        const info = (await imageSizeInfo(cur.bundle_name));
        const mem = await estimateImageMemory(cur.bundle_name);
        const gb = (mb) => `${(mb / 1024).toFixed(1)} ГБ`;
        return (`GPU: ${info.vram_fast_gb ?? '?'} ГБ (быстро) / ${info.vram_min_gb ?? '?'} ГБ (мин.) · ` +
            `сейчас свободно ${gb(mem.estimate.vram_free_mb)} VRAM / ${gb(mem.estimate.ram_free_mb)} RAM — ${mem.message}`);
    },
};
/** Обёртка для повторного чтения инфо по активному имени. */
async function imageSizeInfo(bundleName) {
    return getImageBundleInfo(bundleName);
}
export const videoService = {
    kind: 'video',
    kindLabel: 'Видео-бандл',
    progressKind: 'video-model',
    info: (name) => getVideoBundleInfo(name).then((i) => i),
    entries: () => getVideoBundles().then((list) => list.map((e) => ({ name: e.name, label: e.label, is_default: e.is_default }))),
    activate: (name) => setSelectedVideoBundle(name).then((i) => i),
    validateDir: (path, name) => validateVideoBundleDir(path, name),
    setCustomDir: (path) => setVideoBundleDir(path).then((i) => i),
    clearCustomDir: () => setVideoBundleDir(null).then((i) => i),
    download: (saveDir, name) => downloadVideoBundle(saveDir, name).then((i) => i),
    removeFiles: (name) => removeVideoBundleFiles(name ?? null).then((i) => i),
};
/** Реестр сервисов для декларативного `<image-bundle-tab service="...">`. */
export function serviceFor(kind) {
    if (kind === 'video')
        return videoService;
    if (kind === 'image')
        return imageService;
    return undefined;
}
