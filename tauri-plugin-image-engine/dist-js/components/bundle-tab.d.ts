export declare class BundleTabElement extends HTMLElement {
    private root;
    private busy;
    private unlisten?;
    private appliedBundle;
    private service?;
    /** Статусы бандлов из последнего refresh (fully_downloaded и файлы) —
     * источник гейта кнопки «Активировать». */
    private statuses;
    connectedCallback(): void;
    disconnectedCallback(): void;
    private setProgress;
    private showProgress;
    private refresh;
    private onBundleChange;
    private onActivate;
    /** Сборка целевой папки скачивания: если хоть один файл уже на диске —
     * продолжаем в текущей папке; иначе диалог. Выбранная папка = ПУЛ для всех
     * наборов (25.10.2026): подпапка `${base}/${bundle_name}` больше не создаётся —
     * она порождала перекачку общих файлов и «3 файла есть, 1 нет» при
     * переключении активного бандла (инцидент 25.10.2026). */
    private onDownload;
    /** Custom-папка (add): диалог → validate → set → честный тост о нехватке. */
    private onAddCustomDir;
    /** Custom-папка (resume): докачать недостающие в текущую папку. */
    private onResume;
    private onRemoveFromList;
    private confirmDelete;
    private closeModal;
    private doDeleteFiles;
}
