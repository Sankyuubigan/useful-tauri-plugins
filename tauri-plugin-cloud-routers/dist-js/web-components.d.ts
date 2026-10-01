export declare class CloudRoutersPanel extends HTMLElement {
    private router;
    private states;
    private combosByRouter;
    private offProgress;
    private visibilityObserver;
    private refreshTimer;
    private root;
    private unsubUpdate?;
    constructor();
    private stateOf;
    /** Текущий статус активного роутера; null пока он неизвестен или ошибка. */
    private get status();
    private get combos();
    /**
     * Запись состояния — всегда с явным роутером. Иначе ответ на действие, начатое
     * на одной вкладке, лёг бы в слот той, на которую юзер успел переключиться.
     */
    private setStatus;
    private setCombos;
    connectedCallback(): void;
    disconnectedCallback(): void;
    private observeVisibility;
    /**
     * Обновить статус роутера. Ответ пишется в слот ИМЕННО `target`, поэтому быстрые
     * клики по вкладкам не могут записать статус чужого роутера (гонка last-write-wins).
     *
     * Кэш НЕ затирается: если статус уже `ready`, он остаётся на экране, пока идёт
     * фоновая проверка (stale-while-revalidate) — переключение вкладок не мигает.
     */
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
