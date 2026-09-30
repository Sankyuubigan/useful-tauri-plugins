import { invoke } from '@tauri-apps/api/core'

export interface YtdlpStatus {
  installed: boolean
  version?: string | null
  exe_path: string
  dir: string
  ffmpeg_installed: boolean
  ffmpeg_path?: string | null
  deno_installed: boolean
  deno_path?: string | null
  message: string
}

export interface VideoInfo {
  title?: string
  duration?: number
  thumbnail?: string
  uploader?: string
  description?: string
  [key: string]: unknown
}

export function getYtdlpStatus(): Promise<YtdlpStatus> {
  return invoke<YtdlpStatus>('plugin:ytdlp|get_ytdlp_status')
}

export function installYtdlp(): Promise<YtdlpStatus> {
  return invoke<YtdlpStatus>('plugin:ytdlp|install_ytdlp')
}

export function checkYtdlpUpdate(): Promise<string | null> {
  return invoke<string | null>('plugin:ytdlp|check_ytdlp_update')
}

export function installYtdlpUpdate(): Promise<YtdlpStatus> {
  return invoke<YtdlpStatus>('plugin:ytdlp|install_ytdlp_update')
}

export function setYtdlpDir(path: string): Promise<YtdlpStatus> {
  return invoke<YtdlpStatus>('plugin:ytdlp|set_ytdlp_dir', { path })
}

export function setFfmpegPath(path: string): Promise<YtdlpStatus> {
  return invoke<YtdlpStatus>('plugin:ytdlp|set_ffmpeg_path', { path })
}

export function installDeno(): Promise<YtdlpStatus> {
  return invoke<YtdlpStatus>('plugin:ytdlp|install_deno')
}

export function fetchVideoInfo(url: string): Promise<VideoInfo> {
  return invoke<VideoInfo>('plugin:ytdlp|fetch_video_info', { url })
}

export interface DownloadParams {
  url: string
  format: string
  quality: string
  outputDir: string
  authMode: string
  browser?: string | null
  cookiesFile?: string | null
}

export function downloadVideo(params: DownloadParams): Promise<void> {
  return invoke<void>('plugin:ytdlp|download_video', {
    url: params.url,
    format: params.format,
    quality: params.quality,
    outputDir: params.outputDir,
    authMode: params.authMode,
    browser: params.browser ?? null,
    cookiesFile: params.cookiesFile ?? null,
  })
}

export function cancelDownload(): Promise<void> {
  return invoke<void>('plugin:ytdlp|cancel_download')
}

import './web-components'
