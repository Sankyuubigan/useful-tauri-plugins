export type TabFileInfo = {
    role: string;
    filename: string;
    size_bytes?: number | null;
    save_path: string;
    exists: boolean;
};
/** Инфо о бандле, достаточная вкладке (структурно совместимы image/video). */
export type TabInfo = {
    bundle_name: string;
    label: string;
    note: string;
    files: TabFileInfo[];
    total_bytes: number;
    save_dir: string;
    free_space_gb: number;
    fully_downloaded: boolean;
    vram_fast_gb?: number | null;
    vram_min_gb?: number | null;
    ram_min_gb?: number | null;
};
export type TabEntry = {
    name: string;
    label: string;
    is_default?: boolean;
};
export type TabValidate = {
    valid: boolean;
    missing: string[];
};
/** Через что вкладка говорит с бэкендом. */
export interface BundleService {
    kind: 'image' | 'video';
    kindLabel: string;
    /** kind события downloader:progress, по которому вкладка фильтрует прогресс. */
    progressKind: string;
    info(name?: string | null): Promise<TabInfo>;
    entries(): Promise<TabEntry[]>;
    activate(name: string): Promise<TabInfo>;
    /** Сброс кастом-выбора (есть только у вкладки «Картинки» — у видео дефолт каталога). */
    resetSelection?(name?: string | null): Promise<TabInfo>;
    validateDir(path: string, name?: string | null): Promise<TabValidate>;
    setCustomDir(path: string): Promise<TabInfo>;
    clearCustomDir(): Promise<TabInfo>;
    /** Качать в папку: для «Скачать набор» — после диалога, для «Докачать» — текущая. */
    download(saveDir: string, name?: string | null): Promise<TabInfo>;
    /** Удалить файлы бандла (null = активный). Вкладка передаёт ВЫБРАННОЕ имя
     * (PRP 29.10.26 D1) — удаление выбранного не должно трогать активный. */
    removeFiles(name?: string | null): Promise<TabInfo>;
    /** Текст прогноза памяти (только image: NVML-замер estimate). */
    memoryProbe?(name?: string | null): Promise<string>;
}
export declare const imageService: BundleService;
export declare const videoService: BundleService;
/** Реестр сервисов для декларативного `<image-bundle-tab service="...">`. */
export declare function serviceFor(kind: string): BundleService | undefined;
