import { invoke } from '@tauri-apps/api/core';
export async function getFfmpegStatus(dir) {
    return invoke('plugin:ffmpeg|ffmpeg_status', { dir });
}
