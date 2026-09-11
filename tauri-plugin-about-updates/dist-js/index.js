import { invoke } from '@tauri-apps/api/core';
import { check } from '@tauri-apps/plugin-updater';
/** История релизов GitHub (для отката). Репозиторий задан в tauri.conf.json плагина. */
export async function getReleaseHistory() {
    return invoke('plugin:about-updates|get_release_history');
}
/** Откат на конкретный релиз по его URL установщика. */
export async function installRelease(downloadUrl) {
    return invoke('plugin:about-updates|install_release', { downloadUrl });
}
/** Версия хост-приложения. */
export async function getAppVersion() {
    return invoke('plugin:about-updates|get_app_version');
}
/** Настроенная ссылка «Поддержать автора» (или null, если не задана). */
export async function getSupportUrl() {
    return invoke('plugin:about-updates|get_support_url');
}
/** Проверка обновления через официальный tauri-plugin-updater (читает latest.json хоста). */
export async function checkForUpdate() {
    return check();
}
/** Скачивание и установка обновления (тихо, без подтверждения). */
export async function downloadAndInstallUpdate(update) {
    await update.downloadAndInstall();
}
// Регистрируем Web Component при импорте пакета.
import './web-components';
