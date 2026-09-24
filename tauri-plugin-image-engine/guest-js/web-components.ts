import {
  checkImageEngineUpdate,
  downloadImageBundle,
  estimateImageMemory,
  getImageBundleInfo,
  getImageEngineStatus,
  installImageEngine,
  installImageEngineUpdate,
  notifyBundleChanged,
  removeImageBundle,
  removeImageEngine,
  setImageBundleDir,
  setImageEngineDir,
  setImageEngineVariant,
  validateImageBundleDir,
  type ImageBundleInfo,
  type ImageBundleValidate,
  type ImageEngineStatus,
} from './index'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { listen } from '@tauri-apps/api/event'

const STYLE = `
  :host { display: block; color: var(--text, #eee); font-family: var(--font, system-ui, sans-serif); font-size: 13px; }
  .row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .row + .row { margin-top: 8px; }
  label { color: var(--text-muted, #999); min-width: 60px; }
  button { padding: 6px 10px; cursor: pointer; border-radius: 6px; border: 1px solid var(--border, #333);
           background: var(--session-hover, #eee); color: var(--text, #eee); font: inherit; font-size: 13px; }
  button.primary { background: var(--primary, #4a90d9); color: #fff; border-color: var(--primary, #4a90d9); }
  button.danger { background: var(--danger, #b54242); color: #fff; border-color: var(--danger, #b54242); }
  button:disabled { opacity: .5; cursor: default; }
  select, input[type=text] { padding: 6px; border-radius: 6px; border: 1px solid var(--border, #333);
           background: var(--bg-elevated, #1c1c1c); color: var(--text, #eee); font: inherit; font-size: 13px; }
  .progress-container { display: none; margin-top: 10px; }
  .progress-status { color: var(--text-muted, #999); font-size: 12px; margin-bottom: 5px; word-break: break-all; }
  .progress-track { height: 8px; border-radius: 6px; background: var(--border, #333); overflow: hidden; }
  .progress-bar { height: 100%; width: 0%; background: var(--primary, #4a90d9); transition: width .1s linear; }
  .hint { color: var(--text-muted, #999); font-size: 12px; white-space: pre-line; }
  .files { margin: 6px 0 0 0; padding-left: 18px; }
  .files li { margin: 2px 0; }
  .ok { color: #6fbf73; }
  .missing { color: var(--text-muted, #999); }
  .overlay { position: fixed; inset: 0; background: rgba(0,0,0,.5); display: none; align-items: center;
             justify-content: center; z-index: 2000; }
  .overlay.open { display: flex; }
  .box { background: var(--bg-elevated, #1c1c1c); border: 1px solid var(--border, #333); border-radius: 12px;
         padding: 20px; max-width: 520px; width: 90%; }
  .box h3 { margin: 0 0 12px; font-size: 16px; }
  .box p { margin: 8px 0; color: var(--text-muted, #aaa); }
  .overlay-buttons { display: flex; gap: 10px; justify-content: flex-end; margin-top: 16px; }
`

function esc(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;')
}

function formatBytes(n: number): string {
  if (!isFinite(n) || n <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  let i = 0
  while (n >= 1024 && i < units.length - 1) { n /= 1024; i++ }
  return `${n.toFixed(1)} ${units[i]}`
}

/** Лёгкий всплывающий тост (без внешних зависимостей; использует CSS-переменные хоста). */
function toast(msg: string, kind: 'success' | 'error' = 'success') {
  const el = document.createElement('div')
  el.textContent = msg
  el.style.cssText =
    `position:fixed; right:16px; bottom:16px; z-index:5000; max-width:420px; padding:10px 14px;` +
    `border-radius:8px; font-size:13px; color:#fff; ` +
    `background:${kind === 'success' ? 'rgba(60,140,70,.95)' : 'rgba(180,60,60,.95)'};`
  document.body.appendChild(el)
  setTimeout(() => el.remove(), 4200)
}

// ─────────────────────────────────────────────────────────────────────────────
// <image-engine-panel> — статус/установка движка sd.cpp
// ─────────────────────────────────────────────────────────────────────────────

class ImageEnginePanel extends HTMLElement {
  private root!: ShadowRoot
  private busy = false
  private applied = 'auto'
  private unlisten?: () => void

  connectedCallback() {
    if (this.root) return
    this.root = this.attachShadow({ mode: 'open' })
    this.root.innerHTML = `
      <style>${STYLE}</style>
      <div>
        <div class="row"><label>Статус:</label><span id="status" class="hint">Проверка…</span></div>
        <div class="row"><label>GPU:</label><span id="gpu" class="hint"></span></div>
        <div class="row"><label>Путь:</label><span id="path" class="hint" style="word-break:break-all; flex:1;"></span></div>
        <div class="row" style="margin-top:8px;">
          <label>Бекенд:</label>
          <select id="variant" style="flex:1; min-width:200px;"><option value="">…</option></select>
          <button id="apply" class="primary" style="display:none;">Применить</button>
        </div>
        <div id="variantHint" class="hint"></div>
        <div class="row" style="margin-top:10px;">
          <button id="install" class="primary">Установить</button>
          <button id="checkUpdate" class="secondary" style="display:none;">Проверить обновление</button>
          <button id="installUpdate" class="primary" style="display:none;">Обновить</button>
          <button id="remove" class="danger" style="display:none;">Удалить</button>
          <button id="setDir" class="secondary" style="display:none;">Изменить путь</button>
        </div>
        <div id="progress" class="progress-container">
          <div class="progress-status" id="progressStatus">0 MB / 0 MB</div>
          <div class="progress-track"><div class="progress-bar" id="progressBar"></div></div>
        </div>
        <div id="overlay" class="overlay">
          <div class="box">
            <h3>Удалить движок изображений?</h3>
            <p>GPU-ускорение генерации отключится.</p>
            <div class="overlay-buttons">
              <button id="delCancel" class="secondary">Отмена</button>
              <button id="delOk" class="danger">Удалить</button>
            </div>
          </div>
        </div>
      </div>`

    this.root.getElementById('variant')!.addEventListener('change', () => this.onVariantChange())
    this.root.getElementById('apply')!.addEventListener('click', () => this.onApplyVariant())
    this.root.getElementById('install')!.addEventListener('click', () => this.onInstall())
    this.root.getElementById('checkUpdate')!.addEventListener('click', () => this.onCheckUpdate())
    this.root.getElementById('installUpdate')!.addEventListener('click', () => this.onInstallUpdate())
    this.root.getElementById('remove')!.addEventListener('click', () => {
      this.root.getElementById('overlay')!.classList.add('open')
    })
    this.root.getElementById('delCancel')!.addEventListener('click', () => {
      this.root.getElementById('overlay')!.classList.remove('open')
    })
    this.root.getElementById('delOk')!.addEventListener('click', () => {
      this.root.getElementById('overlay')!.classList.remove('open')
      void this.onRemove()
    })
    this.root.getElementById('setDir')!.addEventListener('click', () => this.onSetDir())

    void this.refresh()
    void listen('downloader:progress', (e) => {
      const p = (e as { payload: { downloaded: number; total: number; kind?: string; status?: string } }).payload
      if (p.kind && !['engine', 'image-model'].includes(p.kind)) return
      this.setProgress(p.downloaded, p.total)
      if (p.status === 'done' || p.status === 'error') {
        setTimeout(() => this.showProgress(false), 400)
      }
    }).then((u) => { this.unlisten = u }).catch(() => {})
  }

  disconnectedCallback() {
    this.unlisten?.()
  }

  private setStatus(s: string) {
    const el = this.root.getElementById('status')
    if (el) el.textContent = s
  }

  private setProgress(downloaded: number, total: number) {
    const pct = total > 0 ? (downloaded / total) * 100 : 0
    const bar = this.root.getElementById('progressBar') as HTMLElement
    bar.style.width = `${pct}%`
    const st = this.root.getElementById('progressStatus')!
    st.textContent = `${formatBytes(downloaded)} / ${total > 0 ? formatBytes(total) : '?'}${total > 0 ? ` (${pct.toFixed(0)}%)` : ''}`
  }

  private showProgress(on: boolean) {
    this.root.getElementById('progress')!.style.display = on ? 'block' : 'none'
  }

  private hintText(st: ImageEngineStatus, value: string): string {
    const lines: string[] = []
    if (value === 'auto') {
      const resolved = (st.available_variants || []).find((v) => v.id === st.resolved_variant)
      lines.push(`Авто-подбор для этой машины: ${resolved ? resolved.label : (st.resolved_variant || '—')}.`)
    } else {
      const v = (st.available_variants || []).find((x) => x.id === value)
      if (v && v.note) lines.push(v.note)
    }
    const installed = st.installed_variants || []
    lines.push(installed.length > 0
      ? `Установлены: ${installed.join(', ')}.`
      : 'Ни один бекенд ещё не установлен.')
    return lines.join('\n')
  }

  private async refresh() {
    let st: ImageEngineStatus
    try {
      st = await getImageEngineStatus()
    } catch (e) {
      this.setStatus(`Ошибка: ${e}`)
      return
    }
    this.setStatus(st.message)
    this.root.getElementById('gpu')!.textContent = st.has_nvidia ? st.gpu_name || 'NVIDIA' : 'Не обнаружена (CPU-режим)'
    this.root.getElementById('path')!.textContent = st.path || '—'

    const sel = this.root.getElementById('variant') as HTMLSelectElement
    sel.innerHTML = ''
    const auto = document.createElement('option')
    auto.value = 'auto'
    auto.textContent = 'Авто (рекомендуется)'
    sel.appendChild(auto)
    for (const v of st.available_variants || []) {
      const o = document.createElement('option')
      o.value = v.id
      o.textContent = v.installed ? `${v.label} — установлен` : v.recommended ? `${v.label} (рекомендуется)` : v.label
      sel.appendChild(o)
    }
    this.applied = st.selected_variant || 'auto'
    sel.value = this.applied
    this.root.getElementById('variantHint')!.textContent = this.hintText(st, sel.value)
    this.root.getElementById('apply')!.style.display = sel.value === this.applied ? 'none' : 'inline-block'
    this.applyButtonStates(st)
  }

  private applyButtonStates(st: ImageEngineStatus) {
    const installed = st.installed
    this.root.getElementById('install')!.style.display = installed ? 'none' : 'inline-block'
    this.root.getElementById('checkUpdate')!.style.display = installed ? 'inline-block' : 'none'
    this.root.getElementById('remove')!.style.display = installed ? 'inline-block' : 'none'
    this.root.getElementById('setDir')!.style.display = 'inline-block'
  }

  private async onVariantChange() {
    const sel = this.root.getElementById('variant') as HTMLSelectElement
    const value = sel.value
    this.root.getElementById('apply')!.style.display = value === this.applied ? 'none' : 'inline-block'
    try {
      const s = await getImageEngineStatus()
      this.root.getElementById('variantHint')!.textContent = this.hintText(s, value)
    } catch { /* не критично */ }
  }

  private async onApplyVariant() {
    const btn = this.root.getElementById('apply') as HTMLButtonElement
    const variant = (this.root.getElementById('variant') as HTMLSelectElement).value
    btn.disabled = true
    try {
      let installedVariants: string[] = []
      try { installedVariants = (await getImageEngineStatus()).installed_variants || [] } catch { /* не критично */ }
      if (variant !== 'auto' && !installedVariants.includes(variant)) {
        this.showProgress(true)
        this.setProgress(0, 0)
      }
      await setImageEngineVariant(variant)
      toast(variant === 'auto' ? 'Бекенд: авто-подбор.' : 'Бекенд применён.')
      await this.refresh()
    } catch (e) {
      toast(`Ошибка смены бекенда: ${e}`, 'error')
    } finally {
      btn.disabled = false
      this.showProgress(false)
      await this.refresh()
    }
  }

  private async onInstall() {
    const btn = this.root.getElementById('install') as HTMLButtonElement
    btn.disabled = true
    this.showProgress(true)
    this.root.getElementById('progressStatus')!.textContent = 'Подготовка…'
    try {
      await installImageEngine()
      await this.refresh()
      toast('Движок изображений установлен!')
    } catch (e) {
      toast(`Ошибка установки движка: ${e}`, 'error')
    } finally {
      btn.disabled = false
      this.showProgress(false)
    }
  }

  private async onCheckUpdate() {
    const btn = this.root.getElementById('checkUpdate') as HTMLButtonElement
    btn.disabled = true
    const prevLabel = btn.textContent
    btn.textContent = 'Проверка…'
    this.root.getElementById('installUpdate')!.style.display = 'none'
    try {
      const newTag = await checkImageEngineUpdate()
      if (newTag) {
        this.setStatus(`Доступно обновление движка: ${newTag}`)
        this.root.getElementById('installUpdate')!.style.display = 'inline-block'
      } else {
        this.setStatus('Движок изображений актуален')
      }
    } catch (e) {
      this.setStatus('')
      toast(`Ошибка проверки обновления движка: ${e}`, 'error')
    } finally {
      btn.disabled = false
      btn.textContent = prevLabel || 'Проверить обновление'
    }
  }

  private async onInstallUpdate() {
    const btn = this.root.getElementById('installUpdate') as HTMLButtonElement
    btn.disabled = true
    this.showProgress(true)
    this.root.getElementById('progressStatus')!.textContent = 'Обновление…'
    try {
      await installImageEngineUpdate()
      this.root.getElementById('installUpdate')!.style.display = 'none'
      await this.refresh()
      toast('Движок изображений обновлён.', 'success')
    } catch (e) {
      toast(`Ошибка обновления движка: ${e}`, 'error')
    } finally {
      btn.disabled = false
      this.showProgress(false)
    }
  }

  private async onRemove() {
    if (this.busy) return
    this.busy = true
    try {
      await removeImageEngine()
      await this.refresh()
      toast('Движок изображений удалён.')
    } catch (e) {
      toast(`Ошибка удаления движка: ${e}`, 'error')
      void this.refresh()
    } finally {
      this.busy = false
    }
  }

  private async onSetDir() {
    try {
      const sel = await openDialog({ directory: true })
      if (!sel) return
      const path = Array.isArray(sel) ? sel[0] : sel
      if (!path) return
      await setImageEngineDir(path)
      await this.refresh()
      toast('Путь движка изменён.')
    } catch (e) {
      toast(`Ошибка изменения пути: ${e}`, 'error')
    }
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// <image-bundle-panel> — скачивание бандла весов (4 файла, одна кнопка)
// ─────────────────────────────────────────────────────────────────────────────

class ImageBundlePanel extends HTMLElement {
  private root!: ShadowRoot
  private busy = false
  private unlisten?: () => void

  connectedCallback() {
    if (this.root) return
    this.root = this.attachShadow({ mode: 'open' })
    this.root.innerHTML = `
      <style>${STYLE}</style>
      <div>
        <div class="row"><label>Набор:</label><span id="label" class="hint">…</span></div>
        <div id="note" class="hint"></div>
        <ul class="files" id="files"></ul>
        <div class="row" style="margin-top:8px;"><label>Диск:</label><span id="disk" class="hint"></span></div>
        <div class="row"><label>Память:</label><span id="mem" class="hint"></span></div>
        <div class="row"><label>Папка:</label><span id="dir" class="hint" style="word-break:break-all; flex:1;"></span></div>
        <div class="row" style="margin-top:10px;">
          <button id="download" class="primary">Скачать набор</button>
          <button id="remove" class="danger" style="display:none;">Удалить файлы</button>
        </div>
        <div id="progress" class="progress-container">
          <div class="progress-status" id="progressStatus">0 MB / 0 MB</div>
          <div class="progress-track"><div class="progress-bar" id="progressBar"></div></div>
        </div>
      </div>`

    this.root.getElementById('download')!.addEventListener('click', () => this.onDownload())
    this.root.getElementById('remove')!.addEventListener('click', () => this.onRemove())

    void this.refresh()
    void listen('downloader:progress', (e) => {
      const p = (e as { payload: { downloaded: number; total: number; kind?: string; status?: string } }).payload
      if (p.kind && p.kind !== 'image-model') return
      this.setProgress(p.downloaded, p.total)
      if (p.status === 'done' || p.status === 'error') {
        setTimeout(() => { this.showProgress(false); void this.refresh() }, 400)
      }
    }).then((u) => { this.unlisten = u }).catch(() => {})
  }

  disconnectedCallback() {
    this.unlisten?.()
  }

  private setProgress(downloaded: number, total: number) {
    const pct = total > 0 ? (downloaded / total) * 100 : 0
    const bar = this.root.getElementById('progressBar') as HTMLElement
    bar.style.width = `${pct}%`
    const st = this.root.getElementById('progressStatus')!
    st.textContent = `${formatBytes(downloaded)} / ${total > 0 ? formatBytes(total) : '?'}${total > 0 ? ` (${pct.toFixed(0)}%)` : ''}`
  }

  private showProgress(on: boolean) {
    this.root.getElementById('progress')!.style.display = on ? 'block' : 'none'
  }

  private async refresh() {
    let info: ImageBundleInfo
    try {
      info = await getImageBundleInfo()
    } catch (e) {
      this.root.getElementById('label')!.textContent = `Ошибка: ${e}`
      return
    }
    this.root.getElementById('label')!.textContent = info.label
    this.root.getElementById('note')!.textContent = info.note || ''
    const ul = this.root.getElementById('files')!
    ul.innerHTML = ''
    let anyExists = false
    for (const f of info.files) {
      if (f.exists) anyExists = true
      const li = document.createElement('li')
      li.className = f.exists ? 'ok' : 'missing'
      const size = f.size_bytes ? ` — ${formatBytes(f.size_bytes)}` : ''
      li.textContent = `${f.exists ? '✓' : '○'} ${esc(f.filename)} (${f.role})${size}`
      ul.appendChild(li)
    }
    const totalGb = (info.total_bytes / 1024 / 1024 / 1024).toFixed(1)
    this.root.getElementById('disk')!.textContent =
      `~${totalGb} ГБ на диске · свободно ${info.free_space_gb} ГБ`
    this.root.getElementById('mem')!.textContent =
      `GPU: ${info.vram_fast_gb ?? '?'} ГБ (быстро) / ${info.vram_min_gb ?? '?'} ГБ (мин.) · RAM ≥${info.ram_min_gb ?? '?'} ГБ`
    this.root.getElementById('dir')!.textContent = info.save_dir
    const dlBtn = this.root.getElementById('download') as HTMLButtonElement
    dlBtn.style.display = info.fully_downloaded ? 'none' : 'inline-block'
    dlBtn.textContent = anyExists ? 'Докачать файлы' : 'Скачать набор'
    this.root.getElementById('remove')!.style.display = info.fully_downloaded ? 'inline-block' : 'none'
    try {
      const mem = await estimateImageMemory()
      const gb = (mb: number) => `${(mb / 1024).toFixed(1)} ГБ`
      this.root.getElementById('mem')!.textContent =
        `GPU: ${info.vram_fast_gb ?? '?'} ГБ (быстро) / ${info.vram_min_gb ?? '?'} ГБ (мин.) · ` +
        `сейчас свободно ${gb(mem.estimate.vram_free_mb)} VRAM / ${gb(mem.estimate.ram_free_mb)} RAM — ${mem.message}`
    } catch { /* NVML может отсутствовать — не критично */ }
  }

  private async onDownload() {
    if (this.busy) return
    this.busy = true
    const btn = this.root.getElementById('download') as HTMLButtonElement
    btn.disabled = true
    this.showProgress(true)
    this.setProgress(0, 0)
    try {
      const cur = await getImageBundleInfo()
      let target = cur.save_dir
      const anyExists = cur.files.some((f) => f.exists)
      if (!anyExists || !target) {
        const sel = await openDialog({ directory: true, title: 'Куда скачать набор?' })
        if (!sel) return
        const base = Array.isArray(sel) ? sel[0] : sel
        if (!base) return
        const sep = base.includes('\\') ? '\\' : '/'
        const tail = base.split(/[\\/]/).pop() ?? ''
        target = tail === cur.bundle_name ? base : (base.endsWith(sep) ? `${base}${cur.bundle_name}` : `${base}${sep}${cur.bundle_name}`)
      }
      await downloadImageBundle(target)
      notifyBundleChanged()
      toast('Набор изображений скачан!')
    } catch (e) {
      toast(`Ошибка скачивания набора: ${e}`, 'error')
    } finally {
      this.busy = false
      btn.disabled = false
      this.showProgress(false)
      await this.refresh()
    }
  }

  private async onRemove() {
    if (this.busy) return
    this.busy = true
    try {
      await removeImageBundle()
      notifyBundleChanged()
      toast('Файлы набора удалены.')
    } catch (e) {
      toast(`Ошибка удаления: ${e}`, 'error')
    } finally {
      this.busy = false
      await this.refresh()
    }
  }
}

class ImageModelsPanel extends HTMLElement {
  private root!: ShadowRoot
  private busy = false
  private unlisten?: () => void

  connectedCallback() {
    if (this.root) return
    this.root = this.attachShadow({ mode: 'open' })
    this.root.innerHTML = `
      <style>${STYLE}</style>
      <div>
        <div class="row"><label>Папка:</label><span id="dir" class="hint" style="word-break:break-all; flex:1;">…</span></div>
        <div id="status" class="hint"></div>
        <ul class="files" id="files"></ul>
        <div class="row" style="margin-top:10px;">
          <button id="add" class="secondary">+ Добавить набор</button>
          <button id="resume" class="primary" style="display:none;">Докачать файлы</button>
        </div>
        <div id="progress" class="progress-container">
          <div class="progress-status" id="progressStatus">0 MB / 0 MB</div>
          <div class="progress-track"><div class="progress-bar" id="progressBar"></div></div>
        </div>
      </div>`
    this.root.getElementById('add')!.addEventListener('click', () => this.onAdd())
    this.root.getElementById('resume')!.addEventListener('click', () => this.onResume())
    document.addEventListener('image:bundle-changed', this.onChangedBound)
    void this.refresh()

    void listen('downloader:progress', (e) => {
      const p = (e as { payload: { downloaded: number; total: number; kind?: string; status?: string } }).payload
      if (p.kind && p.kind !== 'image-model') return
      this.setProgress(p.downloaded, p.total)
      if (p.status === 'done' || p.status === 'error') {
        setTimeout(() => { this.showProgress(false); void this.refresh() }, 400)
      }
    }).then((u) => { this.unlisten = u }).catch(() => {})
  }

  disconnectedCallback() {
    document.removeEventListener('image:bundle-changed', this.onChangedBound)
    this.unlisten?.()
  }

  private onChangedBound = () => { void this.refresh() }

  private setProgress(downloaded: number, total: number) {
    const pct = total > 0 ? (downloaded / total) * 100 : 0
    const bar = this.root.getElementById('progressBar') as HTMLElement
    if (bar) bar.style.width = `${pct}%`
    const st = this.root.getElementById('progressStatus')
    if (st) st.textContent = `${formatBytes(downloaded)} / ${total > 0 ? formatBytes(total) : '?'}${total > 0 ? ` (${pct.toFixed(0)}%)` : ''}`
  }

  private showProgress(on: boolean) {
    const prg = this.root.getElementById('progress')
    if (prg) prg.style.display = on ? 'block' : 'none'
  }

  private async refresh() {
    let info: ImageBundleInfo
    try {
      info = await getImageBundleInfo()
    } catch (e) {
      this.root.getElementById('status')!.textContent = `Ошибка: ${e}`
      return
    }
    this.root.getElementById('dir')!.textContent = info.save_dir
    const ul = this.root.getElementById('files')!
    ul.innerHTML = ''
    for (const f of info.files) {
      const li = document.createElement('li')
      li.className = f.exists ? 'ok' : 'missing'
      const size = f.size_bytes ? ` — ${formatBytes(f.size_bytes)}` : ''
      li.textContent = `${f.exists ? '✓' : '○'} ${esc(f.filename)} (${f.role})${size}`
      ul.appendChild(li)
    }
    const missing = info.files.filter((f) => !f.exists).length
    this.root.getElementById('status')!.textContent =
      info.fully_downloaded ? `Набор валиден: ${info.files.length}/${info.files.length} файла на месте.` : `Не хватает файлов: ${missing} из ${info.files.length}.`
    const resumeBtn = this.root.getElementById('resume') as HTMLButtonElement
    if (resumeBtn) {
      resumeBtn.style.display = info.fully_downloaded ? 'none' : 'inline-block'
    }
  }

  private async onResume() {
    if (this.busy) return
    this.busy = true
    const btn = this.root.getElementById('resume') as HTMLButtonElement
    if (btn) btn.disabled = true
    this.showProgress(true)
    this.setProgress(0, 0)
    try {
      const cur = await getImageBundleInfo()
      await downloadImageBundle(cur.save_dir)
      notifyBundleChanged()
      toast('Недостающие файлы набора скачаны!')
    } catch (e) {
      toast(`Ошибка докачивания набора: ${e}`, 'error')
    } finally {
      this.busy = false
      if (btn) btn.disabled = false
      this.showProgress(false)
      await this.refresh()
    }
  }

  private async onAdd() {
    if (this.busy) return
    this.busy = true
    const btn = this.root.getElementById('add') as HTMLButtonElement
    btn.disabled = true
    try {
      const sel = await openDialog({ directory: true, title: 'Папка набора ImageGEN' })
      if (!sel) return
      const path = Array.isArray(sel) ? sel[0] : sel
      if (!path) return
      let v: ImageBundleValidate
      try {
        v = await validateImageBundleDir(path)
      } catch (e) {
        toast(`Ошибка проверки набора: ${e}`, 'error')
        return
      }
      await setImageBundleDir(path)
      notifyBundleChanged()
      if (v.valid) {
        toast('Набор добавлен: все файлы на месте.')
      } else {
        toast(`Папка выбрана. Не хватает файлов: ${v.missing.length}. Нажмите «Докачать файлы».`)
      }
      await this.refresh()
    } catch (e) {
      toast(`Не удалось добавить набор: ${e}`, 'error')
    } finally {
      btn.disabled = false
      this.busy = false
    }
  }
}

if (!customElements.get('image-engine-panel')) {
  customElements.define('image-engine-panel', ImageEnginePanel)
}

if (!customElements.get('image-bundle-panel')) {
  customElements.define('image-bundle-panel', ImageBundlePanel)
}

if (!customElements.get('image-models-panel')) {
  customElements.define('image-models-panel', ImageModelsPanel)
}
