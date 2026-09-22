import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { save } from '@tauri-apps/plugin-dialog'

export interface ErrorReportInput {
  errorType: string
  message: string
  stack?: string | null
  severity?: string
  kind?: string
}

/** Подписка на строки лога от единого кастомного log::Log (core rules §2.5). */
export function onLogMessage(cb: (line: string) => void): Promise<UnlistenFn> {
  return listen<string>('logs:message', (e) => cb(e.payload))
}

/** Разрешённый путь dev-зеркала (test/last_logs.txt) или пустая строка. */
export function getLastLogsPath(): Promise<string> {
  return invoke<string>('plugin:logs|get_last_logs_path')
}

/** Путь exe-файла лога (например king_orch.log) или null. */
export function getLogFilePath(): Promise<string | null> {
  return invoke<string | null>('plugin:logs|get_log_file_path')
}

/** Запись строки лога из frontend через единый логгер (level по умолчанию «FE»). */
export function logFront(msg: string): void {
  void invoke('plugin:logs|log_frontend_event', { level: 'FE', msg }).catch(() => {})
}

/** Запись строки лога с произвольным уровнем (FE/WARN/ERROR…). Никогда не бросает. */
export async function logFrontendEvent(level: string, msg: string): Promise<void> {
  try {
    await invoke('plugin:logs|log_frontend_event', { level, msg })
  } catch {
    // лог не должен ронять приложение
  }
}

/** Включить/выключить облачную отправку (синхронизирует flаг allow_error_reports хоста). */
export async function setReportingEnabled(enabled: boolean): Promise<void> {
  try {
    await invoke('plugin:logs|set_reporting_enabled', { enabled })
  } catch {
    /* ignore */
  }
}

/** Анонимное analytics-событие (Aptabase). Возвращает false, если отключено или ошибка. */
export async function trackEvent(name: string, props?: Record<string, unknown>): Promise<boolean> {
  try {
    await invoke('plugin:logs|track_event', { name, props: props ?? null })
    return true
  } catch {
    return false
  }
}

/**
 * Отправка ошибки в облако.
 * kind: "crash" | "unhandled" | "taskException" | "handled",
 * severity: "fatal" | "error".
 * Снимок буфера breadcrumbs прикладывается к отчёту автоматически.
 */
export async function trackError(report: ErrorReportInput): Promise<void> {
  try {
    await invoke('plugin:logs|track_error', {
      errorType: report.errorType,
      message: report.message,
      stack: report.stack ?? null,
      severity: report.severity ?? 'error',
      kind: report.kind ?? 'handled',
      breadcrumbs: [...breadcrumbs],
    })
  } catch {
    /* ignore */
  }
}

function defaultLogFilename(): string {
  const d = new Date()
  const pad = (n: number) => String(n).padStart(2, '0')
  const ts = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}_${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}`
  return `logs_${ts}.txt`
}

/** Диалог «Сохранить как» + запись текста лога в выбранный файл. Возвращает путь. */
export async function saveLogsToFile(content: string): Promise<string | null> {
  const path = await save({
    title: 'Сохранить логи',
    defaultPath: defaultLogFilename(),
    filters: [{ name: 'Текст', extensions: ['log', 'txt'] }],
  })
  if (!path) return null
  try {
    await invoke('plugin:logs|save_logs_file', { path, content })
    return path
  } catch {
    return null
  }
}

let captureAttached = false

// ── Flight-recorder фронта: кольцевой буфер хроники (breadcrumbs) ──
// Наполняется перехватом invoke (главное бутылочное горлышко IPC) и кликов.
// В облако/дамп не улетает в реальном времени: снимок прикладывается к отчёту
// об ошибке (см. trackError / initFrontendErrorCapture).
const BREADCRUMB_LIMIT = 100
const breadcrumbs: string[] = []

function stamp(): string {
  return new Date().toISOString().slice(11, 19)
}

function pushBreadcrumb(kind: string, detail: string): void {
  if (breadcrumbs.length >= BREADCRUMB_LIMIT) breadcrumbs.shift()
  breadcrumbs.push(`[${stamp()}] [${kind}] ${detail}`)
}

function takeBreadcrumbs(): string[] {
  return breadcrumbs.splice(0, breadcrumbs.length)
}

function truncate(s: string, n: number): string {
  return s.length > n ? `${s.slice(0, n)}…` : s
}

// ── Авто-перехват invoke: хроника IPC без единой правки в бизнес-логике ──
function installInvokeInterceptor(): void {
  const g = window as unknown as {
    __TAURI_INTERNALS__?: Record<string, unknown>
    __TAURI__?: { core?: Record<string, unknown> }
  }
  const MARKER = '__logs_breadcrumb_wrapped'

  const patch = (proto: Record<string, unknown> | undefined, key: string): void => {
    if (!proto) return
    const orig = proto[key]
    if (typeof orig !== 'function' || (orig as { [MARKER]?: boolean })[MARKER]) return
    const wrapped = async (...args: unknown[]): Promise<unknown> => {
      const cmd = args[0]
      const name = typeof cmd === 'string' ? cmd : String(cmd)
      // Команды собственного плагина не захламляют хронику.
      const skip = name.startsWith('plugin:logs|')
      const start = performance.now()
      if (!skip) {
        const payload = truncate(JSON.stringify(args[1] ?? {}), 300)
        pushBreadcrumb('IPC START', `${name} ${payload}`)
      }
      try {
        const result = await (orig as (...a: unknown[]) => Promise<unknown>).apply(proto, args)
        if (!skip) {
          const ms = (performance.now() - start).toFixed(1)
          pushBreadcrumb('IPC OK', `${name} (${ms}ms)`)
        }
        return result
      } catch (err) {
        const ms = (performance.now() - start).toFixed(1)
        const emsg = err instanceof Error ? err.message : String(err ?? 'unknown')
        if (!skip) {
          pushBreadcrumb('IPC ERROR', `${name} (${ms}ms) ${emsg}`)
          logFront(`[IPC ERROR] ${name} (${ms}ms) ${emsg}`)
        }
        throw err
      }
    }
    ;(wrapped as { [MARKER]?: boolean })[MARKER] = true
    proto[key] = wrapped
  }

  try {
    patch(g.__TAURI_INTERNALS__, 'invoke')
    patch(g.__TAURI__?.core, 'invoke')
  } catch {
    /* перехват invoke не критичен — буфер просто останется без IPC-хроники */
  }
}

// ── Авто-хроника действий в интерфейсе (клики в capture-фазе) ──
function installClickCapture(): void {
  document.addEventListener(
    'click',
    (e) => {
      const target = e.target as HTMLElement | null
      if (!target) return
      const id = target.id ? `#${target.id}` : ''
      const cls =
        typeof target.className === 'string' && target.className
          ? `.${target.className.split(' ')[0]}`
          : ''
      const text = (target.textContent ?? '').trim().slice(0, 40)
      pushBreadcrumb('UI ACTION', `<${target.tagName.toLowerCase()}${id}${cls}> ${truncate(text, 40)}`)
    },
    true,
  )
}

/** Сброс контекста при необработанной ошибке: breadcrumbs → crash_dump.log + облако. */
async function flushFrontendCrash(
  errorType: string,
  message: string,
  stack?: string,
): Promise<void> {
  const crumbs = takeBreadcrumbs()
  logFront(`[window error] ${message}`)
  try {
    await invoke('plugin:logs|dump_frontend_error', {
      errorType,
      message,
      stack: stack ?? null,
      breadcrumbs: crumbs,
    })
  } catch {
    /* сбой отчёта не должен ронять UI */
  }
}

/** Один раз на старте приложения: window.onerror + unhandledrejection → лог + облако + дамп. */
export function initFrontendErrorCapture(): void {
  if (captureAttached) return
  captureAttached = true

  installInvokeInterceptor()
  installClickCapture()

  window.addEventListener('error', (e) => {
    const msg = e.message ?? 'unknown error'
    const stack = e.error instanceof Error ? e.error.stack : undefined
    void flushFrontendCrash('Frontend Error', msg, stack)
  })

  window.addEventListener('unhandledrejection', (e) => {
    const reason = (e as PromiseRejectionEvent).reason
    let message = String(reason ?? 'unknown rejection')
    let stack: string | undefined
    if (reason instanceof Error) {
      message = reason.message
      stack = reason.stack
    }
    void flushFrontendCrash('Unhandled Promise Rejection', message, stack)
  })
}

// Web Component <logs-panel> регистрируется при импорте пакета.
import './web-components'