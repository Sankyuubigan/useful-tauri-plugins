import { type UnlistenFn } from '@tauri-apps/api/event';
/** Опции скачивания (зеркало DownloadOptions на бэке). */
export interface DownloadOptions {
    label?: string;
    kind?: string;
    expected_size?: number | null;
    magic?: number[] | null;
    min_size?: number | null;
    keep_partial?: boolean;
}
/** Payload события `downloader:progress`. */
export interface DownloadProgress {
    task_id: string;
    label: string;
    kind: string;
    url: string;
    dest: string;
    downloaded: number;
    total: number;
    speed_bps: number;
    /** Оценка оставшегося времени, сек; < 0 — неизвестно. */
    eta_s: number;
    level: number;
    level_name: string;
    status: 'running' | 'done' | 'error' | 'cancelled';
    message: string;
}
/** Активная задача из list_active. */
export interface ActiveDownload {
    task_id: string;
    label: string;
    kind: string;
    url: string;
    dest: string;
    elapsed_ms: number;
    cancelled: boolean;
}
/** Скачать файл. Прогресс — событие `downloader:progress`. */
export declare function downloadFile(url: string, dest: string, opts?: DownloadOptions): Promise<void>;
/** Скачать во временный файл и вернуть байты (маленькие ответы). */
export declare function downloadBytes(url: string, opts?: DownloadOptions): Promise<number[]>;
/** Отменить активную загрузку. true — задача найдена. */
export declare function cancelDownload(taskId: string): Promise<boolean>;
/** Список активных загрузок. */
export declare function listActive(): Promise<ActiveDownload[]>;
/** Подписка на единый прогресс-событие. */
export declare function onProgress(cb: (p: DownloadProgress) => void): Promise<UnlistenFn>;
import './web-components';
