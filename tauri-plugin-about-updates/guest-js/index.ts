import { invoke } from '@tauri-apps/api/core'
import { check, type Update } from '@tauri-apps/plugin-updater'

export interface ReleaseInfo {
  version: string
  pubDate?: string | null
  notes?: string | null
  downloadUrl: string
  isCurrent: boolean
}

/** Что запускало установку последний раз. */
export type InstallKind = 'rollback' | 'update'

/** Состояние последней установки. */
export type InstallState = 'pending' | 'done' | 'failed'

/** Отчёт о последней установке: вердикт по фактически установленной версии. */
export interface InstallReport {
  kind: InstallKind
  targetVersion: string
  previousVersion: string
  installer: string
  installerBytes: number
  nsisArgs: string
  startedAt: string
  state: InstallState
  finishedAt?: string | null
  currentVersion?: string | null
  detail?: string | null
}

/** История релизов GitHub (для отката). Репозиторий задан в tauri.conf.json плагина. */
export async function getReleaseHistory(): Promise<ReleaseInfo[]> {
  return invoke<ReleaseInfo[]>('plugin:about-updates|get_release_history')
}

/**
 * Откат на конкретный релиз по его URL установщика.
 *
 * `version` обязателен: бэкенд пишет отчёт об установке и на следующем старте
 * сверяет его с фактически установленной версией — без запрошенной версии
 * «откат сработал / не сработал» нечем подтвердить.
 */
export async function installRelease(downloadUrl: string, version: string): Promise<void> {
  return invoke('plugin:about-updates|install_release', { downloadUrl, version })
}

/** Отчёт о последней установке (откат/обновление) с вердиктом, либо null. */
export async function getInstallReport(): Promise<InstallReport | null> {
  return invoke<InstallReport | null>('plugin:about-updates|get_install_report')
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
