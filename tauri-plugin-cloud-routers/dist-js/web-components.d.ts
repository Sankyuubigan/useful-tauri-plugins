export declare class CloudRoutersPanel extends HTMLElement {
    private router;
    private status;
    private combos;
    private offProgress;
    private visibilityObserver;
    private refreshTimer;
    private root;
    private unsubUpdate?;
    constructor();
    connectedCallback(): void;
    disconnectedCallback(): void;
    private observeVisibility;
    private refresh;
    private onProgress;
    private setBtnBusy;
    private notifyCombosChanged;
    private onInstall;
    private onOpen;
    private onStop;
    private onShowCombos;
    private onSaveApiKey;
    private onSetDir;
    private onCheckUpdate;
    private onInstallUpdate;
    private switchRouter;
    private render;
    private renderUpdateState;
}
