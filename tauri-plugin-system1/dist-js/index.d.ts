import { type UnlistenFn } from '@tauri-apps/api/event';
/** Тип типизированного вопроса. Плагин знает только `noul`. */
export type QuestionType = 'noul';
/** Один вариант ответа. */
export interface OptionSpec {
    /** Текст критерия, который увидит модель. */
    text: string;
}
/** Типизированный вопрос к модели System-1. */
export interface TypedQuestion {
    /** Идентификатор для маппинга результата (например, `"e3"`). */
    id: string;
    qtype: QuestionType;
    instructions: string;
    /**
     * Ровно два варианта: `[отрицательный, положительный]`.
     *
     * Порядок значим: перестановка вариантов меняет ответ модели
     * (`docs/LAYA_MODEL.md` §4.2). Плагин усредняет обмен букв A/B, но слот
     * положительного варианта остаётся вторым.
     */
    options: [OptionSpec, OptionSpec];
}
/** Политика перестановок. */
export type PermutationPolicy = 'letters' | 'single';
/** Запрос инференса. */
export interface DecisionRequest {
    state: string;
    questions: TypedQuestion[];
    /** По умолчанию `letters` — два прохода с обменом A/B. */
    permutations?: PermutationPolicy;
}
/** Ответ по одному вопросу. */
export interface QuestionAnswer {
    id: string;
    /** Усреднённая вероятность истинного варианта, 0..=1. */
    pTrue: number;
    passes: number;
}
/** Результат инференса. */
export interface DecisionResult {
    answers: QuestionAnswer[];
    /** Всегда `cpu`: CUDA требует системных зависимостей, DirectML откачен. */
    device: string;
    elapsedMs: number;
}
/** Состояние ONNX Runtime на диске. */
export interface RuntimeState {
    present: boolean;
    /** Полный путь, куда кладётся и где ищется DLL. */
    path: string;
    sizeBytes: number;
    /** Версия, которую ставит плагин. */
    version: string;
    expectedBytes: number;
}
/** Один файл комплекта System-1. */
export interface FileStatus {
    /** `runtime` | `model` | `tokenizer`. */
    kind: string;
    name: string;
    path: string;
    present: boolean;
    sizeBytes: number;
    /** Что делать, если файла нет. */
    action: string;
}
/** Сводный статус System-1. */
export interface StatusReport {
    defaultModel: string;
    modelPresent: boolean;
    expectedBytes: number;
    runtimePresent: boolean;
    runtimePath: string;
    runtimeSearchPaths: string[];
    loaded: boolean;
    device: string;
    modelsDir: string;
    runtime: RuntimeState;
    /** Пофайловая проверка: рантайм, модель, токенизатор. */
    files: FileStatus[];
}
/** Состояние модели на диске. */
export interface ModelState {
    id: string;
    displayName: string;
    dir: string;
    present: boolean;
    missing: string[];
    expectedBytes: number;
}
/** Статус System-1: модель, рантайм ONNX, устройство. */
export declare function getStatus(): Promise<StatusReport>;
/**
 * Скачать весь комплект System-1: сначала ONNX Runtime, затем модель.
 *
 * Единственная команда установки с нуля. Порядок зафиксирован на бэкенде:
 * инференс без DLL невозможен, поэтому качать 646 МБ модели при отсутствии
 * рантайма — тратить время впустую.
 */
export declare function downloadAll(): Promise<ModelState>;
/** Скачать модель в `KingOrchData/system1/models/<id>`. */
export declare function downloadModel(modelId?: string): Promise<ModelState>;
/** Удалить скачанную модель с диска. */
export declare function removeModel(modelId?: string): Promise<void>;
/**
 * Выполнить типизированные вопросы и вернуть вероятности.
 *
 * Плагин не решает, какой вердикт «правильный» — пороги и критерии живут в хосте.
 */
export declare function decide(request: DecisionRequest): Promise<DecisionResult>;
/** Событие прогресса скачивания (от `tauri-plugin-downloader`). */
export interface DownloadProgress {
    url?: string;
    dest?: string;
    downloaded?: number;
    total?: number;
}
/** Подписаться на прогресс скачивания модели. */
export declare function onDownloadProgress(cb: (progress: DownloadProgress) => void): Promise<UnlistenFn>;
import './web-components';
