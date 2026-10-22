export declare class CloudRoutersPanel extends HTMLElement {
    private router;
    private states;
    private combosByRouter;
    private offProgress;
    private visibilityObserver;
    private refreshTimer;
    private root;
    private unsubUpdate?;
    /**
     * Роутеры, для которых установка/переустановка ВЫПОЛНЯЕТСЯ прямо сейчас.
     * Почему нельзя полагаться на backend-статус: во время установки статусы
     * честно сообщают «неполная установка» (node.exe уже есть, tgz ещё качается),
     * и каждые 15 секунд refresh рисовал бы предупреждение о поломке, хотя
     * установка в разгаре.
     */
    private installing;
    /**
     * Последний прогресс установки по роутерам. Хранится в state, потому что
     * periodic render() пересоздаёт DOM, а `onProgress` мутирует DOM императивно —
     * без этого долгие стадии без событий (распаковка, npm, postinstall)
     * «сбрасывали» бы полосу прогресса между событиями. `total < 0` — ошибка.
     */
    private progressByRouter;
    /** Роутер, у которого «Переустановить» ожидает второй клик-подтверждения. */
    private reinstallArm;
    private reinstallArmTimer;
    constructor();
    private stateOf;
    private get status();
    private get combos();
    private setStatus;
    private setCombos;
    connectedCallback(): void;
    disconnectedCallback(): void;
    private observeVisibility;
    private refresh;
    private onProgress;
    /**
     * Восстановить прогресс-бар из сохранённого state (после render()).
     * Императивные DOM-правки имеет право делать ТОЛЬКО этот метод и
     * `applyProgress`: оба синхронизированы с `progressByRouter`, поэтому
     * перерисовка не может потерять текущий ход установки.
     */
    private applyProgress;
    private setBtnBusy;
    private notifyCombosChanged;
    private onInstall;
    /**
     * Ошибка установки/переустановки: тост + лог + строка прогресса превращается
     * в ошибку. Ошибка сохраняется в `progressByRouter`, а не только в DOM:
     * последующие перерисовки (refresh каждые 15с) не должны её терять.
     */
    private reportInstallError;
    private onOpen;
    private onStart;
    private onStop;
    private onShowCombos;
    private onSetDir;
    private onCheckUpdate;
    /** Скопировать адрес Web UI: юзер открывает дашборд в обычном браузере, а не
     * только вкладкой приложения. Адрес — из статуса роутера
     * (`RouterStatus.base_url`), доп. источников нет (SSOT). */
    private onCopyUrl;
    private onInstallUpdate;
    /**
     * Снять «armed»-состояние кнопки «Переустановить» (таймер отката).
     * Вызывается при втором клике, смене вкладки и истечении окна подтверждения.
     */
    private disarmReinstallArm;
    /**
     * Кнопка «Переустановить»: остановить сервер, снести установку и данные
     * (настройки дашборда, включая пароль; комбо) и поставить последнюю версию
     * заново. Двойное подтверждение вместо `confirm()`: первый клик поднимает
     * предупреждение с 6-секундным окном, второй клик в этом окне — запуск.
     */
    private onReinstall;
    private switchRouter;
    private render;
    private renderUpdateState;
}
