import {
  ensureStarted,
  getCombos,
  getStatus,
  installOrUpdate,
  onProgress,
  openDashboard,
  ROUTER_IDS,
  setApiKey,
  setRouterDir,
  stop,
  type ComboInfo,
  type RouterId,
  type RouterStatus,
} from './index'
import { checkUpdate, getUpdateState, onUpdateState, setUpdateState } from './updates'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { invoke } from '@tauri-apps/api/core'

const STYLE = `
  :host {
    --bg-base: #0c0e14;
    --bg-surface: #141721;
    --bg-surface-hover: #1b202e;
    --bg-card: rgba(22, 27, 38, 0.75);
    --border-subtle: rgba(255, 255, 255, 0.07);
    --border-strong: rgba(255, 255, 255, 0.14);
    
    --text-main: #f1f5f9;
    --text-muted: #94a3b8;
    --text-dim: #64748b;
    
    --accent: #6366f1;
    --accent-hover: #4f46e5;
    --accent-glow: rgba(99, 102, 241, 0.25);
    
    --success: #10b981;
    --success-bg: rgba(16, 185, 129, 0.12);
    --success-glow: rgba(16, 185, 129, 0.35);
    
    --warning: #f59e0b;
    --warning-bg: rgba(245, 158, 11, 0.12);
    
    --danger: #ef4444;
    --danger-bg: rgba(239, 68, 68, 0.12);

    display: block;
    color: var(--text-main);
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Inter', sans-serif;
    font-size: 13px;
    line-height: 1.5;
    box-sizing: border-box;
    padding: 16px;
    background: radial-gradient(circle at top right, rgba(99, 102, 241, 0.05), transparent 40%), var(--bg-base);
    border-radius: 12px;
  }

  *, *::before, *::after {
    box-sizing: border-box;
  }

  /* ── Табы роутеров ── */
  .tabs-nav {
    display: flex;
    gap: 6px;
    background: rgba(0, 0, 0, 0.25);
    padding: 4px;
    border-radius: 10px;
    border: 1px solid var(--border-subtle);
    margin-bottom: 16px;
    overflow-x: auto;
  }
  .tabs-nav button {
    flex: 1;
    min-width: 90px;
    background: transparent;
    border: 1px solid transparent;
    color: var(--text-muted);
    font-weight: 500;
    font-size: 13px;
    padding: 7px 14px;
    border-radius: 8px;
    cursor: pointer;
    transition: all 0.2s cubic-bezier(0.4, 0, 0.2, 1);
    white-space: nowrap;
    text-align: center;
  }
  .tabs-nav button:hover {
    color: var(--text-main);
    background: rgba(255, 255, 255, 0.04);
  }
  .tabs-nav button.active {
    background: var(--bg-surface);
    color: #fff;
    border-color: var(--border-strong);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3), 0 0 1px rgba(255, 255, 255, 0.2);
  }

  /* ── Карточка Статуса (Hero) ── */
  .status-hero {
    background: var(--bg-card);
    backdrop-filter: blur(12px);
    border: 1px solid var(--border-subtle);
    border-radius: 12px;
    padding: 16px;
    margin-bottom: 14px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.2);
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .status-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
  }
  .status-title-row {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .router-name {
    font-size: 17px;
    font-weight: 700;
    letter-spacing: -0.02em;
    color: #fff;
  }
  .status-pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    border-radius: 9999px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    border: 1px solid var(--border-subtle);
    background: var(--bg-surface);
  }
  .status-pill.online {
    color: var(--success);
    background: var(--success-bg);
    border-color: rgba(16, 185, 129, 0.25);
  }
  .status-pill.warn {
    color: var(--warning);
    background: var(--warning-bg);
    border-color: rgba(245, 158, 11, 0.25);
  }
  .status-pill.offline {
    color: var(--text-dim);
  }

  .pulse-dot {
    position: relative;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: currentColor;
  }
  .status-pill.online .pulse-dot::after {
    content: '';
    position: absolute;
    inset: -3px;
    border-radius: 50%;
    background: var(--success);
    opacity: 0.75;
    animation: ping 2s cubic-bezier(0, 0, 0.2, 1) infinite;
  }
  @keyframes ping {
    75%, 100% {
      transform: scale(2.2);
      opacity: 0;
    }
  }

  .status-message {
    font-size: 13px;
    color: var(--text-muted);
    display: flex;
    align-items: center;
    gap: 8px;
  }

  /* ── Сетка параметров (Grid Specs) ── */
  .specs-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(130px, 1fr));
    gap: 8px;
    margin-bottom: 14px;
  }
  .spec-item {
    background: var(--bg-surface);
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    padding: 8px 10px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .spec-label {
    font-size: 10.5px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-dim);
    font-weight: 600;
  }
  .spec-value {
    font-size: 12.5px;
    color: var(--text-main);
    font-weight: 500;
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* ── Экшены и Кнопки ── */
  .actions-bar {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
    margin-bottom: 14px;
  }
  
  button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 7px 13px;
    cursor: pointer;
    border-radius: 7px;
    border: 1px solid var(--border-subtle);
    background: var(--bg-surface);
    color: var(--text-main);
    font-family: inherit;
    font-size: 12.5px;
    font-weight: 500;
    transition: all 0.15s ease;
    user-select: none;
  }
  button:hover:not(:disabled) {
    background: var(--bg-surface-hover);
    border-color: var(--border-strong);
    transform: translateY(-1px);
  }
  button:active:not(:disabled) {
    transform: translateY(0);
  }
  button:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  
  button.primary {
    background: linear-gradient(135deg, var(--accent), #4338ca);
    border-color: rgba(255, 255, 255, 0.15);
    color: #fff;
    box-shadow: 0 2px 10px var(--accent-glow);
  }
  button.primary:hover:not(:disabled) {
    background: linear-gradient(135deg, #4f46e5, #3730a3);
    box-shadow: 0 4px 14px var(--accent-glow);
  }
  
  button.start {
    background: linear-gradient(135deg, #10b981, #059669);
    border-color: rgba(255, 255, 255, 0.1);
    color: #fff;
    box-shadow: 0 2px 8px var(--success-glow);
  }
  button.start:hover:not(:disabled) {
    background: linear-gradient(135deg, #059669, #047857);
  }
  
  button.stop:hover:not(:disabled) {
    background: var(--danger-bg);
    border-color: rgba(239, 68, 68, 0.3);
    color: #fca5a5;
  }

  button.busy {
    position: relative;
    pointer-events: none;
    color: transparent !important;
  }
  button.busy svg { opacity: 0; }
  button.busy::after {
    content: '';
    position: absolute;
    width: 14px;
    height: 14px;
    border: 2px solid #ffffff;
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 0.7s linear infinite;
  }

  /* ── Предупреждения и Баннеры ── */
  .banner {
    padding: 10px 12px;
    border-radius: 8px;
    margin-bottom: 12px;
    font-size: 12px;
    line-height: 1.4;
    display: flex;
    align-items: flex-start;
    gap: 8px;
  }
  .banner-warn {
    background: var(--warning-bg);
    border: 1px solid rgba(245, 158, 11, 0.25);
    color: #fbbf24;
  }
  .banner-info {
    background: rgba(99, 102, 241, 0.08);
    border: 1px solid rgba(99, 102, 241, 0.2);
    color: #c7d2fe;
  }
  .banner code {
    background: rgba(0, 0, 0, 0.35);
    padding: 1px 5px;
    border-radius: 4px;
    font-family: ui-monospace, monospace;
    font-size: 11px;
    color: #fff;
  }

  /* ── Прогресс ── */
  .progress-container {
    display: none;
    background: var(--bg-surface);
    border: 1px solid var(--border-subtle);
    padding: 10px;
    border-radius: 8px;
    margin-bottom: 12px;
  }
  .progress-container.on { display: block; }
  .progress-status {
    color: var(--text-muted);
    font-size: 12px;
    margin-bottom: 6px;
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  .progress-track {
    height: 6px;
    border-radius: 6px;
    background: rgba(255, 255, 255, 0.06);
    overflow: hidden;
  }
  .progress-bar {
    height: 100%;
    width: 0%;
    background: linear-gradient(90deg, var(--accent), #a855f7);
    border-radius: 6px;
    box-shadow: 0 0 10px rgba(168, 85, 247, 0.5);
    transition: width 0.2s ease;
  }

  /* ── Комбо / Доступные роуты ── */
  .combos-wrapper {
    display: none;
    background: var(--bg-card);
    border: 1px solid var(--border-subtle);
    border-radius: 10px;
    padding: 12px;
    margin-bottom: 14px;
  }
  .combos-wrapper.on { display: block; }
  .combos-title {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    font-weight: 700;
    color: var(--text-dim);
    margin-bottom: 8px;
  }
  .combo-item {
    padding: 8px 0;
    border-bottom: 1px solid var(--border-subtle);
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .combo-item:last-child { border-bottom: 0; padding-bottom: 0; }
  .combo-header {
    font-weight: 600;
    color: #fff;
    font-size: 13px;
  }
  .combo-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .model-chip {
    font-family: ui-monospace, monospace;
    font-size: 11px;
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid var(--border-subtle);
    color: var(--text-muted);
    padding: 2px 6px;
    border-radius: 4px;
  }

  /* ── Футер с путями ── */
  .path-box {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 11px;
    color: var(--text-dim);
    padding: 6px 10px;
    background: rgba(0, 0, 0, 0.2);
    border-radius: 6px;
    border: 1px solid rgba(255, 255, 255, 0.03);
    word-break: break-all;
    font-family: ui-monospace, monospace;
  }

  .spinner {
    display: inline-block;
    width: 12px;
    height: 12px;
    border: 2px solid var(--accent);
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin { to { transform: rotate(360deg); } }

  .update-badge {
    background: var(--success);
    color: #000;
    font-size: 10px;
    font-weight: 700;
    padding: 2px 6px;
    border-radius: 999px;
    animation: pulse-update 1.5s infinite;
  }
  @keyframes pulse-update {
    0%, 100% { opacity: 1; transform: scale(1); }
    50% { opacity: 0.8; transform: scale(0.96); }
  }
`

function esc(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;')
}

function toast(msg: string, kind: 'success' | 'error' = 'success'): void {
  const el = document.createElement('div')
  el.textContent = msg
  el.style.cssText =
    `position:fixed; right:20px; bottom:20px; z-index:9999; max-width:400px; padding:10px 16px;` +
    `border-radius:10px; font: 500 13px system-ui, sans-serif; backdrop-filter: blur(12px);` +
    `box-shadow:0 12px 30px rgba(0,0,0,.5), 0 0 0 1px rgba(255,255,255,0.1);` +
    `background:${kind === 'success' ? 'rgba(16, 185, 129, 0.95)' : 'rgba(239, 68, 68, 0.95)'}; color:#fff;` +
    `animation: toast-in 0.2s cubic-bezier(0.16, 1, 0.3, 1);`
  document.body.appendChild(el)
  setTimeout(() => {
    el.style.opacity = '0'
    el.style.transition = 'opacity 0.25s ease'
    setTimeout(() => el.remove(), 250)
  }, 3200)
}

function logPlugin(msg: string): void {
  void invoke('plugin:logs|log_frontend_event', { level: 'FE', msg }).catch(() => { })
}

const ROUTER_LABELS: Record<RouterId, string> = {
  '9router': '9Router',
  extremerouter: 'ExtremeRouter',
  omniroute: 'OmniRoute',
  gateway: 'Gateway',
}

const NATIVE_GATEWAY: RouterId = 'gateway'
const isNativeGateway = (r: RouterId): boolean => r === NATIVE_GATEWAY

type RouterState =
  | { kind: 'loading' }
  | { kind: 'error'; message: string }
  | { kind: 'ready'; status: RouterStatus }

export class CloudRoutersPanel extends HTMLElement {
  private router: RouterId = '9router'
  private states = new Map<RouterId, RouterState>()
  private combosByRouter = new Map<RouterId, ComboInfo[]>()
  private offProgress: (() => void) | null = null
  private visibilityObserver: MutationObserver | null = null
  private refreshTimer: number | null = null
  private root: ShadowRoot
  private unsubUpdate?: () => void

  constructor() {
    super()
    this.root = this.attachShadow({ mode: 'open' })
  }

  private stateOf(router: RouterId): RouterState {
    return this.states.get(router) ?? { kind: 'loading' }
  }

  private get status(): RouterStatus | null {
    const s = this.stateOf(this.router)
    return s.kind === 'ready' ? s.status : null
  }

  private get combos(): ComboInfo[] {
    return this.combosByRouter.get(this.router) ?? []
  }

  private setStatus(router: RouterId, status: RouterStatus): void {
    this.states.set(router, { kind: 'ready', status })
  }

  private setCombos(router: RouterId, combos: ComboInfo[]): void {
    this.combosByRouter.set(router, combos)
  }

  connectedCallback() {
    this.render()
    this.unsubUpdate = onUpdateState(() => this.renderUpdateState(getUpdateState(this.router)))
    void this.refresh()
    onProgress((p) => {
      if (p.router === this.router) this.onProgress(p.text, p.done, p.total)
    }).then((off) => {
      this.offProgress = off
    })
    this.observeVisibility()
    this.refreshTimer = window.setInterval(() => void this.refresh(), 15000)
  }

  disconnectedCallback() {
    this.unsubUpdate?.()
    this.offProgress?.()
    this.offProgress = null
    this.visibilityObserver?.disconnect()
    this.visibilityObserver = null
    if (this.refreshTimer !== null) {
      clearInterval(this.refreshTimer)
      this.refreshTimer = null
    }
  }

  private observeVisibility() {
    let host: HTMLElement | null = this
    while (host && host !== document.body && !host.classList?.contains('view')) {
      host = host.parentElement
    }
    if (!host || host === document.body) return
    this.visibilityObserver = new MutationObserver(() => {
      if (host.classList.contains('active')) void this.refresh()
    })
    this.visibilityObserver.observe(host, { attributes: true, attributeFilter: ['class'] })
  }

  private async refresh(target: RouterId = this.router) {
    if (!this.states.has(target)) {
      this.states.set(target, { kind: 'loading' })
      this.render()
    }
    try {
      this.states.set(target, { kind: 'ready', status: await getStatus(target) })
    } catch (e) {
      const msg = `Не удалось получить статус ${target}: ${String(e)}`
      logPlugin(`[cloud-routers] ${msg}`)
      toast(msg, 'error')
      this.states.set(target, { kind: 'error', message: msg })
    }
    this.render()
  }

  private onProgress(text: string, done: number, total: number) {
    const bar = this.root.querySelector<HTMLElement>('.progress-bar')
    const label = this.root.querySelector<HTMLElement>('.progress-status')
    const box = this.root.querySelector<HTMLElement>('.progress-container')
    if (box) box.classList.add('on')
    const pct = total > 0 ? `${Math.min(100, (done / total) * 100).toFixed(0)}%` : '...'
    if (label) label.innerHTML = `<span>${esc(text)}</span><span>${pct}</span>`
    if (bar) bar.style.width = total > 0 ? `${Math.min(100, (done / total) * 100).toFixed(1)}%` : '100%'
    if (total > 0 && done >= total) {
      logPlugin(`[cloud-routers] установка завершена: ${text}`)
      void this.refresh()
    }
  }

  private setBtnBusy(selector: string, v: boolean) {
    const btn = this.root.querySelector<HTMLButtonElement>(selector)
    if (btn) {
      btn.disabled = v
      btn.classList.toggle('busy', v)
    }
  }

  private notifyCombosChanged(router: RouterId = this.router) {
    window.dispatchEvent(
      new CustomEvent('cloud-routers:combos-changed', {
        detail: { router, combos: this.combosByRouter.get(router) ?? [] },
      }),
    )
  }

  private async onInstall() {
    const router = this.router
    this.setBtnBusy('.install', true)
    try {
      this.setStatus(router, await installOrUpdate(router, true))
      this.setCombos(router, await getCombos(router).catch(() => []))
    } catch (e) {
      const msg = `Ошибка установки: ${String(e)}`
      logPlugin(`[cloud-routers] ${msg}`)
      toast(msg, 'error')
      const label = this.root.querySelector<HTMLElement>('.progress-status')
      const box = this.root.querySelector<HTMLElement>('.progress-container')
      if (box) box.classList.add('on')
      if (label) label.textContent = `Ошибка: ${String(e)}`
      return
    } finally {
      this.setBtnBusy('.install', false)
    }
    this.notifyCombosChanged(router)
    this.render()
  }

  private async onOpen() {
    const router = this.router
    this.setBtnBusy('.open', true)
    try {
      await openDashboard(router)
      const fresh = await getStatus(router).catch(() => null)
      if (fresh) this.setStatus(router, fresh)
    } catch (e) {
      const msg = `Ошибка открытия Web UI: ${String(e)}`
      logPlugin(`[cloud-routers] ${msg}`)
      toast(msg, 'error')
    } finally {
      this.setBtnBusy('.open', false)
    }
    this.render()
  }

  private async onStart() {
    const router = this.router
    this.setBtnBusy('.start', true)
    try {
      const status = await ensureStarted(router)
      this.setStatus(router, status)
      this.setCombos(router, await getCombos(router).catch(() => []))
    } catch (e) {
      const msg = `Ошибка запуска: ${String(e)}`
      logPlugin(`[cloud-routers] ${msg}`)
      toast(msg, 'error')
    } finally {
      this.setBtnBusy('.start', false)
    }
    this.notifyCombosChanged(router)
    this.render()
  }

  private async onStop() {
    const router = this.router
    this.setBtnBusy('.stop', true)
    try {
      this.setStatus(router, await stop(router))
      this.setCombos(router, [])
      this.notifyCombosChanged(router)
    } catch (e) {
      const msg = `Ошибка остановки: ${String(e)}`
      logPlugin(`[cloud-routers] ${msg}`)
      toast(msg, 'error')
    } finally {
      this.setBtnBusy('.stop', false)
    }
    this.render()
  }

  private async onShowCombos() {
    const router = this.router
    this.setBtnBusy('.refresh', true)
    try {
      this.setCombos(router, await getCombos(router))
      this.setStatus(router, await getStatus(router))
    } catch (e) {
      const msg = `Ошибка обновления комбо: ${String(e)}`
      logPlugin(`[cloud-routers] ${msg}`)
      toast(msg, 'error')
    } finally {
      this.setBtnBusy('.refresh', false)
    }
    this.notifyCombosChanged(router)
    this.render()
  }

  private async onSetDir() {
    const router = this.router
    this.setBtnBusy('.setdir', true)
    try {
      const sel = await openDialog({ directory: true })
      if (!sel) return
      const path = Array.isArray(sel) ? sel[0] : sel
      if (!path) return
      this.setStatus(router, await setRouterDir(router, path))
      this.setCombos(router, [])
      this.notifyCombosChanged(router)
      this.render()
    } catch (e) {
      const msg = `Ошибка смены пути: ${String(e)}`
      logPlugin(`[cloud-routers] ${msg}`)
      toast(msg, 'error')
      const label = this.root.querySelector<HTMLElement>('.progress-status')
      const box = this.root.querySelector<HTMLElement>('.progress-container')
      if (box) box.classList.add('on')
      if (label) label.textContent = `Ошибка: ${String(e)}`
    } finally {
      this.setBtnBusy('.setdir', false)
    }
  }

  private async onCheckUpdate() {
    const router = this.router
    const btn = this.root.querySelector<HTMLButtonElement>('.check-update')
    const label = this.root.querySelector<HTMLElement>('.progress-status')
    const box = this.root.querySelector<HTMLElement>('.progress-container')
    if (btn) btn.disabled = true
    this.renderUpdateState({ hasUpdate: false })
    try {
      const next = await checkUpdate(router)
      if (box) box.classList.add('on')
      if (label) {
        label.textContent = next.hasUpdate
          ? `🔥 Доступно обновление: v${next.tag ?? ''}`
          : `Установлена актуальная версия ${this.status?.version ? `(v${this.status.version})` : ''}`
      }
    } catch (e) {
      const msg = `Ошибка проверки обновления: ${String(e)}`
      logPlugin(`[cloud-routers] ${msg}`)
      toast(msg, 'error')
      if (box) box.classList.add('on')
      if (label) label.textContent = `Ошибка проверки: ${String(e)}`
    } finally {
      if (btn) btn.disabled = false
    }
  }

  private async onInstallUpdate() {
    const router = this.router
    await this.onInstall()
    setUpdateState(router, { hasUpdate: false })
  }

  private switchRouter(router: RouterId) {
    if (router === this.router) return
    this.router = router
    this.render()
    void this.refresh(router)
  }

  private render() {
    const state = this.stateOf(this.router)
    const known = state.kind === 'ready'
    const s = this.status
    const installed = s?.installed ?? false
    const running = s?.running ?? false
    const native = isNativeGateway(this.router)

    // Pill UI статуса
    let statusPill = `<span class="status-pill offline"><span class="pulse-dot"></span> Остановлен</span>`
    if (!known) {
      statusPill = `<span class="status-pill"><span class="spinner"></span> Проверка</span>`
    } else if (installed && running) {
      statusPill = `<span class="status-pill online"><span class="pulse-dot"></span> В сети</span>`
    } else if (installed && !running) {
      statusPill = `<span class="status-pill warn"><span class="pulse-dot"></span> Готов к запуску</span>`
    }

    const message = known
      ? s!.message
      : state.kind === 'error'
        ? state.message
        : 'Синхронизация состояния шлюза…'

    const version = s?.version ? `v${s.version}` : '—'
    const port = s?.port ? `${s.port}` : '—'
    const dbStatus = s?.has_npm_runtime ? (s?.db_present ? 'Подключена' : 'Не найдена') : 'N/A'

    // Генерация кнопок действий
    const actions = known
      ? `
        ${s?.can_install ? `<button class="primary install" style="${installed ? 'display:none;' : ''}">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
          Установить
        </button>` : ''}

        ${!running ? `<button class="start" ${installed ? '' : 'disabled'}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><polygon points="5 3 19 12 5 21 5 3"/></svg>
          Запустить
        </button>` : ''}

        <button class="stop" ${running ? '' : 'disabled'}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><rect x="4" y="4" width="16" height="16" rx="2"/></svg>
          Остановить
        </button>

        <button class="open" ${installed ? '' : 'disabled'}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></svg>
          Web UI
        </button>

        ${s?.can_check_update ? `
          <button class="check-update" style="${installed ? '' : 'display:none;'}">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21.5 2v6h-6M21.34 15.57a10 10 0 1 1-.57-8.38l5.67-5.67"/></svg>
            Обновления
          </button>
        ` : ''}

        <button class="primary install-update" style="display:none;">Обновить сейчас</button>

        <button class="refresh" ${installed ? '' : 'disabled'} title="Перечитать конфигурацию">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/><path d="M8 16H3v5"/></svg>
          Комбо
        </button>

        <button class="setdir" title="Изменить директорию установки">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
          Путь
        </button>
      `
      : `<span style="color:var(--text-dim)"><span class="spinner"></span> Загрузка действий...</span>`

    this.root.innerHTML = `
      <style>${STYLE}</style>

      <!-- Табы -->
      <div class="tabs-nav">
        ${ROUTER_IDS.map(
      (id) => `<button class="${this.router === id ? 'active' : ''}" data-router="${id}">${ROUTER_LABELS[id]}</button>`,
    ).join('')}
      </div>

      <!-- Главная статус-карточка -->
      <div class="status-hero">
        <div class="status-header">
          <div class="status-title-row">
            <span class="router-name">${ROUTER_LABELS[this.router]}</span>
            ${statusPill}
            <span id="updateBadge" class="update-badge" style="display:none;">UPDATE</span>
          </div>
          <span style="font-family:ui-monospace, monospace; font-size:12px; color:var(--text-dim);">${version}</span>
        </div>
        <div class="status-message">
          <span>${message}</span>
        </div>
      </div>

      <!-- Сетка параметров -->
      <div class="specs-grid">
        <div class="spec-item">
          <span class="spec-label">Порт</span>
          <span class="spec-value" style="color: #38bdf8;">${port}</span>
        </div>
        ${s?.has_npm_runtime ? `
          <div class="spec-item">
            <span class="spec-label">Runtime</span>
            <span class="spec-value">${s?.node_version ? `Node ${s.node_version}` : '—'}</span>
          </div>
          <div class="spec-item">
            <span class="spec-label">База данных</span>
            <span class="spec-value" style="color:${s?.db_present ? 'var(--success)' : 'var(--warning)'};">${dbStatus}</span>
          </div>
        ` : ''}
        <div class="spec-item" style="grid-column: span 2;">
          <span class="spec-label">Каталог данных</span>
          <span class="spec-value" title="${s?.data_dir ?? ''}">${s?.data_dir ? esc(s.data_dir) : 'Не задан'}</span>
        </div>
      </div>

      <!-- Тулбар действий -->
      <div class="actions-bar">
        ${actions}
      </div>

      <!-- Уведомления/Баннеры -->
      ${native ? `
        <div class="banner banner-info">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="flex-shrink:0;margin-top:2px;"><circle cx="12" cy="12" r="10"/><line x1="12" y1="16" x2="12" y2="12"/><line x1="12" y1="8" x2="12.01" y2="8"/></svg>
          <div>
            Шлюз настраивается через собственный UI. Жми <b>«Web UI»</b> или открой в браузере:
            <code>${s?.base_url ? esc(s.base_url) : 'http://localhost:' + port}/dashboard</code>
          </div>
        </div>
      ` : ''}

      ${!installed && !native && (s?.node_present || s?.server_present) ? `
        <div class="banner banner-warn">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="flex-shrink:0;margin-top:2px;"><path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3Z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
          <div>Неполная установка: отсутствуют компоненты пакета. Нажмите <b>«Установить»</b>, чтобы восстановить целостность.</div>
        </div>
      ` : ''}

      <!-- Прогресс установки / загрузки -->
      <div class="progress-container">
        <div class="progress-status"><span>Подготовка...</span><span>0%</span></div>
        <div class="progress-track"><div class="progress-bar"></div></div>
      </div>

      <!-- Маппинг комбо-моделей -->
      <div class="combos-wrapper ${this.combos.length ? 'on' : ''}">
        <div class="combos-title">Активные связки моделей (${this.combos.length})</div>
        ${this.combos.map((c) => `
          <div class="combo-item">
            <div class="combo-header">${esc(c.name)}</div>
            <div class="combo-chips">
              ${c.models.map(m => `<span class="model-chip">${esc(m)}</span>`).join('')}
            </div>
          </div>
        `).join('')}
      </div>

      <!-- Путь к бинарнику/установке -->
      <div class="path-box" title="${s?.path ? esc(s.path) : ''}">
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/></svg>
        <span>Путь: ${s?.path ? esc(s.path) : 'Не определен'}</span>
      </div>
    `

    // Слушатели событий
    this.root.querySelectorAll<HTMLButtonElement>('.tabs-nav button').forEach((b) => {
      b.addEventListener('click', () => this.switchRouter(b.dataset.router as RouterId))
    })
    this.root.querySelector('.install')?.addEventListener('click', () => void this.onInstall())
    this.root.querySelector('.check-update')?.addEventListener('click', () => void this.onCheckUpdate())
    this.root.querySelector('.install-update')?.addEventListener('click', () => void this.onInstallUpdate())
    this.root.querySelector('.start')?.addEventListener('click', () => void this.onStart())
    this.root.querySelector('.stop')?.addEventListener('click', () => void this.onStop())
    this.root.querySelector('.refresh')?.addEventListener('click', () => void this.onShowCombos())
    this.root.querySelector('.open')?.addEventListener('click', () => void this.onOpen())
    this.root.querySelector('.setdir')?.addEventListener('click', () => void this.onSetDir())

    this.renderUpdateState(getUpdateState(this.router))
  }

  private renderUpdateState(s: { hasUpdate: boolean; tag?: string }) {
    const badge = this.root.querySelector<HTMLElement>('#updateBadge')
    const updateBtn = this.root.querySelector<HTMLElement>('.install-update')
    const display = s.hasUpdate ? 'inline-flex' : 'none'
    if (badge) badge.style.display = display
    if (updateBtn) updateBtn.style.display = display
  }
}

if (!customElements.get('cloud-routers-panel')) {
  customElements.define('cloud-routers-panel', CloudRoutersPanel)
}