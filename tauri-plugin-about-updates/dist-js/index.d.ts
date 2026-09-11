import { type Update } from '@tauri-apps/plugin-updater';
export interface ReleaseInfo {
    version: string;
    pubDate?: string | null;
    notes?: string | null;
    downloadUrl: string;
    isCurrent: boolean;
}
/** История релизов GitHub (для отката). Репозиторий задан в tauri.conf.json плагина. */
export declare function getReleaseHistory(): Promise<ReleaseInfo[]>;
/** Откат на конкретный релиз по его URL установщика. */
export declare function installRelease(downloadUrl: string): Promise<void>;
/** Версия хост-приложения. */
export declare function getAppVersion(): Promise<string>;
/** Настроенная ссылка «Поддержать автора» (или null, если не задана). */
export declare function getSupportUrl(): Promise<string | null>;
/** Проверка обновления через официальный tauri-plugin-updater (читает latest.json хоста). */
export declare function checkForUpdate(): Promise<Update | null>;
/** Скачивание и установка обновления (тихо, без подтверждения). */
export declare function downloadAndInstallUpdate(update: Update): Promise<void>;
import './web-components';
