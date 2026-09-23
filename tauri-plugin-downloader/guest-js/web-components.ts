import { cancelDownload, onProgress, type DownloadProgress } from './index'

const STATUS_LABEL: Record<string, string> = {
  running: 'Скачивание…',
  done: 'Готово',
  error: 'Ошибка',
  cancelled: 'Отменено',
}

function formatBytes(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  let i = 0
  let v = n
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024
    i++
  }
  return `${v >= 10 || i === 0 ? Math.round(v) : v.toFixed(1)} ${units[i]}`
}

function formatSpeed(bps: number): string {
  if (!Number.isFinite(bps) || bps <= 0) return '—'
  return `${formatBytes(bps)}/s`
}

function formatEta(sec: number): string {
  if (!Number.isFinite(sec) || sec < 0) return '—'
  if (sec < 60) return `${Math.max(1, Math.round(sec))} с`
  const m = Math.floor(sec / 60)
  const s = Math.round(sec % 60)
  if (m < 60) return `${m} мин ${s} с`
  const h = Math.floor(m / 60)
  return `${h} ч ${m % 60} мин`
}

function pct(p: DownloadProgress): number {
  if (!p.total || p.total <= 0) return 0
  return Math.min(100, Math.max(0, (p.downloaded / p.total) * 100))
}

interface RowState {
  p: DownloadProgress
  terminalAt: number
}

/** Общая логика строк прогресса для виджета и встраиваемой панели. */
function createRowsController(render: (rows: Map<string, RowState>) => void) {
  const rows = new Map<string, RowState>()
  let unlisten: (() => void) | null = null
  let hideTimer: number | null = null

  const purgeTerminal = (): void => {
    const now = Date.now()
    for (const [id, row] of rows) {
      if (row.p.status !== 'running' && now - row.terminalAt > 4000) {
        rows.delete(id)
      }
    }
    if (rows.size === 0 && hideTimer !== null) {
      window.clearTimeout(hideTimer)
      hideTimer = null
    }
    render(rows)
  }

  const ensureListen = (): void => {
    if (unlisten) return
    void onProgress((p) => {
      const prev = rows.get(p.task_id)
      if (p.status === 'running') {
        rows.set(p.task_id, { p, terminalAt: 0 })
        if (hideTimer !== null) {
          window.clearTimeout(hideTimer)
          hideTimer = null
        }
      } else {
        rows.set(p.task_id, { p, terminalAt: Date.now() })
        if (hideTimer === null) {
          hideTimer = window.setTimeout(() => {
            hideTimer = null
            purgeTerminal()
          }, 4200)
        }
      }
      void prev
      render(rows)
    }).then((fn) => {
      unlisten = fn
    })
  }

  const destroy = (): void => {
    if (hideTimer !== null) window.clearTimeout(hideTimer)
    if (unlisten) {
      unlisten()
      unlisten = null
    }
    rows.clear()
    render(rows)
  }

  return { ensureListen, destroy, rows }
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

function rowsHtml(rows: Map<string, RowState>, compact: boolean): string {
  if (rows.size === 0) return ''
  let html = ''
  for (const { p } of rows.values()) {
    const percent = pct(p)
    const known = p.total > 0
    const statusText = STATUS_LABEL[p.status] ?? p.status
    const meta = known
      ? `${formatBytes(p.downloaded)} / ${formatBytes(p.total)} · ${formatSpeed(p.speed_bps)} · осталось ${formatEta(p.eta_s)}`
      : `${formatBytes(p.downloaded)} · ${formatSpeed(p.speed_bps)}`
    html += `<div class="dl-row${p.status !== 'running' ? ` dl-${p.status}` : ''}" data-task="${escapeHtml(p.task_id)}">
      <div class="dl-head">
        <span class="dl-label" title="${escapeHtml(p.label)}">${escapeHtml(p.label)}</span>
        <span class="dl-meta">
          <span class="dl-level">${escapeHtml(p.level_name || statusText)}</span>
          ${known ? `<span class="dl-pct">${percent.toFixed(0)}%</span>` : ''}
          <span class="dl-status">${escapeHtml(p.status === 'running' ? statusText : p.message || statusText)}</span>
        </span>
      </div>
      <div class="dl-bar"><div class="dl-fill" style="width:${known ? percent.toFixed(1) : 0}%"></div></div>
      ${compact ? '' : `<div class="dl-sub">${escapeHtml(meta)}</div>`}
      ${p.status === 'running' ? `<button type="button" class="dl-cancel" data-cancel="${escapeHtml(p.task_id)}" title="Отменить">×</button>` : ''}
    </div>`
  }
  return html
}

const HOST_STYLE = `
:host { display: block; color: inherit; font: inherit; }
.dl-list { display: flex; flex-direction: column; gap: 8px; }
.dl-row {
  position: relative;
  background: color-mix(in srgb, Canvas 92%, CanvasText 8%);
  border: 1px solid color-mix(in srgb, CanvasText 18%, transparent);
  border-radius: 8px;
  padding: 8px 34px 8px 10px;
}
.dl-head { display: flex; justify-content: space-between; gap: 8px; align-items: baseline; }
.dl-label { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.dl-meta { display: flex; gap: 8px; font-size: 0.85em; opacity: 0.75; flex-shrink: 0; }
.dl-bar {
  margin-top: 6px;
  height: 6px;
  border-radius: 4px;
  overflow: hidden;
  background: color-mix(in srgb, CanvasText 12%, transparent);
}
.dl-fill {
  height: 100%;
  background: #3b82f6;
  transition: width 150ms linear;
}
.dl-done .dl-fill { background: #22c55e; }
.dl-error .dl-fill { background: #ef4444; }
.dl-cancelled .dl-fill { background: #a3a3a3; }
.dl-sub { margin-top: 4px; font-size: 0.8em; opacity: 0.65; }
.dl-cancel {
  position: absolute;
  top: 6px;
  right: 6px;
  width: 22px;
  height: 22px;
  border: none;
  border-radius: 50%;
  cursor: pointer;
  background: transparent;
  color: inherit;
  font-size: 16px;
  line-height: 1;
  opacity: 0.55;
}
.dl-cancel:hover { opacity: 1; background: color-mix(in srgb, CanvasText 10%, transparent); }
`

function attachCancelClicks(host: ShadowRoot | HTMLElement, onId: (id: string) => void): void {
  host.addEventListener('click', (e) => {
    const btn = (e.target as HTMLElement | null)?.closest('[data-cancel]') as HTMLElement | null
    const id = btn?.dataset?.cancel
    if (id) onId(id)
  })
}

/** Глобальный плавающий виджет: все активные загрузки. */
class DownloaderWidget extends HTMLElement {
  private ctrl: ReturnType<typeof createRowsController> | null = null
  private shadow: ShadowRoot | null = null

  connectedCallback(): void {
    if (!this.shadow) this.shadow = this.attachShadow({ mode: 'open' })
    if (this.ctrl) return
    const style = document.createElement('style')
    style.textContent = HOST_STYLE + `
:host { position: fixed; right: 16px; bottom: 16px; z-index: 9999; max-width: min(420px, calc(100vw - 32px)); }
.dl-list { min-width: 280px; }
`
    this.shadow.appendChild(style)
    const list = document.createElement('div')
    list.className = 'dl-list'
    this.shadow.appendChild(list)
    attachCancelClicks(this.shadow, (id) => void cancelDownload(id))
    this.ctrl = createRowsController((rows) => {
      list.innerHTML = rowsHtml(rows, true)
      this.style.display = rows.size === 0 ? 'none' : ''
    })
    this.style.display = 'none'
    this.ctrl.ensureListen()
  }

  disconnectedCallback(): void {
    this.ctrl?.destroy()
    this.ctrl = null
    if (this.shadow) this.shadow.innerHTML = ''
  }
}

/** Встраиваемая панель (в настройки/панель llama-engine). */
class DownloaderProgress extends HTMLElement {
  private ctrl: ReturnType<typeof createRowsController> | null = null
  private shadow: ShadowRoot | null = null
  /** Через атрибут kind фильтруются категории ("engine,model"). */
  private filterKinds: string[] = []

  static get observedAttributes(): string[] {
    return ['kind', 'label']
  }

  connectedCallback(): void {
    if (!this.shadow) this.shadow = this.attachShadow({ mode: 'open' })
    if (this.ctrl) return
    const style = document.createElement('style')
    style.textContent = HOST_STYLE
    this.shadow.appendChild(style)
    const list = document.createElement('div')
    list.className = 'dl-list'
    this.shadow.appendChild(list)
    attachCancelClicks(this.shadow, (id) => void cancelDownload(id))
    this.ctrl = createRowsController((rows) => {
      const filtered = this.filterRows(rows)
      list.innerHTML = filtered.size === 0
        ? '<div class="dl-empty" style="opacity:.55;font-size:.9em">Нет активных загрузок</div>'
        : rowsHtml(filtered, false)
    })
    this.ctrl.ensureListen()
    this.ctrl && this.render()
  }

  attributeChangedCallback(): void {
    this.syncFilter()
    this.render()
  }

  disconnectedCallback(): void {
    this.ctrl?.destroy()
    this.ctrl = null
    if (this.shadow) this.shadow.innerHTML = ''
  }

  private syncFilter(): void {
    const kind = this.getAttribute('kind')
    this.filterKinds = kind ? kind.split(',').map((s) => s.trim()).filter(Boolean) : []
  }

  private filterRows(rows: Map<string, RowState>): Map<string, RowState> {
    const label = this.getAttribute('label')?.trim()
    if (!this.filterKinds.length && !label) return rows
    const out = new Map<string, RowState>()
    for (const [id, row] of rows) {
      const kindOk =
        !this.filterKinds.length || this.filterKinds.includes(row.p.kind)
      const labelOk = !label || row.p.label === label
      if (kindOk && labelOk) out.set(id, row)
    }
    return out
  }

  private render(): void {
    if (!this.ctrl) return
    const list = this.shadow?.querySelector('.dl-list')
    if (list) list.innerHTML = rowsHtml(this.filterRows(this.ctrl.rows), false)
  }
}

if (typeof customElements !== 'undefined' && !customElements.get('downloader-widget')) {
  customElements.define('downloader-widget', DownloaderWidget)
}
if (typeof customElements !== 'undefined' && !customElements.get('downloader-progress')) {
  customElements.define('downloader-progress', DownloaderProgress)
}
