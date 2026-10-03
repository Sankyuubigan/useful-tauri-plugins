import {
  ttsGetSettings,
  ttsDefaultDirs,
  ttsListModels,
  ttsListVoices,
  ttsDownloadModel,
  ttsAddVoice,
  ttsDeleteVoice,
  ttsUpdateVoice,
  ttsVoiceAudio,
  ttsVoiceAvatar,
  ttsVoiceTrimmedAudio,
  type TtsSettings,
  type VoiceInfo,
} from './index'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import { PANEL_STYLES } from './panels/styles'
import './panels/speech-engine-panel'

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
    // Прогресс модели приходит из общего движка загрузок проекта
    // (`downloader:progress`), а не из плагина речи: свой загрузчик отдан
    // в пользу единого (tauri-plugin-downloader).
    void listen<{ kind: string; downloaded: number; total: number; speed_bps: number }>(
      'downloader:progress',
      (ev) => {
        const p = ev.payload
        if (p.kind !== 'model') return
        this.download = { current: p.downloaded, total: p.total }
        this.updateProgress()
      },
    )
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

// `speech-engine-panel` регистрируется в ./panels/speech-engine-panel (импорт выше).
if (!customElements.get('speech-models-panel')) {
  customElements.define('speech-models-panel', SpeechModelsPanel)
}
if (!customElements.get('speech-voice-storage')) {
  customElements.define('speech-voice-storage', SpeechVoiceStorage)
}