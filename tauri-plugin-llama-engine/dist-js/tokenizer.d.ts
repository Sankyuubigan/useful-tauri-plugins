/**
 * Подсчёт токенов для плашек Live-превью.
 *
 * Основной путь — точный токенизатор HuggingFace (тот же, что у самой модели):
 * подгружается из CDN по требованию и кэшируется в браузере.
 * Если библиотека недоступна (оффлайн, ошибка загрузки) — фоллбэк по длине
 * текста (байты / 3), та же формула, по которой движок считает `--ctx-size`.
 */
export interface TokenCountResult {
    /** Число токенов. */
    tokens: number;
    /** true — точный подсчёт токенизатором модели; false — оценка по длине. */
    exact: boolean;
}
/**
 * Точный подсчёт токенов через `@huggingface/transformers` (CDN).
 * При недоступности токенизатора возвращает фоллбэк `байты / 3`
 * и `exact: false`.
 */
export declare function countTokens(text: string, tokenizerId: string): Promise<TokenCountResult>;
