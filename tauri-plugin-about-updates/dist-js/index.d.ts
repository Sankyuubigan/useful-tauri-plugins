import { type Update } from '@tauri-apps/plugin-updater';
export interface ReleaseInfo {
    version: string;
    pubDate?: string | null;
    notes?: string | null;
    downloadUrl: string;
    isCurrent: boolean;
}
/** Что запускало установку последний раз. */
export type InstallKind = 'rollback' | 'update';
/** Состояние последней установки. */
export type InstallState = 'pending' | 'done' | 'failed';
/** Отчёт о последней установке: вердикт по фактически установленной версии. */
export interface InstallReport {
    kind: InstallKind;
    targetVersion: string;
    previousVersion: string;
    installer: string;
    installerBytes: number;
    nsisArgs: string;
    startedAt: string;
    state: InstallState;
    finishedAt?: string | null;
    currentVersion?: string | null;
    detail?: string | null;
}
/** История релизов GitHub (для отката). Репозиторий задан в tauri.conf.json плагина. */
export declare function getReleaseHistory(): Promise<ReleaseInfo[]>;
/**
 * Откат на конкретный релиз по его URL установщика.
 *
 * `version` обязателен: бэкенд пишет отчёт об установке и на следующем старте
 * сверяет его с фактически установленной версией — без запрошенной версии
 * «откат сработал / не сработал» нечем подтвердить.
 */
export declare function installRelease(downloadUrl: string, version: string): Promise<void>;
/** Отчёт о последней установке (откат/обновление) с вердиктом, либо null. */
export declare function getInstallReport(): Promise<InstallReport | null>;
/** Версия хост-приложения. */
export declare function getAppVersion(): Promise<string>;
/** Настроенная ссылка «Поддержать автора» (или null, если не задана). */
export declare function getSupportUrl(): Promise<string | null>;
/** Проверка обновления через официальный tauri-plugin-updater (читает latest.json хоста). */
export declare function checkForUpdate(): Promise<Update | null>;
/** Скачивание и установка обновления (тихо, без подтверждения). */
export declare function downloadAndInstallUpdate(update: Update): Promise<void>;
import './web-components';
