import {
  checkRouterUpdate,
  getCombos,
  getStatus,
  installOrUpdate,
  onProgress,
  openDashboard,
  setApiKey,
  setRouterDir,
  stop,
  type ComboInfo,
  type NineRouterStatus,
  type RouterId,
} from './index'
import { getUpdateState, onUpdateState, setUpdateState } from './updates'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { invoke } from '@tauri-apps/api/core'

const STYLE = `
  :host { display: block; color: var(--text, #e6e6e6); font-family: var(--font, system-ui, sans-serif); font-size: 13px; }
  .row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .row + .row { margin-top: 8px; }
  button { padding: 6px 10px; cursor: pointer; border-radius: 6px; border: 1px solid var(--border, #333);
           background: var(--session-hover, #2a2a2a); color: var(--text, #e6e6e6); font: inherit; font-size: 13px; }
  button.primary { background: var(--primary, #4a90d9); color: #fff; border-color: var(--primary, #4a90d9); }
  button:disabled { opacity: .5; cursor: default; }
  button.busy { position: relative; pointer-events: none; color: transparent !important; }
  button.busy::after { content: ''; position: absolute; top: 50%; left: 50%; width: 14px; height: 14px;
                        margin: -7px 0 0 -7px;
                        border: 2px solid var(--primary, #4a90d9); border-top-color: transparent; border-radius: 50%;
                        animation: spin 0.8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .field { margin-top: 8px; }
  .field input { flex: 1; padding: 6px 8px; border-radius: 6px; border: 1px solid var(--border, #333);
                 background: var(--session-hover, #2a2a2a); color: var(--text, #e6e6e6); font: inherit; font-size: 13px; }
  .api-ok { color: var(--success, #3fa45b); font-size: 12px; margin-left: 4px; }
  .api-err { color: var(--warning, #d8a13a); font-size: 12px; margin-left: 4px; }
  .status { display: flex; gap: 8px; align-items: center; }
  .dot { width: 9px; height: 9px; border-radius: 50%; background: var(--text-muted, #777); flex: 0 0 auto; }
  .dot.on { background: var(--success, #3fa45b); }
  .dot.warn { background: var(--warning, #d8a13a); }
  .update-badge { display: inline-block; width: 8px; height: 8px; border-radius: 50%;
                  background: #4caf50; margin-left: 6px; box-shadow: 0 0 6px #4caf50; }
  .muted { color: var(--text-muted, #999); font-size: 12px; }
  .path { color: var(--text-muted, #999); font-size: 12px; word-break: break-all; }
  .progress-container { display: none; margin-top: 10px; }
  .progress-container.on { display: block; }
  .progress-status { color: var(--text-muted, #999); font-size: 12px; margin-bottom: 5px; word-break: break-all; }
  .progress-track { height: 8px; border-radius: 6px; background: var(--border, #333); overflow: hidden; }
  .progress-bar { height: 100%; width: 0%; background: var(--primary, #4a90d9); transition: width .15s linear; }
  .warn-hint { color: var(--warning, #d8a13a); margin-top: 6px; }
  .combos { margin-top: 10px; display: none; }
  .combos.on { display: block; }
  .combo { padding: 4px 0; border-bottom: 1px solid var(--border, #333); }
  .combo:last-child { border-bottom: 0; }
  .combo .name { font-weight: 600; }
  .combo .models { color: var(--text-muted, #999); font-size: 12px; word-break: break-all; }
  .tabs { display: flex; gap: 4px; margin-bottom: 10px; }
  .tabs button { padding: 4px 10px; font-size: 12px; border-radius: 4px; }
  .tabs button.active { background: var(--primary, #4a90d9); color: #fff; border-color: var(--primary, #4a90d9); }
`

function esc(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;')
}

function toast(msg: string, kind: 'success' | 'error' = 'success'): void {
  const el = document.createElement('div')
  el.textContent = msg
  el.style.cssText =
    `position:fixed; right:16px; bottom:16px; z-index:5000; max-width:420px; padding:10px 14px;` +
    `border-radius:8px; font:13px var(--font, system-ui, sans-serif); box-shadow:0 3px 14px rgba(0,0,0,.4);` +
    `background:${kind === 'success' ? 'var(--primary, #4a90d9)' : 'var(--danger, #b54242)'}; color:#fff;`
  document.body.appendChild(el)
  setTimeout(() => el.remove(), 3500)
}

function logPlugin(msg: string): void {
  void invoke('plugin:logs|log_frontend_event', { level: 'FE', msg }).catch(() => {})
}

export class CloudRoutersPanel extends HTMLElement {
  private router: RouterId = '9router'
  private status: NineRouterStatus | null = null
  private combos: ComboInfo[] = []
  private offProgress: (() => void) | null = null
  private visibilityObserver: MutationObserver | null = null
  private refreshTimer: number | null = null
  private root: ShadowRoot
  private unsubUpdate?: () => void

  constructor() {
    super()
    this.root = this.attachShadow({ mode: 'open' })
  }

  connectedCallback() {
    this.render()
    this.unsubUpdate = onUpdateState((s) => this.renderUpdateState(s))
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

  private async refresh() {
    try {
      this.status = await getStatus(this.router)
    } catch (e) {
      this.status = null
      logPlugin(`[cloud-routers] getStatus failed: ${String(e)}`)
    }
    this.render()
  }

  private onProgress(text: string, done: number, total: number) {
    const bar = this.root.querySelector<HTMLElement>('.progress-bar')
    const label = this.root.querySelector<HTMLElement>('.progress-status')
    const box = this.root.querySelector<HTMLElement>('.progress-container')
    if (box) box.classList.add('on')
    if (label) label.textContent = text
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

  private notifyCombosChanged() {
    window.dispatchEvent(new CustomEvent('cloud-routers:combos-changed', {
      detail: { router: this.router, combos: this.combos },
    }))
  }

  private async onInstall() {
    this.setBtnBusy('.install', true)
    try {
      this.status = await installOrUpdate(this.router, true)
      this.combos = await getCombos(this.router).catch(() => [])
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
    this.notifyCombosChanged()
    this.render()
  }

  private async onOpen() {
    this.setBtnBusy('.open', true)
    try {
      await openDashboard(this.router)
      this.status = await getStatus(this.router).catch(() => this.status)
    } catch (e) {
      const msg = `Ошибка открытия Web UI: ${String(e)}`
      logPlugin(`[cloud-routers] ${msg}`)
      toast(msg, 'error')
    } finally {
      this.setBtnBusy('.open', false)
    }
    this.render()
  }

  private async onStop() {
    this.setBtnBusy('.stop', true)
    try {
      this.status = await stop(this.router)
      this.combos = []
      this.notifyCombosChanged()
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
    this.setBtnBusy('.refresh', true)
    try {
      this.combos = await getCombos(this.router)
      this.status = await getStatus(this.router)
    } catch (e) {
      const msg = `Ошибка обновления комбо: ${String(e)}`
      logPlugin(`[cloud-routers] ${msg}`)
      toast(msg, 'error')
    } finally {
      this.setBtnBusy('.refresh', false)
    }
    this.notifyCombosChanged()
    this.render()
  }

  private async onSaveApiKey() {
    const input = this.root.querySelector<HTMLInputElement>('.api-key-input')
    const ok = this.root.querySelector<HTMLElement>('.api-ok')
    const err = this.root.querySelector<HTMLElement>('.api-err')
    if (!input) return
    ok && (ok.textContent = '')
    err && (err.textContent = '')
    this.setBtnBusy('.save-key', true)
    try {
      this.status = await setApiKey(this.router, input.value.trim())
      ok && (ok.textContent = '✓ сохранён')
    } catch (e) {
      const msg = `Ошибка сохранения API-ключа: ${String(e)}`
      logPlugin(`[cloud-routers] ${msg}`)
      toast(msg, 'error')
      if (err) err.textContent = `Ошибка: ${String(e)}`
    } finally {
      this.setBtnBusy('.save-key', false)
    }
    this.render()
  }

  private async onSetDir() {
    this.setBtnBusy('.setdir', true)
    try {
      const sel = await openDialog({ directory: true })
      if (!sel) return
      const path = Array.isArray(sel) ? sel[0] : sel
      if (!path) return
      this.status = await setRouterDir(this.router, path)
      this.combos = []
      this.notifyCombosChanged()
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
    const btn = this.root.querySelector<HTMLButtonElement>('.check-update')
    const updateBtn = this.root.querySelector<HTMLButtonElement>('.install-update')
    if (btn) btn.disabled = true
    if (updateBtn) updateBtn.style.display = 'none'
    const label = this.root.querySelector<HTMLElement>('.progress-status')
    const box = this.root.querySelector<HTMLElement>('.progress-container')
    try {
      const newTag = await checkRouterUpdate(this.router)
      if (newTag) {
        if (box) box.classList.add('on')
        if (label) label.textContent = `Доступно обновление: v${newTag}`
        if (updateBtn) updateBtn.style.display = 'inline-block'
      } else {
        if (box) box.classList.add('on')
        if (label) label.textContent = `Актуален${this.status?.version ? ` (v${this.status.version})` : ''}`
      }
    } catch (e) {
      const msg = `Ошибка проверки обновления: ${String(e)}`
      logPlugin(`[cloud-routers] ${msg}`)
      toast(msg, 'error')
      if (box) box.classList.add('on')
      if (label) label.textContent = `Ошибка проверки обновления: ${String(e)}`
    } finally {
      if (btn) btn.disabled = false
    }
  }

  private async onInstallUpdate() {
    await this.onInstall()
    setUpdateState({ hasUpdate: false })
    const updateBtn = this.root.querySelector<HTMLButtonElement>('.install-update')
    if (updateBtn) updateBtn.style.display = 'none'
  }

  private switchRouter(router: RouterId) {
    this.router = router
    this.status = null
    this.combos = []
    void this.refresh()
    this.render()
  }

  private render() {
    const s = this.status
    const installed = s?.installed ?? false
    const running = s?.running ?? false
    const dotClass = installed && running ? 'dot on' : installed ? 'dot warn' : 'dot'
    const message = s?.message ?? 'Загрузка...'
    const version = s?.version ? `v${s.version}` : '—'
    const nodeVersion = s?.node_version ? `Node ${s.node_version}` : 'Node —'

    this.root.innerHTML = `
      <style>${STYLE}</style>
      <div class="tabs">
        <button class="${this.router === '9router' ? 'active' : ''}" data-router="9router">9Router</button>
        <button class="${this.router === 'extremerouter' ? 'active' : ''}" data-router="extremerouter">ExtremeRouter</button>
        <button class="${this.router === 'omniroute' ? 'active' : ''}" data-router="omniroute">OmniRoute</button>
      </div>
      <div class="status">
        <span class="${dotClass}"></span>
        <span>${message}</span>
        <span id="updateBadge" class="update-badge" style="display:none;"></span>
      </div>
      <div class="row muted">
        <span>${this.router}: ${version}</span>
        <span>·</span>
        <span>${nodeVersion}</span>
        <span>·</span>
        <span>порт ${s?.port ?? '—'}</span>
      </div>
      <div class="row">
        <button class="primary install" style="${installed ? 'display:none;' : ''}">Установить</button>
        <button class="check-update" style="${installed ? '' : 'display:none;'}">Проверить обновление</button>
        <button class="primary install-update" style="display:none;">Обновить</button>
        <button class="open" ${installed ? '' : 'disabled'}>Открыть Web UI</button>
        <button class="stop" ${running ? '' : 'disabled'}>Остановить</button>
        <button class="refresh" ${installed ? '' : 'disabled'}>⟳ Обновить комбо</button>
        <button class="setdir">Изменить путь</button>
      </div>
      <div class="row muted">
        <span>Каталог базы данных: ${s?.data_dir ? esc(s.data_dir) : '—'}</span>
      </div>
      <div class="row muted">
        <span>Статус БД: ${s?.db_present ? 'найдена' : '⚠ не найдена'}</span>
      </div>
      ${!installed && (s?.node_present || s?.server_present)
        ? '<div class="muted warn-hint">Частичная установка: найдены не все компоненты. Нажмите «Установить», чтобы починить.</div>'
        : ''}
      <div class="progress-container">
        <div class="progress-status"></div>
        <div class="progress-track"><div class="progress-bar"></div></div>
      </div>
      <div class="combos ${this.combos.length ? 'on' : ''}">
        ${this.combos
          .map(
            (c) =>
              `<div class="combo"><div class="name">${esc(c.name)}</div>` +
              `<div class="models">${esc(c.models.join(', '))}</div></div>`,
          )
          .join('')}
      </div>
      <div class="row path">Путь установки программы: ${s?.path ? esc(s.path) : '—'}</div>
    `

    this.root.querySelectorAll<HTMLButtonElement>('.tabs button').forEach((b) => {
      b.addEventListener('click', () => this.switchRouter(b.dataset.router as RouterId))
    })
    this.root.querySelector('.install')?.addEventListener('click', () => void this.onInstall())
    this.root.querySelector('.check-update')?.addEventListener('click', () => void this.onCheckUpdate())
    this.root.querySelector('.install-update')?.addEventListener('click', () => void this.onInstallUpdate())
    this.root.querySelector('.stop')?.addEventListener('click', () => void this.onStop())
    this.root.querySelector('.refresh')?.addEventListener('click', () => void this.onShowCombos())
    this.root.querySelector('.open')?.addEventListener('click', () => void this.onOpen())
    this.root.querySelector('.setdir')?.addEventListener('click', () => void this.onSetDir())
    this.renderUpdateState(getUpdateState())
  }

  private renderUpdateState(s: { hasUpdate: boolean; tag?: string }) {
    const badge = this.root.querySelector<HTMLElement>('#updateBadge')
    const updateBtn = this.root.querySelector<HTMLElement>('.install-update')
    if (s.hasUpdate) {
      if (badge) badge.style.display = 'inline-block'
      if (updateBtn) updateBtn.style.display = 'inline-block'
    } else if (badge) {
      badge.style.display = 'none'
    }
  }
}

if (!customElements.get('cloud-routers-panel')) {
  customElements.define('cloud-routers-panel', CloudRoutersPanel)
}
