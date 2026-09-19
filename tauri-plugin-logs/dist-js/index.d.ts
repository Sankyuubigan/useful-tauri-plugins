import { type UnlistenFn } from '@tauri-apps/api/event';
export interface ErrorReportInput {
    errorType: string;
    message: string;
    stack?: string | null;
    severity?: string;
    kind?: string;
}
/** Подписка на строки лога от единого кастомного log::Log (core rules §2.5). */
export declare function onLogMessage(cb: (line: string) => void): Promise<UnlistenFn>;
/** Разрешённый путь dev-зеркала (test/last_logs.txt) или пустая строка. */
export declare function getLastLogsPath(): Promise<string>;
/** Путь exe-файла лога (например king_orch.log) или null. */
export declare function getLogFilePath(): Promise<string | null>;
/** Запись строки лога из frontend через единый логгер (level по умолчанию «FE»). */
export declare function logFront(msg: string): void;
/** Запись строки лога с произвольным уровнем (FE/WARN/ERROR…). Никогда не бросает. */
export declare function logFrontendEvent(level: string, msg: string): Promise<void>;
/** Включить/выключить облачную отправку (синхронизирует flаг allow_error_reports хоста). */
export declare function setReportingEnabled(enabled: boolean): Promise<void>;
/** Анонимное analytics-событие (Aptabase). Возвращает false, если отключено или ошибка. */
export declare function trackEvent(name: string, props?: Record<string, unknown>): Promise<boolean>;
/**
 * Отправка ошибки в облако.
 * kind: "crash" | "unhandled" | "taskException" | "handled",
 * severity: "fatal" | "error".
 * Снимок буфера breadcrumbs прикладывается к отчёту автоматически.
 */
export declare function trackError(report: ErrorReportInput): Promise<void>;
/** Диалог «Сохранить как» + запись текста лога в выбранный файл. Возвращает путь. */
export declare function saveLogsToFile(content: string): Promise<string | null>;
/** Один раз на старте приложения: window.onerror + unhandledrejection → лог + облако + дамп. */
export declare function initFrontendErrorCapture(): void;
import './web-components';
