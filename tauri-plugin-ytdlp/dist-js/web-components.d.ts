declare function esc(s: string): string;
declare function formatBytes(n: number): string;
declare function toast(msg: string, kind?: 'success' | 'error'): void;
export { esc, formatBytes, toast };
