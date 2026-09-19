declare function esc(s: string): string;
declare function fileName(p: string): string;
declare function formatBytes(n: number): string;
/** Лёгкий всплывающий toст (без внешних зависимостей; использует CSS-переменные хоста). */
declare function toast(msg: string, kind?: 'success' | 'error'): void;
export { esc, fileName, formatBytes, toast };
