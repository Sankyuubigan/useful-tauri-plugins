/**
 * Панель System-1: проверка комплекта файлов, установка, статус.
 *
 * Показывает ТОЛЬКО факты, полученные от бэкенда: наличие и размер каждого
 * файла, реальное устройство исполнения. Ничего не достраивает на клиенте —
 * иначе панель врала бы при расхождении с реальностью.
 *
 * Имена методов НЕ пересекаются с `HTMLElement`: `style` и `remove` уже заняты
 * самим DOM, и перекрытие ломает и типизацию, и вызовы (`remove()` у элемента
 * удаляет его из DOM — молчаливая, но неприятная ошибка).
 */
declare class System1Panel extends HTMLElement {
    private status;
    private busy;
    connectedCallback(): void;
    disconnectedCallback(): void;
    private buildStyle;
    private buildBody;
    private onProgress;
    private refresh;
    private download;
    private deleteModel;
    private setButtons;
    /** Одна строка таблицы файлов. */
    private renderFile;
    private render;
}
export { System1Panel };
