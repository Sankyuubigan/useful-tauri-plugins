import {
  ttsPresets,
  ttsEngineBackends,
  ttsGetSettings,
  ttsDefaultDirs,
  ttsListModels,
  ttsListVoices,
  ttsCheckUpdate,
  ttsDownloadEngine,
  ttsDownloadModel,
  ttsAddVoice,
  ttsDeleteVoice,
  ttsUpdateVoice,
  ttsVoiceAudio,
  ttsVoiceAvatar,
  ttsVoiceTrimmedAudio,
  ttsSaveSettings,
  ttsUnload,
  type TtsSettings,
  type VoiceInfo,
} from './index'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'

/** Общие стили плашек — темизируются через CSS-переменные хоста. */
const PANEL_STYLES = `
  :host { display: block; color: var(--text, #333); font-family: var(--font, system-ui, sans-serif); }
  * { box-sizing: border-box; }
  button { margin: 4px 4px 0 0; padding: 5px 10px; cursor: pointer; border-radius: 6px;
           border: 1px solid var(--border, #ccc); background: var(--session-hover, #eee);
           color: var(--text, #333); font: inherit; }
  button.primary { background: var(--primary, #89b4fa); color: #1e1e2e; font-weight: 600; border-color: var(--primary, #89b4fa); }
  button.primary:hover:not(:disabled) { background: var(--primary-hover, #74a0f0); }
  button:disabled { opacity: .5; cursor: default; }
  select, input, textarea { font: inherit; color: var(--text, #333); background: var(--bg-color, #fff);
           border: 1px solid var(--border, #ccc); border-radius: 6px; padding: 4px 6px; }
  input[readonly] { opacity: .85; }
  input[type="range"] { accent-color: var(--primary, #89b4fa); width: 100%; padding: 0; }
  .muted { color: var(--text-muted, #888); font-size: 12px; }
  .row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin-top: 6px; }
  .field { margin-top: 8px; }
  .field label { display: block; font-size: 13px; margin-bottom: 2px; }
  .field input[type="text"], .field select { width: 100%; }
  .progress { height: 6px; background: var(--session-hover, #eee); border-radius: 3px; margin-top: 6px; overflow: hidden; }
  .progress > div { height: 100%; background: var(--primary, #4a90d9); width: 0; transition: width .2s; }
  h3 { font-size: 15px; margin: 14px 0 8px; opacity: .85; }
  h4 { font-size: 14px; margin: 12px 0 6px; opacity: .8; }
  hr { border: none; border-top: 1px solid var(--border, #45475a); margin: 16px 0; }
  label { display: block; margin: 10px 0 6px; opacity: .8; }
  .checkbox-row { display: flex; align-items: center; gap: 8px; margin: 12px 0 4px; opacity: .9; }
  .checkbox-row input { flex: none; width: auto; min-width: auto; }
  .hint { opacity: .65; font-size: 13px; margin: 4px 0 10px; color: var(--text, #cdd6f4); }
  .hint.warn { color: #f9e2af; }
  code { background: var(--bg-color, #313244); padding: 1px 6px; border-radius: 4px; word-break: break-all; color: var(--text, #cdd6f4); }
  .vs-head { display: flex; justify-content: space-between; align-items: center; gap: 12px; }
  .vs-head h3 { margin: 14px 0 8px; }
  .voice-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 10px; margin-top: 6px; }
  .voice-card { display: flex; gap: 12px; align-items: center; background: var(--bg-color, #313244);
       border: 1px solid var(--border, #45475a); border-radius: 10px; padding: 10px 12px; }
  .vc-avatar { flex: 0 0 auto; width: 52px; height: 52px; border-radius: 50%; overflow: hidden;
       background: var(--session-hover, #45475a); display: flex; align-items: center; justify-content: center; }
  .vc-avatar img { width: 100%; height: 100%; object-fit: cover; }
  .vc-avatar-ph { font-size: 22px; font-weight: 700; color: var(--text, #cdd6f4); }
  .vc-body { flex: 1 1 auto; min-width: 0; }
  .vc-name { font-size: 14px; font-weight: 600; color: var(--text, #cdd6f4); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .vc-ref { font-size: 12px; opacity: .7; color: var(--text, #cdd6f4); margin-top: 2px; display: -webkit-box;
       -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
  .vc-date { font-size: 11px; opacity: .5; margin-top: 3px; }
  .vc-actions { flex: 0 0 auto; display: flex; gap: 4px; align-items: center; }
  .vc-actions button { padding: 4px 9px; font-size: 12px; margin: 0; }
  .vc-del { background: transparent; color: #f38ba8; opacity: .8; }
  .vc-del:hover { opacity: 1; background: var(--session-hover, #45475a); }
  .ve-overlay { position: fixed; inset: 0; background: rgba(0, 0, 0, 0.55); display: flex; align-items: center;
       justify-content: center; z-index: 50; padding: 16px; }
  .ve-modal { background: #181825; border: 1px solid var(--border, #45475a); border-radius: 12px; padding: 18px 20px;
       width: 100%; max-width: 440px; max-height: 90vh; overflow-y: auto; box-shadow: 0 20px 50px rgba(0, 0, 0, 0.5);
       color: var(--text, #cdd6f4); }
  .ve-modal h3 { margin: 0 0 12px; font-size: 17px; }
  .ve-modal input[type="text"], .ve-modal textarea { width: 100%; box-sizing: border-box; padding: 8px; border-radius: 6px;
       border: 1px solid var(--border, #45475a); background: var(--bg-color, #313244); color: var(--text, #cdd6f4); }
  .ve-modal textarea { min-height: 64px; resize: vertical; font-family: inherit; }
  .ve-modal input::placeholder, .ve-modal textarea::placeholder { color: var(--text-muted, #7f849c); }
  .status { opacity: .8; font-size: 13px; color: var(--text, #cdd6f4); }
  ul { list-style: none; padding: 0; margin: 8px 0 0; }
  li { display: flex; justify-content: space-between; align-items: center; gap: 8px;
       padding: 6px 10px; background: var(--bg-color, #313244); border-radius: 6px; margin-bottom: 4px; }
  .mname { flex: 1; min-width: 0; font-size: 13px; }
  .badges { display: flex; align-items: center; gap: 6px; flex-shrink: 0; }
  button.small { padding: 2px 10px; font-size: 11px; border-radius: 8px; margin: 0; }
  .badge { font-size: 11px; padding: 2px 7px; border-radius: 10px; background: var(--session-hover, #45475a);
           color: var(--text, #cdd6f4); white-space: nowrap; }
  .badge.ru { background: #313244; color: #f38ba8; border: 1px solid #f38ba8; font-weight: 600; }
  .badge.ok { background: #1e2a1e; color: #a6e3a1; display: inline-flex; align-items: center; gap: 5px; }
  .badge.ok::before { content: ''; width: 8px; height: 8px; border-radius: 50%; background: #a6e3a1; box-shadow: 0 0 6px #a6e3a1; }
  .badge.warn { background: #33260f; color: #f9c77a; }
`

/**
 * <speech-engine-panel></speech-engine-panel>
 *
 * Плашка «Движок TTS»: установка/обновление prebuilt CrispASR, выбор бэкенда,
 * папки движка/моделей, выгрузка (освобождение VRAM).
 */
class SpeechEnginePanel extends HTMLElement {
  private root!: ShadowRoot
  private busy = false
  private initialized = false
  private settings: TtsSettings = {}
  private defaults = { engine_dir: '', models_dir: '' }
  private backend = ''
  private backends: Array<{ id: string; label: string }> = []
  private status = ''
  private updateInfo = ''
  private download: { current: number; total: number } | null = null
  private unlisteners: Array<() => void> = []

  connectedCallback() {
    this.root = this.attachShadow({ mode: 'open' })
    this.render()
    this.init()
  }

  private render() {
    this.root.innerHTML = `
      <style>${PANEL_STYLES}</style>
      <div>
        <h3>Движок</h3>
        <label for="backend">Тип бэкенда:</label>
        <div class="row">
          <select id="backend"></select>
          <span id="backends_none" class="hint" hidden>не удалось получить список бинарей (нет сети?)</span>
        </div>
        <p class="hint">Статус: <span id="engine_status">—</span></p>
        <div class="row">
          <button id="install" class="primary">скачать движок</button>
          <button id="refresh">проверить обновления</button>
          <button id="engine_dir_browse">изменить путь к движку</button>
          <button id="unload">выгрузить (VRAM)</button>
        </div>
        <p class="hint">Путь к движку: <code id="engine_dir_code"></code></p>
        <p class="hint" id="update_info" hidden></p>
        <div id="progress" class="progress" style="display:none"><div></div></div>
        <div id="status" class="muted" style="margin-top:8px"></div>

        <hr />

        <h3>Папка моделей TTS</h3>
        <div class="row">
          <input id="models_dir_input" type="text" readonly value="" style="flex:1; min-width:200px;" />
          <button id="models_dir_browse">выбрать папку</button>
        </div>
        <p class="hint">Внутри создаётся подпапка на каждый пресет; все GGUF качаются туда автоматически.</p>
      </div>`

    this.root.getElementById('engine_dir_browse')!.addEventListener('click', () => this.pickDir('engine_dir'))
    this.root.getElementById('models_dir_browse')!.addEventListener('click', () => this.pickDir('models_dir'))
    this.root.getElementById('backend')!.addEventListener('change', (e) => {
      this.backend = (e.target as HTMLSelectElement).value
      this.saveSettings()
    })
    this.root.getElementById('install')!.addEventListener('click', () => this.install())
    this.root.getElementById('refresh')!.addEventListener('click', () => this.checkUpdate())
    this.root.getElementById('unload')!.addEventListener('click', async () => {
      try { await ttsUnload(); this.setStatus('движок выгружен'); } catch (e) { this.setStatus('ошибка: ' + (e as Error).message) }
    })
  }

  private async init() {
    if (this.initialized) return
    const [s, d, b] = await Promise.all([
      ttsGetSettings().catch(() => ({}) as TtsSettings),
      ttsDefaultDirs().catch(() => ({ engine_dir: '', models_dir: '' })),
      ttsEngineBackends().catch(() => []),
    ])
    this.settings = s
    this.defaults = d
    this.backends = b
    if (!this.settings.engine_dir) this.settings.engine_dir = d.engine_dir
    if (!this.settings.models_dir) this.settings.models_dir = d.models_dir
    if (!this.settings.engine_backend && b.length) this.settings.engine_backend = b[0].id

    ;(this.root.getElementById('engine_dir_code') as HTMLElement).textContent = this.settings.engine_dir
    ;(this.root.getElementById('models_dir_input') as HTMLInputElement).value = this.settings.models_dir
    const sel = this.root.getElementById('backend') as HTMLSelectElement
    sel.innerHTML = b.map(x => `<option value="${x.id}">${x.label}</option>`).join('')
    if (b.length === 0) {
      const none = document.createElement('option')
      none.value = ''
      none.textContent = 'сеть недоступна…'
      sel.appendChild(none)
      const span = this.root.getElementById('backends_none')
      if (span) span.hidden = false
    }
    sel.value = this.settings.engine_backend || b[0]?.id || ''
    this.backend = sel.value

    this.unlisteners.push(await listen<{ kind: string; name: string; downloaded: number; total: number }>('tts-download', (ev) => {
      const p = ev.payload
      this.download = { current: p.downloaded, total: p.total }
      this.updateProgress()
    }))
    this.initialized = true
    this.checkUpdate()
  }

  private async pickDir(field: 'engine_dir' | 'models_dir') {
    const picked = await open({ directory: true }).catch(() => null)
    if (picked && typeof picked === 'string') {
      if (field === 'engine_dir') {
        this.settings.engine_dir = picked
        ;(this.root.getElementById('engine_dir_code') as HTMLElement).textContent = picked
      } else {
        this.settings.models_dir = picked
        ;(this.root.getElementById('models_dir_input') as HTMLInputElement).value = picked
      }
      this.saveSettings()
    }
  }

  private async saveSettings() {
    try { await ttsSaveSettings(this.settings) } catch { /* ignore */ }
  }

  private setStatus(s: string) {
    this.status = s
    const el = this.root.getElementById('status')
    if (el) (el as HTMLElement).textContent = [s, this.updateInfo].filter(Boolean).join(' • ')
  }

  private setEngineStatus(s: string) {
    const el = this.root.getElementById('engine_status')
    if (el) el.textContent = s
  }

  private updateProgress() {
    const box = this.root.getElementById('progress') as HTMLElement
    const bar = box.firstElementChild as HTMLElement
    if (this.download && this.download.total > 0) {
      box.style.display = 'block'
      bar.style.width = `${Math.min(100, (this.download.current / this.download.total) * 100)}%`
      this.setStatus(`скачивание… ${Math.round(this.download.total / 1048576)} МБ`)
    } else if (this.download) {
      box.style.display = 'block'
      bar.style.width = '100%'
    } else {
      box.style.display = 'none'
    }
  }

  private async checkUpdate() {
    const res = await ttsCheckUpdate().catch(() => null)
    if (!res) return
    const infoEl = this.root.getElementById('update_info') as HTMLElement | null
    if (res.ok) {
      const need = (res.engines ?? []).filter(e => e.update_available)
      this.updateInfo = need.length
        ? `последняя версия ${res.latest}: обновления для ${need.map(e => e.label).join(', ')}`
        : `установлена последняя версия (${res.latest})`
      this.setEngineStatus(need.length ? 'есть обновления' : `установлена последняя версия (${res.latest})`)
    } else {
      this.updateInfo = 'обновления недоступны: ' + (res.error ?? '?')
      this.setEngineStatus('не установлен')
    }
    if (infoEl) {
      infoEl.textContent = this.updateInfo
      infoEl.hidden = false
    }
    this.setStatus(this.status)
  }

  private async install() {
    if (this.busy) return
    if (!this.backend) { this.setStatus('выберите бэкенд'); return }
    this.busy = true
    this.setStatus('скачиваю движок…')
    this.download = { current: 0, total: 0 }
    this.updateProgress()
    try {
      await ttsDownloadEngine(this.backend, this.settings.engine_dir || this.defaults.engine_dir)
      this.setStatus('движок скачан')
      await this.checkUpdate()
    } catch (e) {
      this.setStatus('ошибка: ' + (e as Error).message)
    } finally {
      this.download = null
      this.updateProgress()
      this.busy = false
    }
  }

  disconnectedCallback() {
    for (const u of this.unlisteners) u()
    this.unlisteners = []
  }
}

/**
 * <speech-models-panel models-dir="..."></speech-models-panel>
 *
 * Плашка «Установленные модели»: список GGUF-пресетов с признаком установки,
 * кнопка скачивания недостающих. Атрибут `models-dir` обычно не нужен —
 * берётся из ttsGetSettings().
 */
class SpeechModelsPanel extends HTMLElement {
  private root!: ShadowRoot
  private models: Array<any> = []
  private download: { current: number; total: number } | null = null
  private busyId = ''
  private modelsDir = ''
  private initialized = false
  private unlisteners: Array<() => void> = []

  connectedCallback() {
    this.root = this.attachShadow({ mode: 'open' })
    this.render()
    this.init()
  }

  static get observedAttributes() { return ['models-dir'] }
  attributeChangedCallback() {
    if (!this.root || !this.initialized) return
    const dir = this.getAttribute('models-dir')
    if (dir && dir !== this.modelsDir) {
      this.modelsDir = dir
      void this.reload()
    }
  }

  private render() {
    this.root.innerHTML = `
      <style>${PANEL_STYLES}</style>
      <div>
        <div class="row"><strong>Установленные модели (GGUF)</strong></div>
        <div id="progress" class="progress" style="display:none"><div></div></div>
        <div id="models"></div>
        <div id="status" class="muted" style="margin-top:8px"></div>
        <div class="row" style="margin-top:8px">
          <button id="refresh" class="small">⟳ обновить</button>
        </div>
      </div>`
    this.root.getElementById('refresh')!.addEventListener('click', () => this.reload())
  }

  private async init() {
    if (this.initialized) return
    const attr = this.getAttribute('models-dir')
    if (!this.modelsDir) this.modelsDir = attr || ''
    if (!this.modelsDir) {
      const s = await ttsGetSettings().catch(() => ({} as TtsSettings))
      this.modelsDir = s.models_dir || ''
      if (!this.modelsDir) {
        const d = await ttsDefaultDirs().catch(() => ({ models_dir: '' }))
        this.modelsDir = d.models_dir
      }
    }
    this.initialized = true
    // Не блокируем init на listen: в некоторых сборках listen может кидать,
    // но список моделей обязан грузиться независимо. Таймстемп/событие.
    void listen<{ kind: string; name: string; downloaded: number; total: number }>('tts-download', (ev) => {
      const p = ev.payload
      if (p.kind !== 'model') return
      this.download = { current: p.downloaded, total: p.total }
      this.updateProgress()
    })
      .then((u) => this.unlisteners.push(u))
      .catch(() => {})
    void this.reload()
  }

  private async reload() {
    this.setStatus('загружаю…')
    const t = setTimeout(() => this.setStatus('таймаут ожидания ответа (сеть?)'), 10000)
    try {
      this.models = await ttsListModels(this.modelsDir)
      if (this.models.length === 0) {
        this.setStatus('список пуст (нет пресетов или пустой каталог моделей)')
      } else {
        this.setStatus(`найдено моделей: ${this.models.length}`)
      }
    } catch (e) {
      this.setStatus('ошибка: ' + ((e as Error).message || String(e)))
    } finally {
      clearTimeout(t)
    }
    this.renderList()
  }

  private renderList() {
    const box = this.root.getElementById('models')!
    if (this.models.length === 0) {
      box.textContent = ''
      return
    }
    box.innerHTML = '<ul>' + this.models.map(m => `
      <li>
        <span class="mname">${this.escapeHtml(m.label)}${(m.voice_type === 'clone' || m.voice_type === 'clone_named') ? ' 🎭' : ''}</span>
        <span class="badges">
          <span class="badge">${m.size}</span>
          ${m.supports_russian ? '<span class="badge ru">RU</span>' : ''}
          ${m.installed
            ? '<span class="badge ok">установлено</span>'
            : `<button data-id="${m.id}" class="small primary install">скачать</button>`}
        </span>
      </li>`).join('') + '</ul>'
    for (const btn of box.querySelectorAll<HTMLButtonElement>('button.install')) {
      btn.addEventListener('click', () => this.install(btn.dataset['id'] || ''))
    }
  }

  private escapeHtml(s: string): string {
    return String(s).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]!))
  }

  private setStatus(s: string) {
    const el = this.root.getElementById('status')
    if (el) (el as HTMLElement).textContent = s
  }

  private updateProgress() {
    const box = this.root.getElementById('progress') as HTMLElement
    const bar = box.firstElementChild as HTMLElement
    if (this.download && this.download.total > 0) {
      box.style.display = 'block'
      bar.style.width = `${Math.min(100, (this.download.current / this.download.total) * 100)}%`
    } else {
      box.style.display = 'none'
    }
  }

  private async install(id: string) {
    if (this.busyId) return
    this.busyId = id
    this.setStatus(`скачиваю ${id}…`)
    this.download = { current: 0, total: 0 }
    this.updateProgress()
    try {
      await ttsDownloadModel(id, this.modelsDir)
      this.models = await ttsListModels(this.modelsDir).catch(() => this.models)
      this.renderList()
      this.setStatus('модель скачана')
    } catch (e) {
      this.setStatus('ошибка: ' + (e as Error).message)
    } finally {
      this.download = null
      this.updateProgress()
      this.busyId = ''
    }
  }

  disconnectedCallback() {
    for (const u of this.unlisteners) u()
    this.unlisteners = []
  }
}

/**
 * <speech-voice-storage models-dir="..."></speech-voice-storage>
 *
 * Плашка «Хранилище голосов»: список сохранённых голосов (аватары, прослушивание),
 * выбор WAV → декод в 24k mono + опц. шумоподавление),
 * удаление.
 */
class SpeechVoiceStorage extends HTMLElement {
  private root!: ShadowRoot
  private voices: VoiceInfo[] = []
  private modelsDir = ''
  private initialized = false
  private unlisteners: Array<() => void> = []

  connectedCallback() {
    this.root = this.attachShadow({ mode: 'open' })
    this.render()
    this.init()
  }

  static get observedAttributes() { return ['models-dir'] }
  attributeChangedCallback() {
    if (!this.root || !this.initialized) return
    const dir = this.getAttribute('models-dir')
    if (dir && dir !== this.modelsDir) {
      this.modelsDir = dir
      void this.refresh()
    }
  }

  private render() {
    this.root.innerHTML = `
      <style>${PANEL_STYLES}</style>
      <div>
        <div class="vs-head">
          <h3>Хранилище голосов</h3>
          <button id="add" class="primary">добавить голос</button>
        </div>
        <p class="hint">Здесь хранятся ваши голоса для клонирования. Если нажать «добавить голос», откроется редактор — выберите референсное аудио (WAV/MP3/OGG/FLAC) и укажите имя.</p>
        <div id="voices"></div>
        <div class="row"><span id="status" class="status" style="margin-left:8px"></span></div>
        <div id="editor_host"></div>
      </div>`

    this.root.getElementById('add')!.addEventListener('click', () => this.openEditor(null))
  }

  private async init() {
    if (this.initialized) return
    const attr = this.getAttribute('models-dir')
    if (!this.modelsDir) this.modelsDir = attr || ''
    if (!this.modelsDir) {
      const s = await ttsGetSettings().catch(() => ({} as TtsSettings))
      this.modelsDir = s.models_dir || ''
      if (!this.modelsDir) {
        const d = await ttsDefaultDirs().catch(() => ({ models_dir: '' }))
        this.modelsDir = d.models_dir
      }
    }
    this.initialized = true
    await this.refresh()
  }

  private async refresh() {
    this.voices = await ttsListVoices(this.modelsDir).catch(() => [])
    this.renderList()
  }

  private setStatus(s: string) {
    const el = this.root.getElementById('status')
    if (el) (el as HTMLElement).textContent = s
  }

  private async renderList() {
    const box = this.root.getElementById('voices')!
    if (this.voices.length === 0) {
      box.innerHTML = '<p class="hint warn">Пока нет сохранённых голосов — нажмите «добавить голос».</p>'
      return
    }
    box.innerHTML = '<div class="voice-grid"></div>'
    const grid = box.firstElementChild as HTMLElement
    for (const v of this.voices) {
      const card = document.createElement('div')
      card.className = 'voice-card'

      const av = document.createElement('div')
      av.className = 'vc-avatar'
      if (v.has_avatar) {
        const b = await ttsVoiceAvatar(this.modelsDir, v.id).catch(() => null)
        if (b && b.length) {
          const img = document.createElement('img')
          img.alt = v.name
          img.src = 'data:image/jpeg;base64,' + btoa(String.fromCharCode(...b))
          av.appendChild(img)
        } else {
          const ph = document.createElement('div')
          ph.className = 'vc-avatar-ph'
          ph.textContent = v.name.slice(0, 1).toUpperCase()
          av.appendChild(ph)
        }
      } else {
        const ph = document.createElement('div')
        ph.className = 'vc-avatar-ph'
        ph.textContent = v.name.slice(0, 1).toUpperCase()
        av.appendChild(ph)
      }
      card.appendChild(av)

      const body = document.createElement('div')
      body.className = 'vc-body'
      const nm = document.createElement('div')
      nm.className = 'vc-name'
      nm.textContent = v.name
      const rf = document.createElement('div')
      rf.className = 'vc-ref'
      rf.textContent = v.ref_text || '— нет референсного текста —'
      body.appendChild(nm)
      body.appendChild(rf)
      if (v.created_at) {
        const dt = document.createElement('div')
        dt.className = 'vc-date'
        dt.textContent = this.fmtDate(v.created_at)
        body.appendChild(dt)
      }
      card.appendChild(body)

      const acts = document.createElement('div')
      acts.className = 'vc-actions'
      const bPlay = document.createElement('button')
      bPlay.textContent = '▶'
      bPlay.title = 'Прослушать'
      bPlay.addEventListener('click', () => this.playVoice(v.id))
      const bEdit = document.createElement('button')
      bEdit.textContent = '✎'
      bEdit.title = 'Изменить'
      bEdit.addEventListener('click', () => this.openEditor(v))
      const bDel = document.createElement('button')
      bDel.className = 'vc-del'
      bDel.textContent = '✕'
      bDel.title = 'Удалить'
      bDel.addEventListener('click', () => this.deleteVoice(v.id))
      acts.append(bPlay, bEdit, bDel)
      card.appendChild(acts)

      grid.appendChild(card)
    }
  }

  private fmtDate(s: string): string {
    if (!s) return ''
    return s.replace('T', ' ').replace('Z', '').slice(0, 16)
  }

  private async playVoice(id: string) {
    try {
      const data = await ttsVoiceAudio(this.modelsDir, id)
      const url = URL.createObjectURL(new Blob([new Uint8Array(data)], { type: 'audio/wav' }))
      const audio = new Audio(url)
      audio.onended = () => URL.revokeObjectURL(url)
      await audio.play()
    } catch (e) {
      this.setStatus('не удалось проиграть: ' + (e as Error).message)
    }
  }

  private async deleteVoice(id: string) {
    if (!confirm('Удалить голос безвозвратно?')) return
    try {
      await ttsDeleteVoice(this.modelsDir, id)
      await this.refresh()
      this.setStatus('голос удалён')
    } catch (e) {
      this.setStatus('ошибка удаления: ' + (e as Error).message)
    }
  }

  private baseName(p: string): string {
    return p.split('\\').pop()?.split('/').pop() || p
  }

  private openEditor(voice: VoiceInfo | null) {
    const host = this.root.getElementById('editor_host')!
    const ac = new AbortController()
    const close = () => {
      ac.abort()
      host.innerHTML = ''
    }
    host.innerHTML = `
      <div class="ve-overlay" id="ve_overlay" role="presentation">
        <div class="ve-modal" role="dialog" aria-modal="true">
          <h3>${voice ? 'Изменить голос' : 'Новый голос'}</h3>

          <label for="ve_name">Имя голоса:</label>
          <input id="ve_name" type="text" placeholder="напр. Morgan Freeman" value="${voice ? voice.name : ''}" />

          <label for="ve_audio">Референсное аудио${voice ? ' (опц. — чтобы заменить)' : ''}:</label>
          <div class="row">
            <button id="ve_pick_audio">выбрать аудио</button>
            <span class="status" id="ve_audio_status">${voice ? 'без изменений' : 'не выбрано'}</span>
          </div>

          <div id="ve_denoise_block" style="${voice ? 'display:none' : ''}">
            <label class="checkbox-row">
              <input id="ve_denoise" type="checkbox" checked />
              шумоподавление (RNNoise)
            </label>
            <label for="ve_denoise_strength">Сила шумоподавления: <span id="ve_denoise_label">90%</span></label>
            <input id="ve_denoise_strength" type="range" min="0" max="1" step="0.05" value="0.9" />
          </div>

          <label for="ve_text">Референсный текст (опц., улучшает качество):</label>
          <textarea id="ve_text" placeholder="что говорится в аудио">${voice?.ref_text ?? ''}</textarea>

          ${voice ? '<div class="row"><button id="ve_play">▶ прослушать референс</button></div>' : ''}

          <label for="ve_avatar">Аватар (опц.):</label>
          <div class="row">
            <button id="ve_pick_avatar">выбрать картинку</button>
            <span class="status" id="ve_avatar_status">${voice?.has_avatar ? 'без изменений' : 'не выбрана'}</span>
            ${voice?.has_avatar ? '<button class="small" id="ve_remove_avatar">сбросить аватар</button>' : ''}
          </div>

          <div class="row">
            <button id="ve_save" class="primary">сохранить</button>
            <button id="ve_cancel">отмена</button>
            <span class="status" id="ve_status"></span>
          </div>
        </div>
      </div>
      <audio id="ve_audio"></audio>`

    const $ = (id: string) => host.querySelector<HTMLElement>('#' + id)!
    let audioPath = ''
    let avatarPath = ''
    let removeAvatar = false
    let busy = false

    this.root.addEventListener('keydown', (e: Event) => {
      if ((e as KeyboardEvent).key === 'Escape') close()
    }, { signal: ac.signal })
    $('ve_overlay').addEventListener('click', (e) => {
      if (e.target === e.currentTarget) close()
    }, { signal: ac.signal })
    $('ve_cancel').addEventListener('click', () => close(), { signal: ac.signal })

    const denoiseChk = $('ve_denoise') as HTMLInputElement | null
    const denoiseStr = $('ve_denoise_strength') as HTMLInputElement | null
    denoiseChk?.addEventListener('change', (e) => {
      const block = this.root.getElementById('ve_denoise_block')
      if (block) block.style.display = (e.target as HTMLInputElement).checked ? '' : 'none'
    }, { signal: ac.signal })
    denoiseStr?.addEventListener('input', (e) => {
      const lbl = this.root.getElementById('ve_denoise_label')
      if (lbl) lbl.textContent = Math.round(Number((e.target as HTMLInputElement).value) * 100) + '%'
    }, { signal: ac.signal })

    $('ve_pick_audio').addEventListener('click', async () => {
      const p = await open({ filters: [{ name: 'Audio', extensions: ['wav', 'ogg', 'mp3', 'flac'] }] }).catch(() => null)
      if (p && typeof p === 'string') {
        audioPath = p
        $('ve_audio_status').textContent = this.baseName(p)
        const block = this.root.getElementById('ve_denoise_block')
        if (block) block.style.display = ''
      }
    }, { signal: ac.signal })

    $('ve_pick_avatar').addEventListener('click', async () => {
      const p = await open({ filters: [{ name: 'Image', extensions: ['jpg', 'jpeg', 'png', 'webp'] }] }).catch(() => null)
      if (p && typeof p === 'string') {
        avatarPath = p
        removeAvatar = false
        $('ve_avatar_status').textContent = this.baseName(p)
      }
    }, { signal: ac.signal })

    const rmBtn = $('ve_remove_avatar')
    rmBtn?.addEventListener('click', () => {
      removeAvatar = !removeAvatar
      avatarPath = ''
      $('ve_avatar_status').textContent = removeAvatar ? 'будет удалён' : 'без изменений'
    }, { signal: ac.signal })

    const playBtn = $('ve_play')
    playBtn?.addEventListener('click', async () => {
      if (!voice) return
      try {
        const b = await ttsVoiceTrimmedAudio(this.modelsDir, voice.id, '')
        const data = new Uint8Array(b)
        const url = URL.createObjectURL(new Blob([data], { type: 'audio/wav' }))
        const audioEl = $('ve_audio') as HTMLAudioElement
        audioEl.src = url
        audioEl.onended = () => URL.revokeObjectURL(url)
        await audioEl.play()
      } catch (e) {
        $('ve_status').textContent = 'ошибка: ' + (e as Error).message
      }
    }, { signal: ac.signal })

    $('ve_save').addEventListener('click', async () => {
      if (busy) return
      const name = ($('ve_name') as HTMLInputElement).value.trim()
      if (!name) {
        $('ve_status').textContent = 'введите имя голоса'
        return
      }
      if (!voice && !audioPath) {
        $('ve_status').textContent = 'выберите референсное аудио'
        return
      }
      busy = true
      $('ve_status').textContent = 'сохраняю…'
      const refText = ($('ve_text') as HTMLTextAreaElement).value
      const denoise = denoiseChk ? denoiseChk.checked : true
      const denoiseStrength = denoiseStr ? Number(denoiseStr.value) : 0.9
      try {
        if (voice) {
          await ttsUpdateVoice({
            modelsDir: this.modelsDir,
            id: voice.id,
            name,
            refText,
            avatar: removeAvatar ? '__REMOVE__' : avatarPath,
            srcAudio: audioPath,
            denoise,
            denoiseStrength,
          })
        } else {
          await ttsAddVoice({
            modelsDir: this.modelsDir,
            name,
            srcAudio: audioPath,
            refText,
            avatar: avatarPath,
            denoise,
            denoiseStrength,
          })
        }
        close()
        await this.refresh()
        this.setStatus('голос сохранён')
      } catch (e) {
        $('ve_status').textContent = 'ошибка: ' + (e as Error).message
      } finally {
        busy = false
      }
    }, { signal: ac.signal })
  }

  disconnectedCallback() {
    for (const u of this.unlisteners) u()
    this.unlisteners = []
  }
}

if (!customElements.get('speech-engine-panel')) {
  customElements.define('speech-engine-panel', SpeechEnginePanel)
}
if (!customElements.get('speech-models-panel')) {
  customElements.define('speech-models-panel', SpeechModelsPanel)
}
if (!customElements.get('speech-voice-storage')) {
  customElements.define('speech-voice-storage', SpeechVoiceStorage)
}