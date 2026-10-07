/**
 * Подсчёт токенов для плашек Live-превью.
 *
 * Основной путь — точный токенизатор HuggingFace (тот же, что у самой модели):
 * подгружается из CDN по требованию и кэшируется в браузере.
 * Если библиотека недоступна (оффлайн, ошибка загрузки) — фоллбэк по длине
 * текста (байты / 3), та же формула, по которой движок считает `--ctx-size`.
 */

const CHARS_PER_TOKEN = 3;

/** Таймаут загрузки токенизатора с CDN. По истечении — фоллбэк `chars/3`. */
const CDN_TIMEOUT_MS = 5000;

/**
 * ESM-сборка с jsdelivr. Фиксируем версию, чтобы CDN не отдал другую.
 * Загружается динамически — IIFE-бандл плагина не растёт, токенизатор
 * подтягивается только при первом реальном вызове `countTokens`.
 */
const TRANSFORMERS_CDN = 'https://cdn.jsdelivr.net/npm/@huggingface/transformers@3.4.0/+esm';

const tokenizerCache = new Map<string, Promise<unknown>>();
let transformersPromise: Promise<unknown> | null = null;

function withTimeout<T>(promise: Promise<T>, ms: number, label: string): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`${label}: таймаут ${ms}мс`)), ms);
    promise.then(
      (value) => { clearTimeout(timer); resolve(value); },
      (err) => { clearTimeout(timer); reject(err); },
    );
  });
}

async function loadTransformers(): Promise<Record<string, unknown>> {
  if (!transformersPromise) {
    transformersPromise = withTimeout(
      import(/* @vite-ignore */ TRANSFORMERS_CDN),
      CDN_TIMEOUT_MS,
      'Загрузка @huggingface/transformers',
    );
  }
  return transformersPromise as Promise<Record<string, unknown>>;
}

async function getTokenizer(tokenizerId: string): Promise<unknown> {
  if (!tokenizerCache.has(tokenizerId)) {
    const promise = (async () => {
      const mod = await loadTransformers();
      const AutoTokenizer = mod.AutoTokenizer as {
        from_pretrained(id: string): Promise<unknown>
      };
      return withTimeout(
        AutoTokenizer.from_pretrained(tokenizerId),
        CDN_TIMEOUT_MS,
        `Загрузка токенизатора ${tokenizerId}`,
      );
    })();
    tokenizerCache.set(tokenizerId, promise);
  }
  return tokenizerCache.get(tokenizerId);
}

export interface TokenCountResult {
  /** Число токенов. */
  tokens: number
  /** true — точный подсчёт токенизатором модели; false — оценка по длине. */
  exact: boolean
}

/**
 * Точный подсчёт токенов через `@huggingface/transformers` (CDN).
 * При недоступности токенизатора возвращает фоллбэк `байты / 3`
 * и `exact: false`.
 */
export async function countTokens(text: string, tokenizerId: string): Promise<TokenCountResult> {
  if (!text) {
    return { tokens: 0, exact: true };
  }
  try {
    const tokenizer = (await getTokenizer(tokenizerId)) as {
      encode(text: string, options?: { add_special_tokens?: boolean }): Promise<number[]> | number[]
    };
    const tokens = await tokenizer.encode(text, { add_special_tokens: false });
    return { tokens: tokens.length, exact: true };
  } catch (err) {
    const message = err instanceof Error ? err.message.split('\n')[0] : String(err);
    console.warn(`[llama-engine] countTokens: токенизатор ${tokenizerId} недоступен, фоллбэк по длине: ${message}`);
    return { tokens: Math.ceil(text.length / CHARS_PER_TOKEN), exact: false };
  }
}
