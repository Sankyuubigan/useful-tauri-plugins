import {
  autoDownloadDefaultModel,
  checkEngineUpdate,
  deleteModelFile,
  downloadModel,
  getAutoDownloadInfo,
  getEngineConfig,
  getEngineStatus,
  getModelsCatalog,
  getAllCapabilities,
  installEngineUpdate,
  installLlamaCpp,
  addModel,
  notifyModelsChanged,
  removeEngine,
  removeModel,
  setEngineDir,
  setEngineVariant,
  type CatalogEntry,
  type EngineConfig,
  type EngineStatus,
} from './index'
import { open as openDialog, save } from '@tauri-apps/plugin-dialog'
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
  .bar-row { display: flex; justify-content: space-between; align-items: center; gap: 8px; flex-wrap: wrap; }
  .badge { font-size: 12px; vertical-align: middle; cursor: help; }
  .hint { color: var(--text-muted, #999); font-size: 12px; white-space: pre-line; }
  .overlay { position: fixed; inset: 0; background: rgba(0,0,0,.5); display: none; align-items: center;
             justify-content: center; z-index: 2000; }
  .overlay.open { display: flex; }
  .box { background: var(--bg-elevated, #1c1c1c); border: 1px solid var(--border, #333); border-radius: 12px;
         padding: 20px; max-width: 520px; width: 90%; }
  .box h3 { margin: 0 0 12px; font-size: 16px; }
  .box p { margin: 8px 0; color: var(--text-muted, #aaa); }
  .box .strong { color: var(--text, #eee); word-break: break-all; }
  .overlay-buttons { display: flex; gap: 10px; justify-content: flex-end; margin-top: 16px; }
  .empty { color: var(--text-muted, #888); font-size: 13px; }
`

function esc(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;')
}

function fileName(p: string): string {
  const parts = p.split(/[/\\]/)
  return parts[parts.length - 1] || p
}

function formatBytes(n: number): string {
  if (!isFinite(n) || n <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  let i = 0
  while (n >= 1024 && i < units.length - 1) { n /= 1024; i++ }
  return `${n.toFixed(1)} ${units[i]}`
}

/** Лёгкий всплывающий toст (без внешних зависимостей; использует CSS-переменные хоста). */
function toast(msg: string, kind: 'success' | 'error' = 'success') {
  const el = document.createElement('div')
  el.textContent = msg
  el.style.cssText =
    `position:fixed; right:16px; bottom:16px; z-index:5000; max-width:420px; padding:10px 14px;` +
    `border-radius:8px; font:13px var(--font, system-ui, sans-serif); box-shadow:0 3px 14px rgba(0,0,0,.4);` +
    `background:${kind === 'success' ? 'var(--primary, #4a90d9)' : 'var(--danger, #b54242)'}; color:#fff;`
  document.body.appendChild(el)
  setTimeout(() => el.remove(), 3500)
}

// ─────────────────────────────────────────────────────────────────────────────
// <llama-engine-panel> — статус/установка/обновление/выбор бекенда llama.cpp
// ─────────────────────────────────────────────────────────────────────────────

class LlamaEnginePanel extends HTMLElement {
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
        <div id="warning" class="hint" style="display:none; margin-top:10px;"></div>
        <div id="overlay" class="overlay">
          <div class="box">
            <h3>Удалить движок llamacpp?</h3>
            <p>Будет освобождено ~1 ГБ на диске. GPU-ускорение отключится.</p>
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
    // Единый прогресс-событие tauri-plugin-downloader (kind: engine/model/mmproj).
    void listen('downloader:progress', (e) => {
      const p = (e as { payload: { downloaded: number; total: number; kind?: string; status?: string } }).payload
      if (p.kind && !['engine', 'model', 'mmproj'].includes(p.kind)) return
      this.setProgress(p.downloaded, p.total)
      if (p.status === 'done' || p.status === 'error') {
        setTimeout(() => this.showProgress(false), 400)
      }
    }).then((u) => { this.unlisten = u }).catch(() => {})
    // Legacy-событие (если где-то ещё шлётся).
    void listen('engine_progress', (e) => {
      const { downloaded, total } = (e as { payload: { downloaded: number; total: number } }).payload
      this.setProgress(downloaded, total)
    }).then(() => {}).catch(() => {})
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

  private hintText(st: EngineStatus, value: string): string {
    const lines: string[] = []
    if (value === 'auto') {
      const resolved = (st.available_variants || []).find((v) => v.id === st.resolved_variant)
      lines.push(`Авто-подбор для этой машины: ${resolved ? resolved.label : (st.resolved_variant || '—')}.`)
    } else {
      const v = (st.available_variants || []).find((x) => x.id === value)
      if (v && v.note) lines.push(v.note)
    }
    const installed = st.installed_variants || []
    if (installed.length > 0) {
      const names = (st.available_variants || []).filter((x) => installed.includes(x.id)).map((x) => x.label).join(', ')
      lines.push(`Установлены: ${names || installed.join(', ')}.`)
    } else {
      lines.push('Ни один бекенд ещё не установлен.')
    }
    lines.push('Смена бекенда применится при следующем запуске модели.')
    return lines.join('\n')
  }

  private async refresh() {
    let st: EngineStatus
    try {
      st = await getEngineStatus()
    } catch (e) {
      this.setStatus(`Ошибка: ${e}`)
      return
    }
    this.setStatus(st.message)
    const gpu = this.root.getElementById('gpu')!
    gpu.textContent = st.has_nvidia
      ? `${st.gpu_name} (драйвер CUDA ${st.cuda_major}.${st.cuda_minor}${st.compute_cap ? `, compute ${st.compute_cap}` : ''}; рекомендуемый вариант: ${st.required_variant || '?'})`
      : 'Не обнаружена'
    this.root.getElementById('path')!.textContent = st.path || '—'

    const sel = this.root.getElementById('variant') as HTMLSelectElement
    const prev = this.applied
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
    this.root.getElementById('apply')!.style.display = sel.value === prev ? 'none' : sel.value === this.applied ? 'none' : 'inline-block'
    this.applyButtonStates(st)
  }

  private applyButtonStates(st: EngineStatus) {
    const installed = st.installed
    this.root.getElementById('install')!.style.display = installed ? 'none' : 'inline-block'
    this.root.getElementById('checkUpdate')!.style.display = installed ? 'inline-block' : 'none'
    this.root.getElementById('remove')!.style.display = installed ? 'inline-block' : 'none'
    this.root.getElementById('setDir')!.style.display = 'inline-block'
    const installBtn = this.root.getElementById('install') as HTMLButtonElement
    const warning = this.root.getElementById('warning')!
    if (!installed && st.requires_driver_update) {
      installBtn.disabled = true
      warning.style.display = 'block'
      warning.textContent =
        `⚠️ Ваш драйвер NVIDIA поддерживает только CUDA ${st.cuda_major}.${st.cuda_minor}.\n` +
        `Для GPU-ускорения обновите драйвер (нужна версия ≥ 527.41, CUDA 12+).\n` +
        `Пока приложение работает в CPU-режиме.`
    } else if (!installed) {
      installBtn.disabled = false
      warning.style.display = 'none'
    } else {
      warning.style.display = 'none'
    }
  }

  private async onVariantChange() {
    const sel = this.root.getElementById('variant') as HTMLSelectElement
    const value = sel.value
    const st: EngineStatus = { available_variants: [], installed_variants: [], resolved_variant: '', message: '', path: '', has_nvidia: false, requires_driver_update: false, cuda_major: 0, cuda_minor: 0, gpu_name: '', compute_cap: '', required_variant: '', selected_variant: '', installed: false }
    this.root.getElementById('variantHint')!.textContent = this.hintText(st, value)
    if (value !== this.applied) this.root.getElementById('apply')!.style.display = 'inline-block'
    else this.root.getElementById('apply')!.style.display = 'none'
    try {
      const s = await getEngineStatus()
      this.root.getElementById('variantHint')!.textContent = this.hintText(s, value)
    } catch { /* не критично */ }
  }

  private async onApplyVariant() {
    const btn = this.root.getElementById('apply') as HTMLButtonElement
    const variant = (this.root.getElementById('variant') as HTMLSelectElement).value
    btn.disabled = true
    try {
      let installedVariants: string[] = []
      try { installedVariants = (await getEngineStatus()).installed_variants || [] } catch { /* не критично */ }
      if (!installedVariants.includes(variant)) {
        this.showProgress(true)
        this.setProgress(0, 0)
      }
      await setEngineVariant(variant)
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
      await installLlamaCpp()
      await this.refresh()
      toast('Движок llamacpp установлен!')
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
    btn.classList.add('checking')
    this.root.getElementById('installUpdate')!.style.display = 'none'
    try {
      const newTag = await checkEngineUpdate()
      if (newTag) {
        this.setStatus(`Доступно обновление движка: ${newTag}`)
        this.root.getElementById('installUpdate')!.style.display = 'inline-block'
      } else {
        this.setStatus('Движок llamacpp актуален')
      }
    } catch (e) {
      this.setStatus('')
      toast(`Ошибка проверки обновления движка: ${e}`, 'error')
    } finally {
      btn.disabled = false
      btn.textContent = prevLabel || 'Проверить обновление'
      btn.classList.remove('checking')
    }
  }

  private async onInstallUpdate() {
    const btn = this.root.getElementById('installUpdate') as HTMLButtonElement
    btn.disabled = true
    this.showProgress(true)
    this.root.getElementById('progressStatus')!.textContent = 'Обновление…'
    try {
      await installEngineUpdate()
      this.root.getElementById('installUpdate')!.style.display = 'none'
      await this.refresh()
      toast('Движок llamacpp обновлён.', 'success')
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
      await removeEngine()
      await this.refresh()
      toast('Движок llamacpp удалён.')
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
      await setEngineDir(path)
      await this.refresh()
      toast('Путь движка изменён.')
    } catch (e) {
      toast(`Ошибка изменения пути: ${e}`, 'error')
    }
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// <llama-download-panel> — скачивание новой модели из каталога + авто-загрузка
// ─────────────────────────────────────────────────────────────────────────────

class LlamaDownloadPanel extends HTMLElement {
  private root!: ShadowRoot
  private catalog: CatalogEntry[] = []
  private busy = false
  private unlisten?: () => void

  connectedCallback() {
    if (this.root) return
    this.root = this.attachShadow({ mode: 'open' })
    this.root.innerHTML = `
      <style>${STYLE}</style>
      <div>
        <div class="row">
          <select id="catalog" style="flex:1;"><option value="">Загрузка каталога…</option></select>
          <button id="download" class="primary">Скачать</button>
        </div>
        <div id="progress" class="progress-container">
          <div class="progress-status" id="progressStatus">0 B / 0 B</div>
          <div class="progress-track"><div class="progress-bar" id="progressBar"></div></div>
        </div>
        <button id="autoDownload" class="primary" style="display:none; margin-top:10px;">⚡ Скачать автоматически</button>
        <div id="overlay" class="overlay">
          <div class="box">
            <h3>Автоматическая загрузка модели</h3>
            <p>Будет скачана модель: <span class="strong" id="modalName"></span></p>
            <p>Путь: <span class="strong" id="modalPath"></span></p>
            <p>Свободно на диске: <span class="strong" id="modalSpace"></span></p>
            <p style="margin-top:12px;">Согласны?</p>
            <div class="overlay-buttons">
              <button id="modalCancel" class="secondary">Отмена</button>
              <button id="modalOk" class="primary">ОК</button>
            </div>
          </div>
        </div>
      </div>`

    this.root.getElementById('download')!.addEventListener('click', () => this.onDownload())
    this.root.getElementById('autoDownload')!.addEventListener('click', () => this.onAutoDownload())
    this.root.getElementById('modalCancel')!.addEventListener('click', () => {
      this.root.getElementById('overlay')!.classList.remove('open')
    })
    this.root.getElementById('modalOk')!.addEventListener('click', () => {
      this.root.getElementById('overlay')!.classList.remove('open')
      void this.doAutoDownload()
    })

    void this.refresh()
    void listen('download_progress', (e) => {
      const { downloaded, total, speed_bps } = (e as { payload: { downloaded: number; total: number; speed_bps: number } }).payload
      const pct = total > 0 ? (downloaded / total) * 100 : 0
      const bar = this.root.getElementById('progressBar') as HTMLElement
      bar.style.width = `${pct}%`
      const st = this.root.getElementById('progressStatus')!
      const speed = formatBytes(speed_bps ?? 0)
      st.textContent = `${formatBytes(downloaded)} / ${total > 0 ? formatBytes(total) : '?'}${total > 0 ? ` (${pct.toFixed(0)}%)` : ''} · ${speed}`
    }).then((u) => { this.unlisten = u }).catch(() => {})
  }

  disconnectedCallback() {
    this.unlisten?.()
  }

  private showProgress(on: boolean) {
    this.root.getElementById('progress')!.style.display = on ? 'block' : 'none'
  }

  private async refresh() {
    try {
      this.catalog = await getModelsCatalog()
      const sel = this.root.getElementById('catalog') as HTMLSelectElement
      sel.innerHTML = '<option value="">-- Выберите модель --</option>'
      for (const m of this.catalog) {
        const o = document.createElement('option')
        o.value = m.name
        const badges = this.badges(m).join(' ')
        o.textContent = (m.size_gb ? `${m.name} (${m.size_gb} GB)` : m.name) + (badges ? `  ${badges}` : '')
        sel.appendChild(o)
      }
      let cfg: EngineConfig
      try { cfg = await getEngineConfig() } catch { cfg = { models: [], model_params: {}, mmproj_files: {}, model_meta: {} } }
      this.root.getElementById('autoDownload')!.style.display = (cfg.models || []).length === 0 ? 'inline-block' : 'none'
    } catch (e) {
      (this.root.getElementById('catalog') as HTMLSelectElement).innerHTML = '<option value="">Ошибка</option>'
    }
  }

  private badges(m: CatalogEntry): string[] {
    const out: string[] = []
    if (m.uncen) out.push('😈')
    if (m.vision) out.push('👁️')
    if (m.audio) out.push('🎵')
    return out
  }

  private async onDownload() {
    if (this.busy) return
    const sel = this.root.getElementById('catalog') as HTMLSelectElement
    const name = sel.value
    if (!name) return
    const model = this.catalog.find((m) => m.name === name)
    if (!model) return
    this.busy = true
    const btn = this.root.getElementById('download') as HTMLButtonElement
    btn.disabled = true
    try {
      const defaultName = (model.download_url.split('/').pop() || `${model.name}.gguf`).split('?')[0]
      const savePath = await save({ defaultPath: defaultName, filters: [{ name: 'GGUF', extensions: ['gguf'] }] })
      if (!savePath) return
      this.showProgress(true)
      this.root.getElementById('progressStatus')!.textContent = 'Подключение…'
      await downloadModel(model.download_url, savePath)
      if (model.mmproj_url) {
        const sep = savePath.includes('\\') ? '\\' : '/'
        const dir = savePath.substring(0, savePath.lastIndexOf(sep))
        const mpName = (model.mmproj_url.split('/').pop() || 'mmproj.gguf').split('?')[0]
        await downloadModel(model.mmproj_url, `${dir}${sep}${mpName}`)
      }
      const res = await addModel(savePath, { uncen: !!model.uncen, vision: !!model.vision, audio: !!model.audio })
      if (res?.warning) toast(res.warning, 'error')
      notifyModelsChanged()
      void this.refresh()
      toast(`Модель ${model.name} скачана!`)
    } catch (e) {
      toast(`Ошибка: ${e}`, 'error')
    } finally {
      btn.disabled = false
      this.busy = false
      this.showProgress(false)
    }
  }

  private async onAutoDownload() {
    try {
      const info = await getAutoDownloadInfo()
      this.root.getElementById('modalName')!.textContent = info.size_gb ? `${info.model_name} (${info.size_gb} GB)` : info.model_name
      this.root.getElementById('modalPath')!.textContent = info.save_path
      this.root.getElementById('modalSpace')!.textContent = `${info.free_space_gb} GB`
      this.root.getElementById('overlay')!.classList.add('open')
    } catch (e) {
      toast(`Ошибка: ${e}`, 'error')
    }
  }

  private async doAutoDownload() {
    if (this.busy) return
    this.busy = true
    const btn = this.root.getElementById('autoDownload') as HTMLButtonElement
    btn.disabled = true
    try {
      const info = await getAutoDownloadInfo()
      this.showProgress(true)
      this.root.getElementById('progressStatus')!.textContent = 'Подключение…'
      await autoDownloadDefaultModel(info.save_path)
      notifyModelsChanged()
      void this.refresh()
      toast(`Модель ${info.model_name} скачана!`)
    } catch (e) {
      toast(`Ошибка: ${e}`, 'error')
    } finally {
      btn.disabled = false
      this.busy = false
      this.showProgress(false)
    }
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// <llama-models-panel> — список установленных моделей (добавить/удалить/файл)
// ─────────────────────────────────────────────────────────────────────────────

class LlamaModelsPanel extends HTMLElement {
  private root!: ShadowRoot
  private busy = false
  private pendingAction: 'remove' | 'delete' | null = null
  private pendingPath = ''
  private capMap: Record<string, { uncen: boolean; vision: boolean; audio: boolean }> = {}

  connectedCallback() {
    if (this.root) return
    this.root = this.attachShadow({ mode: 'open' })
    this.root.innerHTML = `
      <style>${STYLE}</style>
      <div>
        <div id="list" style="display:flex; flex-direction:column; gap:8px;"></div>
        <button id="add" class="secondary" style="margin-top:10px;">+ Добавить модель</button>
        <div id="overlay" class="overlay">
          <div class="box">
            <h3 id="overlayTitle">Удалить</h3>
            <p id="overlayMsg"></p>
            <div class="overlay-buttons">
              <button id="modalCancel" class="secondary">Отмена</button>
              <button id="modalOk" class="danger">Удалить</button>
            </div>
          </div>
        </div>
      </div>`

    this.root.getElementById('add')!.addEventListener('click', () => this.onAdd())
    this.root.getElementById('modalCancel')!.addEventListener('click', () => {
      this.root.getElementById('overlay')!.classList.remove('open')
      this.pendingAction = null
    })
    this.root.getElementById('modalOk')!.addEventListener('click', () => {
      this.root.getElementById('overlay')!.classList.remove('open')
      const action = this.pendingAction
      const path = this.pendingPath
      this.pendingAction = null
      this.pendingPath = ''
      if (action) void this.doAction(action, path)
    })

    document.addEventListener('llama:models-changed', this.onChangedBound)
    void this.refresh()
  }

  disconnectedCallback() {
    document.removeEventListener('llama:models-changed', this.onChangedBound)
  }

  private onChangedBound = () => { void this.refresh() }

  private async refresh() {
    let cfg: EngineConfig
    try { cfg = await getEngineConfig() } catch { return }
    try { this.capMap = await getAllCapabilities() } catch { this.capMap = {} }

    const list = this.root.getElementById('list')!
    list.innerHTML = ''
    const models = cfg.models || []
    if (models.length === 0) {
      const empty = document.createElement('div')
      empty.className = 'empty'
      empty.textContent = 'Модели не добавлены.'
      list.appendChild(empty)
      return
    }

    for (const m of models) {
      const row = document.createElement('div')
      row.style.cssText = 'display:flex; align-items:center; justify-content:space-between; gap:10px; padding:8px 10px; border:1px solid var(--border,#333); border-radius:6px; background:var(--bg-elevated,#1c1c1c);'

      const info = document.createElement('div')
      info.style.cssText = 'display:flex; flex-direction:column; gap:2px; min-width:0;'
      const name = document.createElement('div')
      name.style.cssText = 'font-weight:600; color:var(--text,#eee); word-break:break-all;'
      name.textContent = (cfg.last_model === m ? '● ' : '') + fileName(m)
      const meta = this.capMap[m]
      if (meta) {
        if (meta.uncen) { const s = span('😈', 'Без цензуры (uncensored)'); name.appendChild(s) }
        if (meta.vision) { const s = span('👁️', 'Видит изображения (vision)'); name.appendChild(s) }
        if (meta.audio) { const s = span('🎵', 'Понимает аудио (audio)'); name.appendChild(s) }
      }
      const path = document.createElement('div')
      path.textContent = m
      path.style.cssText = 'font-size:11px; color:var(--text-muted,#888); word-break:break-all;'
      info.appendChild(name)
      info.appendChild(path)

      const buttons = document.createElement('div')
      buttons.style.cssText = 'display:flex; gap:8px; flex-shrink:0; flex-wrap:wrap;'
      const rmv = document.createElement('button')
      rmv.className = 'secondary'
      rmv.textContent = 'Удалить из списка'
      rmv.addEventListener('click', () => this.confirm('remove', m))
      const del = document.createElement('button')
      del.className = 'danger'
      del.textContent = 'Удалить файл'
      del.addEventListener('click', () => this.confirm('delete', m))
      buttons.appendChild(rmv)
      buttons.appendChild(del)

      row.appendChild(info)
      row.appendChild(buttons)
      list.appendChild(row)
    }
  }

  private confirm(action: 'remove' | 'delete', path: string) {
    this.pendingAction = action
    this.pendingPath = path
    const title = this.root.getElementById('overlayTitle')!
    const msg = this.root.getElementById('overlayMsg')!
    const ok = this.root.getElementById('modalOk')!
    if (action === 'remove') {
      title.textContent = `Удалить «${fileName(path)}» из списка?`
      msg.textContent = 'Файл на диске не будет удалён.'
      ok.className = 'secondary'
      ok.textContent = 'Удалить из списка'
    } else {
      title.textContent = `Удалить файл «${fileName(path)}»?`
      msg.textContent = 'Файл с диска будет удалён БЕЗВОЗВРАТНО. Запись тоже исчезнет из списка.'
      ok.className = 'danger'
      ok.textContent = 'Удалить файл'
    }
    this.root.getElementById('overlay')!.classList.add('open')
  }

  private async doAction(action: 'remove' | 'delete', path: string) {
    if (this.busy) return
    this.busy = true
    try {
      if (action === 'remove') {
        await removeModel(path)
        toast('Модель удалена из списка.')
      } else {
        await deleteModelFile(path)
        toast('Файл модели удалён.')
      }
      notifyModelsChanged()
      await this.refresh()
    } catch (e) {
      toast(`Ошибка: ${e}`, 'error')
    } finally {
      this.busy = false
    }
  }

  private async onAdd() {
    if (this.busy) return
    this.busy = true
    const btn = this.root.getElementById('add') as HTMLButtonElement
    btn.disabled = true
    try {
      const sel = await openDialog({ filters: [{ name: 'Model', extensions: ['gguf'] }] })
      if (!sel) return
      const path = Array.isArray(sel) ? sel[0] : sel
      if (!path) return
      const res = await addModel(path, null)
      if (res?.warning) toast(res.warning, 'error')
      notifyModelsChanged()
      await this.refresh()
    } catch (e) {
      toast(`Не удалось добавить модель: ${e}`, 'error')
    } finally {
      btn.disabled = false
      this.busy = false
    }
  }
}

function span(text: string, title: string): HTMLSpanElement {
  const s = document.createElement('span')
  s.textContent = text
  s.title = title
  s.className = 'badge'
  s.style.marginLeft = '5px'
  return s
}

if (!customElements.get('llama-engine-panel')) {
  customElements.define('llama-engine-panel', LlamaEnginePanel)
}
if (!customElements.get('llama-download-panel')) {
  customElements.define('llama-download-panel', LlamaDownloadPanel)
}
if (!customElements.get('llama-models-panel')) {
  customElements.define('llama-models-panel', LlamaModelsPanel)
}

export { esc, fileName, formatBytes, toast }