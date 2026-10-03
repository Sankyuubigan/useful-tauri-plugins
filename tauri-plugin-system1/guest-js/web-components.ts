import { invoke } from '@tauri-apps/api/core'
import {
  downloadAll,
  getStatus,
  onDownloadProgress,
  removeModel,
  type DownloadProgress,
  type FileStatus,
  type StatusReport,
} from './index'

/**
 * Автономный тост. Обязателен по PLUGIN_STANDARD §12.3: панель плагина не
 * импортирует ничего из хоста, а `console.error` пользователю не виден.
 */
function toast(message: string, kind: 'success' | 'error' = 'success'): void {
  const element = document.createElement('div')
  element.textContent = message
  element.style.cssText =
    'position:fixed; right:16px; bottom:16px; z-index:5000; max-width:420px; padding:10px 14px;' +
    'border-radius:8px; font:13px var(--font, system-ui, sans-serif); box-shadow:0 3px 14px rgba(0,0,0,.4);' +
    `background:${kind === 'success' ? 'var(--primary, #4a90d9)' : 'var(--danger, #b54242)'}; color:#fff;`
  document.body.appendChild(element)
  setTimeout(() => element.remove(), 3500)
}

/** Запись в лог приложения. `console` пользователю недоступен (F12 закрыт). */
function logPlugin(message: string): void {
  void invoke('plugin:logs|log_frontend_event', { level: 'FE', msg: message }).catch(() => {})
}

const STYLE = `
  :host {
    display: block;
    font: 13px var(--font, system-ui, sans-serif);
    color: var(--text, #e6e6e6);
  }
  .card {
    border: 1px solid var(--border, #2c3240);
    border-radius: 10px;
    padding: 14px 16px;
    background: var(--bg-2, #161a22);
  }
  h3 { margin: 0 0 10px; font-size: 14px; font-weight: 600; }
  .lead { margin: 0 0 12px; color: var(--text-dim, #9aa3b2); line-height: 1.45; }
  table { width: 100%; border-collapse: collapse; }
  th {
    text-align: left; font-weight: 600; font-size: 12px; color: var(--text-dim, #9aa3b2);
    padding: 4px 8px 4px 0; border-bottom: 1px solid var(--border, #2c3240);
  }
  td { padding: 5px 8px 5px 0; vertical-align: top; border-bottom: 1px solid var(--border, #2c3240); }
  td.size { text-align: right; white-space: nowrap; color: var(--text-dim, #9aa3b2); }
  .group {
    font-size: 11px; text-transform: uppercase; letter-spacing: .04em;
    color: var(--text-dim, #9aa3b2); padding-top: 10px;
  }
  .group:first-of-type { padding-top: 0; }
  .ok { color: var(--success, #4caf7d); white-space: nowrap; }
  .bad { color: var(--danger, #b54242); white-space: nowrap; }
  .muted { color: var(--text-dim, #9aa3b2); }
  .row { display: flex; justify-content: space-between; gap: 12px; padding: 3px 0; }
  .label { color: var(--text-dim, #9aa3b2); }
  .value { text-align: right; word-break: break-all; }
  .actions { display: flex; gap: 8px; margin-top: 12px; }
  button {
    font: inherit;
    padding: 6px 12px;
    border-radius: 6px;
    border: 1px solid var(--border, #2c3240);
    background: var(--bg-3, #1e232d);
    color: inherit;
    cursor: pointer;
  }
  button:hover:not(:disabled) { border-color: var(--primary, #4a90d9); }
  button:disabled { opacity: .5; cursor: default; }
  progress { width: 100%; margin-top: 10px; height: 8px; }
  code { font-family: ui-monospace, Consolas, monospace; font-size: 12px; }
`

/** Метка группы файла. Показывается как отдельная строка-разделитель. */
const GROUP_LABEL: Record<string, string> = {
  runtime: 'Движок',
  model: 'Модель',
  tokenizer: 'Токенизатор',
}

/** Человеческий размер файла. */
function humanSize(bytes: number): string {
  if (bytes <= 0) return '—'
  if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} МБ`
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(0)} КБ`
  return `${bytes} Б`
}

/**
 * Экранирование для вставки в `innerHTML`.
 *
 * Пути и имена файлов приходят с диска, то есть это внешние данные. Без
 * экранирования имя вида `<img onerror=…>` выполнилось бы в контексте панели.
 */
function esc(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

/**
 * Панель System-1: проверка комплекта файлов, установка, статус.
 *
 * Показывает ТОЛЬКО факты, полученные от бэкенда: наличие и размер каждого
 * файла, реальное устройство исполнения. Ничего не достраивает на клиенте —
 * иначе панель врала бы при расхождении с реальностью.
 *
 * Имена методов НЕ пересекаются с `HTMLElement`: `style` и `remove` уже заняты
 * самим DOM, и перекрытие ломает и типизацию, и вызовы (`remove()` у элемента
 * удаляет его из DOM — молчаливая, но неприятная ошибка).
 */
class System1Panel extends HTMLElement {
  private status: StatusReport | null = null
  private busy = false

  connectedCallback(): void {
    this.attachShadow({ mode: 'open' })
    this.shadowRoot!.appendChild(this.buildStyle())
    this.shadowRoot!.appendChild(this.buildBody())
    void this.refresh()
    void onDownloadProgress((progress) => this.onProgress(progress))
  }

  disconnectedCallback(): void {
    this.shadowRoot?.replaceChildren()
  }

  private buildStyle(): HTMLStyleElement {
    const element = document.createElement('style')
    element.textContent = STYLE
    return element
  }

  private buildBody(): HTMLElement {
    const card = document.createElement('div')
    card.className = 'card'
    card.innerHTML = `
      <h3>Модель быстрых решений (System-1 / Laya)</h3>
      <p class="lead">
        Работает без CUDA и DirectML — только CPU. Файлы не входят в установщик,
        а скачиваются с сервера и проверяются по контрольной сумме.
        Всё хранится в данных приложения и не занимает место рядом с программой.
      </p>
      <div id="files"><div class="muted">Проверка файлов…</div></div>
      <progress id="progress" max="100" value="0" hidden></progress>
      <div id="summary"></div>
      <div class="actions">
        <button id="refresh">Проверить</button>
        <button id="download">Скачать всё</button>
        <button id="remove">Удалить модель</button>
      </div>
    `
    card.querySelector('#refresh')!.addEventListener('click', () => void this.refresh())
    card.querySelector('#download')!.addEventListener('click', () => void this.download())
    card.querySelector('#remove')!.addEventListener('click', () => void this.deleteModel())
    return card
  }

  private onProgress(progress: DownloadProgress): void {
    const bar = this.shadowRoot?.querySelector('#progress') as HTMLProgressElement | null
    if (!bar || !progress.total) return
    bar.hidden = false
    bar.value = ((progress.downloaded ?? 0) / progress.total) * 100
  }

  private async refresh(): Promise<void> {
    try {
      this.status = await getStatus()
      this.render()
    } catch (error) {
      logPlugin(`[system1] статус не получен: ${String(error)}`)
      toast(`Не удалось получить статус System-1: ${String(error)}`, 'error')
    }
  }

  private async download(): Promise<void> {
    if (this.busy) return
    this.busy = true
    this.setButtons()
    const missing = this.status?.files.filter((file) => !file.present) ?? []
    const what = missing.length ? ` (${missing.length} из ${this.status?.files.length ?? 0})` : ''
    try {
      await downloadAll()
      toast('Файлы System-1 установлены')
      logPlugin(`[system1] комплект установлен${what}`)
      await this.refresh()
    } catch (error) {
      logPlugin(`[system1] установка не удалась: ${String(error)}`)
      toast(`Не удалось установить файлы System-1: ${String(error)}`, 'error')
    } finally {
      this.busy = false
      this.setButtons()
    }
  }

  private async deleteModel(): Promise<void> {
    if (this.busy) return
    this.busy = true
    this.setButtons()
    try {
      await removeModel()
      toast('Модель System-1 удалена')
      logPlugin('[system1] модель удалена')
      await this.refresh()
    } catch (error) {
      logPlugin(`[system1] удаление не удалось: ${String(error)}`)
      toast(`Не удалось удалить модель: ${String(error)}`, 'error')
    } finally {
      this.busy = false
      this.setButtons()
    }
  }

  private setButtons(): void {
    for (const id of ['#refresh', '#download', '#remove']) {
      const button = this.shadowRoot?.querySelector(id) as HTMLButtonElement | null
      if (button) button.disabled = this.busy
    }
  }

  /** Одна строка таблицы файлов. */
  private renderFile(file: FileStatus): string {
    const state = file.present
      ? '<span class="ok">готов</span>'
      : '<span class="bad">нет</span>'
    const detail = file.present
      ? esc(file.action)
      : `<span class="muted">${esc(file.action)}</span>`
    return `
      <tr>
        <td><code>${esc(file.name)}</code><div class="muted">${detail}</div></td>
        <td class="size">${humanSize(file.sizeBytes)}</td>
        <td class="size">${state}</td>
      </tr>
    `
  }

  private render(): void {
    const filesTarget = this.shadowRoot?.querySelector('#files')
    const summaryTarget = this.shadowRoot?.querySelector('#summary')
    const status = this.status
    if (!filesTarget || !summaryTarget || !status) return

    // Файлы идут группами в порядке, который вернул бэкенд: сначала движок,
    // потом модель, потом токенизатор. Разделитель ставится при смене группы,
    // чтобы «Движок / Модель / Токенизатор» читались как три блока.
    let previousKind = ''
    const body = status.files
      .map((file) => {
        const header = file.kind !== previousKind
          ? `<tr><td class="group" colspan="3">${esc(GROUP_LABEL[file.kind] ?? file.kind)}</td></tr>`
          : ''
        previousKind = file.kind
        return header + this.renderFile(file)
      })
      .join('')

    filesTarget.innerHTML = `
      <table>
        <thead>
          <tr><th>Файл</th><th class="size">Размер</th><th class="size">Состояние</th></tr>
        </thead>
        <tbody>${body}</tbody>
      </table>
    `

    const allPresent = status.files.every((file) => file.present)
    const missing = status.files.filter((file) => !file.present)
    const verdict = allPresent
      ? '<span class="ok">все файлы на месте, можно работать</span>'
      : `<span class="bad">не хватает: ${esc(missing.map((file) => file.name).join(', '))}</span>`

    summaryTarget.innerHTML = `
      <div class="row" style="margin-top:10px">
        <span class="label">Готовность</span><span class="value">${verdict}</span>
      </div>
      <div class="row">
        <span class="label">Загружена в память</span>
        <span class="value">${status.loaded ? 'да' : 'нет'}</span>
      </div>
      <div class="row">
        <span class="label">Устройство</span>
        <span class="value">${status.device === 'cpu' ? 'CPU' : esc(status.device)}</span>
      </div>
      <div class="row">
        <span class="label">Движок</span>
        <span class="value"><code>${esc(status.runtime.version)}</code></span>
      </div>
      <div class="row">
        <span class="label">Папка данных</span>
        <span class="value"><code>${esc(status.modelsDir)}</code></span>
      </div>
    `
  }
}

if (!customElements.get('system1-panel')) {
  customElements.define('system1-panel', System1Panel)
}

export { System1Panel }