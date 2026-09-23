import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

/** Опции скачивания (зеркало DownloadOptions на бэке). */
export interface DownloadOptions {
  label?: string
  kind?: string
  expected_size?: number | null
  magic?: number[] | null
  min_size?: number | null
  keep_partial?: boolean
}

/** Payload события `downloader:progress`. */
export interface DownloadProgress {
  task_id: string
  label: string
  kind: string
  url: string
  dest: string
  downloaded: number
  total: number
  speed_bps: number
  /** Оценка оставшегося времени, сек; < 0 — неизвестно. */
  eta_s: number
  level: number
  level_name: string
  status: 'running' | 'done' | 'error' | 'cancelled'
  message: string
}

/** Активная задача из list_active. */
export interface ActiveDownload {
  task_id: string
  label: string
  kind: string
  url: string
  dest: string
  elapsed_ms: number
  cancelled: boolean
}

/** Скачать файл. Прогресс — событие `downloader:progress`. */
export function downloadFile(url: string, dest: string, opts?: DownloadOptions): Promise<void> {
  return invoke('plugin:downloader|download_file', { url, dest, opts: opts ?? null })
}

/** Скачать во временный файл и вернуть байты (маленькие ответы). */
export function downloadBytes(url: string, opts?: DownloadOptions): Promise<number[]> {
  return invoke('plugin:downloader|download_bytes', { url, opts: opts ?? null })
}

/** Отменить активную загрузку. true — задача найдена. */
export function cancelDownload(taskId: string): Promise<boolean> {
  return invoke('plugin:downloader|cancel_download', { taskId })
}

/** Список активных загрузок. */
export function listActive(): Promise<ActiveDownload[]> {
  return invoke('plugin:downloader|list_active')
}

/** Подписка на единый прогресс-событие. */
export function onProgress(cb: (p: DownloadProgress) => void): Promise<UnlistenFn> {
  return listen<DownloadProgress>('downloader:progress', (e) => cb(e.payload))
}

import './web-components'
