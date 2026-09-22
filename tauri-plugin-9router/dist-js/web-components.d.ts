export declare class NineRouterPanel extends HTMLElement {
    private status;
    private combos;
    private busy;
    private offProgress;
    private visibilityObserver;
    private refreshTimer;
    private root;
    constructor();
    connectedCallback(): void;
    disconnectedCallback(): void;
    /** Панель в Settings монтируется один раз при старте приложения, а раздел
     *  переключается классом `.active`. Перечитываем статус каждый раз, когда
     *  раздел становится видимым, — чтобы панель не «врала» устаревшим статусом. */
    private observeVisibility;
    private refresh;
    private onProgress;
    private setBusy;
    private notifyCombosChanged;
    private onInstall;
    private onOpen;
    private onShowCombos;
    private onSaveApiKey;
    private onSetDir;
    private onCheckUpdate;
    private onInstallUpdate;
    private render;
}
