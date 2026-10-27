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
    /** «Скачать набор»/«Докачать файлы»: качает ВСЕГДА в настроенную папку
     * (`save_dir` — кастом или дефолт KingOrchData, пустым не бывает — бэкенд
     * `commands.rs` гарантирует дефолт). Выбор пути — ровно одно место на
     * вкладке: кнопка «Изменить путь…» (SSOT решений; раньше диалог дублировал
     * её на «Скачать» и вместо скачивания открывал проводник). Выбранная папка
     * = ПУЛ для всех наборов (25.10.2026): подпапка `${base}/${bundle_name}`
     * не создаётся — она порождала перекачку общих файлов. */
    private onDownload;
    /** «Изменить путь…»: диалог → проверка наличия файлов → set → честный тост
     * о нехватке. Выбранная папка становится пулом этого набора. */
    private onChangeDir;
    /** «Сбросить путь»: вернуть папку набора к дефолту хранилища. */
    private onResetDir;
    /** Custom-папка (resume): докачать недостающие в текущую папку. */
    private onResume;
    private confirmDelete;
    private closeModal;
    private doDeleteFiles;
}
