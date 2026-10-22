export declare class BundleTabElement extends HTMLElement {
    private root;
    private busy;
    private unlisten?;
    private appliedBundle;
    private service?;
    connectedCallback(): void;
    disconnectedCallback(): void;
    private setProgress;
    private showProgress;
    private refresh;
    private onBundleChange;
    private onActivate;
    /** Сборка целевой папки скачивания (логика ImageBundlePanel): если хоть один
     * файл уже на диске — продолжаем в текущей папке; иначе диалог. */
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
