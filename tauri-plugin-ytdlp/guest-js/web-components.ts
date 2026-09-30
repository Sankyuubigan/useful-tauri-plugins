import {
  cancelDownload,
  checkYtdlpUpdate,
  downloadVideo,
  getYtdlpStatus,
  installDeno,
  installYtdlp,
  installYtdlpUpdate,
  setFfmpegPath,
  setYtdlpDir,
  type YtdlpStatus,
} from './index'
import { invoke } from '@tauri-apps/api/core'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { listen } from '@tauri-apps/api/event'

const STYLE = `
  :host { display: block; color: var(--text, #eee); font-family: var(--font, system-ui, sans-serif); font-size: 13px; }
  .row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .row + .row { margin-top: 8px; }
  label { color: var(--text-muted, #999); min-width: 80px; }
  button { padding: 6px 12px; cursor: pointer; border-radius: 6px; border: 1px solid var(--border, #333);
           background: var(--bg-elevated, #2a2a2a); color: var(--text, #eee); font: inherit; font-size: 13px; }
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
  .badge-ok { color: #4caf50; }
  .badge-err { color: #b54242; }
  .badge-warn { color: #ff9800; }
  .section-title { font-size: 15px; font-weight: 600; color: var(--text, #eee); margin: 0 0 10px 0; }
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
// <ytdlp-panel> — статус/установка/обновление yt-dlp
// ─────────────────────────────────────────────────────────────────────────────

class YtdlpPanel extends HTMLElement {
  private root!: ShadowRoot
  private busy = false

  connectedCallback() {
    if (this.root) return
    this.root = this.attachShadow({ mode: 'open' })
    this.root.innerHTML = `
      <style>${STYLE}</style>
      <div>
        <h3 class="section-title">Управление yt-dlp</h3>
        <div class="row"><label>Статус:</label><span id="status" class="hint">Проверка…</span></div>
        <div class="row"><label>Версия:</label><span id="version" class="hint">—</span></div>
        <div class="row"><label>Путь:</label><span id="path" class="hint" style="word-break:break-all; flex:1;"></span></div>
        <div class="row"><label>FFmpeg:</label><span id="ffmpeg" class="hint">—</span></div>
        <div class="row"><label>Deno:</label><span id="deno" class="hint">—</span></div>
        <div class="row" style="margin-top:10px;">
          <button id="install" class="primary">Установить yt-dlp</button>
          <button id="checkUpdate" style="display:none;">Проверить обновление</button>
          <button id="installUpdate" class="primary" style="display:none;">Обновить</button>
          <button id="installDeno">Установить Deno</button>
          <button id="setDir">Изменить путь</button>
          <button id="setFfmpeg">Путь к FFmpeg</button>
        </div>
        <div id="progress" class="progress-container">
          <div class="progress-status" id="progressStatus">Подготовка…</div>
          <div class="progress-track"><div class="progress-bar" id="progressBar"></div></div>
        </div>
      </div>`

    this.root.getElementById('install')!.addEventListener('click', () => this.onInstall())
    this.root.getElementById('checkUpdate')!.addEventListener('click', () => this.onCheckUpdate())
    this.root.getElementById('installUpdate')!.addEventListener('click', () => this.onInstallUpdate())
    this.root.getElementById('installDeno')!.addEventListener('click', () => this.onInstallDeno())
    this.root.getElementById('setDir')!.addEventListener('click', () => this.onSetDir())
    this.root.getElementById('setFfmpeg')!.addEventListener('click', () => this.onSetFfmpeg())

    void this.refresh()
  }

  disconnectedCallback() {}

  private showProgress(on: boolean) {
    this.root.getElementById('progress')!.style.display = on ? 'block' : 'none'
  }

  private setProgress(pct: number) {
    const bar = this.root.getElementById('progressBar') as HTMLElement
    bar.style.width = `${pct}%`
  }

  private async refresh() {
    let st: YtdlpStatus
    try {
      st = await getYtdlpStatus()
    } catch (e) {
      this.root.getElementById('status')!.textContent = `Ошибка: ${e}`
      return
    }

    this.root.getElementById('status')!.textContent = st.message
    this.root.getElementById('version')!.textContent = st.version ?? '—'
    this.root.getElementById('path')!.textContent = st.exe_path

    const ffmpegEl = this.root.getElementById('ffmpeg')!
    if (st.ffmpeg_installed) {
      ffmpegEl.innerHTML = `<span class="badge-ok">Найден</span> ${esc(st.ffmpeg_path ?? '')}`
    } else {
      ffmpegEl.innerHTML = '<span class="badge-warn">Не найден (нужен для склейки)</span>'
    }

    const denoEl = this.root.getElementById('deno')!
    if (st.deno_installed) {
      denoEl.innerHTML = `<span class="badge-ok">Установлен</span> ${esc(st.deno_path ?? '')}`
    } else {
      denoEl.innerHTML = '<span class="badge-warn">Не установлен (нужен для n-sig)</span>'
    }

    const installed = st.installed
    this.root.getElementById('install')!.style.display = installed ? 'none' : 'inline-block'
    this.root.getElementById('checkUpdate')!.style.display = installed ? 'inline-block' : 'none'
    this.root.getElementById('installUpdate')!.style.display = 'none'
  }

  private async onInstall() {
    if (this.busy) return
    this.busy = true
    const btn = this.root.getElementById('install') as HTMLButtonElement
    btn.disabled = true
    this.showProgress(true)
    this.setProgress(0)
    this.root.getElementById('progressStatus')!.textContent = 'Скачивание yt-dlp...'
    try {
      await installYtdlp()
      toast('yt-dlp установлен!')
      await this.refresh()
    } catch (e) {
      toast(`Ошибка установки: ${e}`, 'error')
    } finally {
      btn.disabled = false
      this.busy = false
      this.showProgress(false)
    }
  }

  private async onCheckUpdate() {
    if (this.busy) return
    this.busy = true
    const btn = this.root.getElementById('checkUpdate') as HTMLButtonElement
    btn.disabled = true
    try {
      const newVer = await checkYtdlpUpdate()
      if (newVer) {
        this.root.getElementById('status')!.textContent = `Доступно обновление: ${newVer}`
        this.root.getElementById('installUpdate')!.style.display = 'inline-block'
      } else {
        this.root.getElementById('status')!.textContent = 'yt-dlp актуален'
      }
    } catch (e) {
      toast(`Ошибка проверки: ${e}`, 'error')
    } finally {
      btn.disabled = false
      this.busy = false
    }
  }

  private async onInstallUpdate() {
    if (this.busy) return
    this.busy = true
    const btn = this.root.getElementById('installUpdate') as HTMLButtonElement
    btn.disabled = true
    this.showProgress(true)
    this.setProgress(0)
    this.root.getElementById('progressStatus')!.textContent = 'Обновление yt-dlp...'
    try {
      await installYtdlpUpdate()
      toast('yt-dlp обновлён!')
      await this.refresh()
    } catch (e) {
      toast(`Ошибка обновления: ${e}`, 'error')
    } finally {
      btn.disabled = false
      this.busy = false
      this.showProgress(false)
    }
  }

  private async onInstallDeno() {
    if (this.busy) return
    this.busy = true
    const btn = this.root.getElementById('installDeno') as HTMLButtonElement
    btn.disabled = true
    this.showProgress(true)
    this.setProgress(0)
    this.root.getElementById('progressStatus')!.textContent = 'Скачивание Deno...'
    try {
      await installDeno()
      toast('Deno установлен!')
      await this.refresh()
    } catch (e) {
      toast(`Ошибка установки Deno: ${e}`, 'error')
    } finally {
      btn.disabled = false
      this.busy = false
      this.showProgress(false)
    }
  }

  private async onSetDir() {
    try {
      const sel = await openDialog({ directory: true })
      if (!sel) return
      const path = Array.isArray(sel) ? sel[0] : sel
      if (!path) return
      await setYtdlpDir(path)
      toast('Путь к yt-dlp изменён')
      await this.refresh()
    } catch (e) {
      toast(`Ошибка: ${e}`, 'error')
    }
  }

  private async onSetFfmpeg() {
    try {
      const sel = await openDialog({ directory: true })
      if (!sel) return
      const path = Array.isArray(sel) ? sel[0] : sel
      if (!path) return
      await setFfmpegPath(path)
      toast('Путь к FFmpeg изменён')
      await this.refresh()
    } catch (e) {
      toast(`Ошибка: ${e}`, 'error')
    }
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// <ytdlp-download-panel> — скачивание YouTube-видео
// ─────────────────────────────────────────────────────────────────────────────

class YtdlpDownloadPanel extends HTMLElement {
  private root!: ShadowRoot
  private busy = false
  private unlisten?: () => void
  private unlistenProgress?: () => void
  private unlistenStatus?: () => void
  private unlistenFinished?: () => void
  private unlistenError?: () => void
  private unlistenCancelled?: () => void

  connectedCallback() {
    if (this.root) return
    this.root = this.attachShadow({ mode: 'open' })
    this.root.innerHTML = `
      <style>${STYLE}</style>
      <div>
        <h3 class="section-title">Скачивание видео</h3>
        <div class="row">
          <label>URL видео:</label>
          <input type="text" id="url" placeholder="https://www.youtube.com/watch?v=..." style="flex:1; min-width:300px;">
        </div>
        <div class="row">
          <label>Формат:</label>
          <select id="format">
            <option value="mp4">MP4 (Без конвертации)</option>
            <option value="best">Лучший (Любой формат)</option>
            <option value="mp3">Только Аудио (MP3)</option>
          </select>
          <label>Качество:</label>
          <select id="quality">
            <option value="max">Максимальное</option>
            <option value="1080">1080p</option>
            <option value="720">720p</option>
            <option value="480">480p</option>
            <option value="360">360p</option>
          </select>
        </div>
        <div class="row">
          <label>Авторизация:</label>
          <select id="authMode">
            <option value="none">Без авторизации</option>
            <option value="browser">Из браузера</option>
            <option value="file">Cookies.txt</option>
          </select>
          <select id="browserSelect" style="display:none;">
            <option value="chrome">Chrome</option>
            <option value="edge">Edge</option>
            <option value="firefox">Firefox</option>
            <option value="brave">Brave</option>
            <option value="opera">Opera</option>
            <option value="vivaldi">Vivaldi</option>
          </select>
          <input type="text" id="cookiesFile" placeholder="Путь к cookies.txt" style="display:none; flex:1;">
          <button id="browseCookies" style="display:none;">Обзор</button>
        </div>
        <div class="row">
          <label>Сохранить в:</label>
          <input type="text" id="outputDir" placeholder="Папка для сохранения" style="flex:1;">
          <button id="browseDir">Обзор</button>
        </div>
        <div class="row" style="margin-top:10px;">
          <button id="download" class="primary" style="min-width:120px;">Скачать</button>
          <button id="cancel" class="danger" style="display:none;">Отмена</button>
        </div>
        <div id="progress" class="progress-container">
          <div class="progress-status" id="progressStatus">Ожидание...</div>
          <div class="progress-track"><div class="progress-bar" id="progressBar"></div></div>
        </div>
      </div>`

    this.root.getElementById('download')!.addEventListener('click', () => this.onDownload())
    this.root.getElementById('cancel')!.addEventListener('click', () => this.onCancel())
    this.root.getElementById('browseDir')!.addEventListener('click', () => this.onBrowseDir())
    this.root.getElementById('browseCookies')!.addEventListener('click', () => this.onBrowseCookies())
    this.root.getElementById('authMode')!.addEventListener('change', () => this.onAuthChange())

    const authMode = this.root.getElementById('authMode') as HTMLSelectElement
    authMode.addEventListener('change', () => this.onAuthChange())

    void this.setupListeners()
    void this.loadDefaults()
  }

  disconnectedCallback() {
    this.unlisten?.()
    this.unlistenProgress?.()
    this.unlistenStatus?.()
    this.unlistenFinished?.()
    this.unlistenError?.()
    this.unlistenCancelled?.()
  }

  private async setupListeners() {
    this.unlistenProgress = await listen<number>('ytdlp:download-progress', (e) => {
      const pct = (e as { payload: number }).payload
      const bar = this.root.getElementById('progressBar') as HTMLElement
      bar.style.width = `${pct}%`
    })
    this.unlistenStatus = await listen<string>('ytdlp:download-status', (e) => {
      this.root.getElementById('progressStatus')!.textContent = (e as { payload: string }).payload
    })
    this.unlistenFinished = await listen('ytdlp:download-finished', () => {
      this.root.getElementById('progressStatus')!.textContent = 'Готово! Видео скачано.'
      this.setBusy(false)
      toast('Видео успешно скачано!')
    })
    this.unlistenError = await listen<string>('ytdlp:download-error', (e) => {
      const msg = (e as { payload: string }).payload
      this.root.getElementById('progressStatus')!.textContent = `Ошибка: ${msg}`
      this.setBusy(false)
      toast(`Ошибка: ${msg}`, 'error')
    })
    this.unlistenCancelled = await listen('ytdlp:download-cancelled', () => {
      this.root.getElementById('progressStatus')!.textContent = 'Отменено'
      this.setBusy(false)
    })
  }

  private async loadDefaults() {
    try {
      const cfg = await invoke<{ download_path?: string }>('plugin:ytdlp|get_ytdlp_status', {})
      const outputDir = this.root.getElementById('outputDir') as HTMLInputElement
      if (cfg.download_path) {
        outputDir.value = cfg.download_path
      }
    } catch { /* ignore */ }
  }

  private onAuthChange() {
    const mode = (this.root.getElementById('authMode') as HTMLSelectElement).value
    const browserSelect = this.root.getElementById('browserSelect') as HTMLSelectElement
    const cookiesFile = this.root.getElementById('cookiesFile') as HTMLInputElement
    const browseCookies = this.root.getElementById('browseCookies') as HTMLButtonElement

    browserSelect.style.display = mode === 'browser' ? 'inline-block' : 'none'
    cookiesFile.style.display = mode === 'file' ? 'inline-block' : 'none'
    browseCookies.style.display = mode === 'file' ? 'inline-block' : 'none'
  }

  private setBusy(busy: boolean) {
    this.busy = busy
    const dlBtn = this.root.getElementById('download') as HTMLButtonElement
    const cancelBtn = this.root.getElementById('cancel') as HTMLButtonElement
    dlBtn.disabled = busy
    dlBtn.style.display = busy ? 'none' : 'inline-block'
    cancelBtn.style.display = busy ? 'inline-block' : 'none'
    this.root.getElementById('progress')!.style.display = busy ? 'block' : 'none'
  }

  private async onBrowseDir() {
    try {
      const sel = await openDialog({ directory: true })
      if (!sel) return
      const path = Array.isArray(sel) ? sel[0] : sel
      if (!path) return
      ;(this.root.getElementById('outputDir') as HTMLInputElement).value = path
    } catch (e) {
      toast(`Ошибка: ${e}`, 'error')
    }
  }

  private async onBrowseCookies() {
    try {
      const sel = await openDialog({ filters: [{ name: 'Text', extensions: ['txt'] }] })
      if (!sel) return
      const path = Array.isArray(sel) ? sel[0] : sel
      if (!path) return
      ;(this.root.getElementById('cookiesFile') as HTMLInputElement).value = path
    } catch (e) {
      toast(`Ошибка: ${e}`, 'error')
    }
  }

  private async onDownload() {
    if (this.busy) return

    const url = (this.root.getElementById('url') as HTMLInputElement).value.trim()
    if (!url) {
      toast('Введите URL видео', 'error')
      return
    }

    const format = (this.root.getElementById('format') as HTMLSelectElement).value
    const quality = (this.root.getElementById('quality') as HTMLSelectElement).value
    const outputDir = (this.root.getElementById('outputDir') as HTMLInputElement).value.trim()
    const authMode = (this.root.getElementById('authMode') as HTMLSelectElement).value
    const browser = (this.root.getElementById('browserSelect') as HTMLSelectElement).value
    const cookiesFile = (this.root.getElementById('cookiesFile') as HTMLInputElement).value.trim()

    if (!outputDir) {
      toast('Укажите папку для сохранения', 'error')
      return
    }

    if (authMode === 'file' && !cookiesFile) {
      toast('Укажите путь к cookies.txt', 'error')
      return
    }

    this.setBusy(true)
    this.root.getElementById('progressBar')!.style.width = '0%'
    this.root.getElementById('progressStatus')!.textContent = 'Подготовка...'

    try {
      await downloadVideo({
        url,
        format,
        quality,
        outputDir,
        authMode,
        browser: authMode === 'browser' ? browser : null,
        cookiesFile: authMode === 'file' ? cookiesFile : null,
      })
    } catch (e) {
      toast(`Ошибка скачивания: ${e}`, 'error')
      this.setBusy(false)
    }
  }

  private async onCancel() {
    try {
      await cancelDownload()
    } catch (e) {
      toast(`Ошибка отмены: ${e}`, 'error')
    }
  }
}

if (!customElements.get('ytdlp-panel')) {
  customElements.define('ytdlp-panel', YtdlpPanel)
}
if (!customElements.get('ytdlp-download-panel')) {
  customElements.define('ytdlp-download-panel', YtdlpDownloadPanel)
}

export { esc, formatBytes, toast }
