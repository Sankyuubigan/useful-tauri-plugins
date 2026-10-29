/**
 * Адаптеры BundleService для вкладок «Картинки» / «Видео».
 *
 * Одна вкладка (image-bundle-tab) работает с обоими бэкендами через этот
 * интерфейс: дубли ImageBundlePanel/ImageModelsPanel вынесены структурно —
 * рендер один, меняются только команды бэкенда.
 */
import {
  downloadImageBundle,
  downloadVideoBundle,
  estimateImageMemory,
  getImageBundleInfo,
  getImageModelsCatalog,
  getVideoBundleInfo,
  getVideoBundles,
  removeImageBundle,
  removeImageBundleFromList,
  removeVideoBundleFiles,
  setImageBundleDir,
  setSelectedImageBundle,
  setSelectedVideoBundle,
  setVideoBundleDir,
  validateImageBundleDir,
  validateVideoBundleDir,
} from '../index'

export type TabFileInfo = {
  role: string
  filename: string
  size_bytes?: number | null
  save_path: string
  exists: boolean
}

/** Инфо о бандле, достаточная вкладке (структурно совместимы image/video). */
export type TabInfo = {
  bundle_name: string
  label: string
  note: string
  files: TabFileInfo[]
  total_bytes: number
  save_dir: string
  free_space_gb: number
  fully_downloaded: boolean
  vram_fast_gb?: number | null
  vram_min_gb?: number | null
  ram_min_gb?: number | null
}

export type TabEntry = {
  name: string
  label: string
  is_default?: boolean
}

export type TabValidate = {
  valid: boolean
  missing: string[]
}

/** Через что вкладка говорит с бэкендом. */
export interface BundleService {
  kind: 'image' | 'video'
  kindLabel: string
  /** kind события downloader:progress, по которому вкладка фильтрует прогресс. */
  progressKind: string
  info(name?: string | null): Promise<TabInfo>
  entries(): Promise<TabEntry[]>
  activate(name: string): Promise<TabInfo>
  /** Сброс кастом-выбора (есть только у вкладки «Картинки» — у видео дефолт каталога). */
  resetSelection?(name?: string | null): Promise<TabInfo>
  validateDir(path: string, name?: string | null): Promise<TabValidate>
  setCustomDir(path: string): Promise<TabInfo>
  clearCustomDir(): Promise<TabInfo>
  /** Качать в папку: для «Скачать набор» — после диалога, для «Докачать» — текущая. */
  download(saveDir: string, name?: string | null): Promise<TabInfo>
  /** Удалить файлы бандла (null = активный). Вкладка передаёт ВЫБРАННОЕ имя
   * (PRP 29.10.26 D1) — удаление выбранного не должно трогать активный. */
  removeFiles(name?: string | null): Promise<TabInfo>
  /** Текст прогноза памяти (только image: NVML-замер estimate). */
  memoryProbe?(name?: string | null): Promise<string>
}

export const imageService: BundleService = {
  kind: 'image',
  kindLabel: 'Бандл',
  progressKind: 'image-model',
  info: (name) => getImageBundleInfo(name).then((i) => i as unknown as TabInfo),
  entries: () =>
    getImageModelsCatalog().then((list) =>
      list.map((e) => ({ name: e.name, label: e.label, is_default: e.is_default })),
    ),
  activate: (name) => setSelectedImageBundle(name).then((i) => i as unknown as TabInfo),
  validateDir: (path, name) => validateImageBundleDir(path, name),
  setCustomDir: (path) => setImageBundleDir(path).then((i) => i as unknown as TabInfo),
  clearCustomDir: () => removeImageBundleFromList().then((i) => i as unknown as TabInfo),
  download: (saveDir, name) => downloadImageBundle(saveDir, name).then((i) => i as unknown as TabInfo),
  removeFiles: (name) => removeImageBundle(name ?? null).then((i) => i as unknown as TabInfo),
  memoryProbe: async (name) => {
    const cur = await getImageBundleInfo(name)
    const info = (await imageSizeInfo(cur.bundle_name)) as TabInfo
    const mem = await estimateImageMemory(cur.bundle_name)
    const gb = (mb: number) => `${(mb / 1024).toFixed(1)} ГБ`
    return (
      `GPU: ${info.vram_fast_gb ?? '?'} ГБ (быстро) / ${info.vram_min_gb ?? '?'} ГБ (мин.) · ` +
      `сейчас свободно ${gb(mem.estimate.vram_free_mb)} VRAM / ${gb(mem.estimate.ram_free_mb)} RAM — ${mem.message}`
    )
  },
}

/** Обёртка для повторного чтения инфо по активному имени. */
async function imageSizeInfo(bundleName: string | null): Promise<unknown> {
  return getImageBundleInfo(bundleName)
}

export const videoService: BundleService = {
  kind: 'video',
  kindLabel: 'Видео-бандл',
  progressKind: 'video-model',
  info: (name) => getVideoBundleInfo(name).then((i) => i as unknown as TabInfo),
  entries: () =>
    getVideoBundles().then((list) =>
      list.map((e) => ({ name: e.name, label: e.label, is_default: e.is_default })),
    ),
  activate: (name) => setSelectedVideoBundle(name).then((i) => i as unknown as TabInfo),
  validateDir: (path, name) => validateVideoBundleDir(path, name),
  setCustomDir: (path) => setVideoBundleDir(path).then((i) => i as unknown as TabInfo),
  clearCustomDir: () => setVideoBundleDir(null as unknown as string).then((i) => i as unknown as TabInfo),
  download: (saveDir, name) => downloadVideoBundle(saveDir, name).then((i) => i as unknown as TabInfo),
  removeFiles: (name) => removeVideoBundleFiles(name ?? null).then((i) => i as unknown as TabInfo),
}

/** Реестр сервисов для декларативного `<image-bundle-tab service="...">`. */
export function serviceFor(kind: string): BundleService | undefined {
  if (kind === 'video') return videoService
  if (kind === 'image') return imageService
  return undefined
}
