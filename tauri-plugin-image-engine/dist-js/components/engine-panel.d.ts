export declare class ImageEnginePanel extends HTMLElement {
    private root;
    private busy;
    private applied;
    private unlisten?;
    private unsubUpdate?;
    connectedCallback(): void;
    disconnectedCallback(): void;
    private setStatus;
    private setProgress;
    private showProgress;
    private hintText;
    private refresh;
    /**
     * Единственное место, где видны бейдж и кнопка «Обновить»: оба выводятся
     * из одного состояния. Раньше в ветке `else` прятался только бейдж — кнопка
     * оставалась на экране после установки обновления.
     */
    private renderUpdateState;
    private applyButtonStates;
    private onVariantChange;
    private onApplyVariant;
    private onInstall;
    private onCheckUpdate;
    private onInstallUpdate;
    private onRemove;
    private onSetDir;
}
