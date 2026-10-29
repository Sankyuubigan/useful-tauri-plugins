export declare class LoraTabElement extends HTMLElement {
    private root;
    private busy;
    private unlisten?;
    private files;
    private repo;
    private localDir;
    connectedCallback(): void;
    disconnectedCallback(): void;
    private refresh;
    private renderGroups;
    /** Скачать веса + их sidecar-описание (если есть в каталоге). */
    private fetchWithSidecar;
    private downloadOne;
    private downloadAll;
}
