import { invoke } from '@tauri-apps/api/core'

export interface FfmpegStatus {
  installed: boolean
  ffmpegVersion: string | null
  ffprobeVersion: string | null
  dir: string
  binDir: string
  message: string
}

export async function getFfmpegStatus(dir: string): Promise<FfmpegStatus> {
  return invoke<FfmpegStatus>('plugin:ffmpeg|ffmpeg_status', { dir })
}
