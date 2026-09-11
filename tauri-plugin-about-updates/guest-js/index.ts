import { invoke } from '@tauri-apps/api/core'
import { check, type Update } from '@tauri-apps/plugin-updater'

export interface ReleaseInfo {
  version: string
  pubDate?: string | null
  notes?: string | null
  downloadUrl: string
  isCurrent: boolean
}

/** История релизов GitHub (для отката). Репозиторий задан в tauri.conf.json плагина. */
export async function getReleaseHistory(): Promise<ReleaseInfo[]> {
  return invoke<ReleaseInfo[]>('plugin:about-updates|get_release_history')
}

/** Откат на конкретный релиз по его URL установщика. */
export async function installRelease(downloadUrl: string): Promise<void> {
  return invoke('plugin:about-updates|install_release', { downloadUrl })
}

/** Версия хост-приложения. */
export async function getAppVersion(): Promise<string> {
  return invoke<string>('plugin:about-updates|get_app_version')
}

/** Настроенная ссылка «Поддержать автора» (или null, если не задана). */
export async function getSupportUrl(): Promise<string | null> {
  return invoke<string | null>('plugin:about-updates|get_support_url')
}

/** Проверка обновления через официальный tauri-plugin-updater (читает latest.json хоста). */
export async function checkForUpdate(): Promise<Update | null> {
  return check()
}

/** Скачивание и установка обновления (тихо, без подтверждения). */
export async function downloadAndInstallUpdate(update: Update): Promise<void> {
  await update.downloadAndInstall()
}

// Регистрируем Web Component при импорте пакета.
import './web-components'
