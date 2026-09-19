import {
  getLastLogsPath,
  logFront,
  onLogMessage,
  saveLogsToFile,
} from './index'
import type { UnlistenFn } from '@tauri-apps/api/event'

/**
 * <logs-panel></logs-panel>
 *
 * Фреймворк-агностичная вкладка «Логи»: последние строки единого логгера
 * (событие `logs:message`) + кнопки Копировать / Сохранить / Очистить и
 * подсказка с путём dev-зеркала (core rules §2.5.1).
 *
 * Тематизируется CSS-переменными хоста:
 *   --text, --text-muted, --border, --font, --mono-font,
 *   --logs-toolbar-bg, --logs-btn-bg, --logs-area-bg, --logs-area-text.
 */
class LogsPanel extends HTMLElement {
  private root!: ShadowRoot
  private unlisten: UnlistenFn | null = null
  private lines: string[] = []
  private nearBottom = true
  private readonly maxLines = 4000

  connectedCallback() {
    if (!this.shadowRoot) {
      this.root = this.attachShadow({ mode: 'open' })
    } else {
      this.root = this.shadowRoot
    }
    this.render()
  }

  disconnectedCallback() {
    this.unlisten?.()
    this.unlisten = null
  }

  private render() {
    this.root.innerHTML = `
      <style>
        :host { display: flex; flex-direction: column; height: 100%; min-height: 0;
               font-family: var(--font, system-ui, sans-serif); }
        .toolbar { display: flex; align-items: center; gap: 8px; padding: 6px 10px;
                   background: var(--logs-toolbar-bg, #2a2d2e);
                   border-bottom: 1px solid var(--border, #3a3d3e);
                   border-top-left-radius: 8px; border-top-right-radius: 8px; }
        .title { font-weight: 600; color: var(--text, #e0e0e0); font-size: 13px; white-space: nowrap; }
        .hint { color: var(--text-muted, #9a9a9a); font-size: 11px; overflow: hidden;
                text-overflow: ellipsis; white-space: nowrap; }
        .spacer { flex: 1; }
        button { font: inherit; font-size: 12px; padding: 4px 10px; cursor: pointer;
                 border: 1px solid var(--border, #3a3d3e); border-radius: 6px;
                 background: var(--logs-btn-bg, #3a3d3e); color: var(--text, #e0e0e0); }
        button:hover { filter: brightness(1.15); }
        textarea { flex: 1; resize: none; width: 100%; box-sizing: border-box; border: none;
                   padding: 8px; background: var(--logs-area-bg, #1e1e1e);
                   color: var(--logs-area-text, #d4d4d4);
                   font-family: var(--mono-font, Consolas, monospace); font-size: 12px;
                   line-height: 1.45; outline: none;
                   border-bottom-left-radius: 8px; border-bottom-right-radius: 8px; }
      </style>
      <div class="toolbar">
        <span class="title">Логи системы</span>
        <span class="hint" id="hint"></span>
        <div class="spacer"></div>
        <button id="copy">Копировать</button>
        <button id="save">Сохранить</button>
        <button id="clear">Очистить</button>
      </div>
      <textarea id="area" readonly spellcheck="false" placeholder="Здесь появится лог приложения…"></textarea>`

    const area = this.root.getElementById('area') as HTMLTextAreaElement
    area.addEventListener('scroll', () => {
      this.nearBottom = area.scrollHeight - area.scrollTop - area.clientHeight < 40
    })

    this.root.getElementById('copy')!.addEventListener('click', () => {
      void this.onCopy()
    })
    this.root.getElementById('save')!.addEventListener('click', () => {
      void this.onSave()
    })
    this.root.getElementById('clear')!.addEventListener('click', () => {
      this.onClear()
    })

    void getLastLogsPath()
      .then((p) => {
        if (p) (this.root.getElementById('hint') as HTMLElement).textContent = `Файл: ${p}`
      })
      .catch(() => {})

    void onLogMessage((line) => this.appendLine(line))
      .then((u) => {
        this.unlisten = u
      })
      .catch((e) => {
        logFront(`[logs-panel] не удалось подписаться на лог: ${String(e)}`)
      })
  }

  private appendLine(line: string) {
    this.lines.push(line)
    const area = this.root.getElementById('area') as HTMLTextAreaElement
    if (!area) return

    let trimmed = false
    if (this.lines.length > this.maxLines) {
      const overflow = this.lines.length - this.maxLines
      this.lines.splice(0, overflow)
      trimmed = true
    }

    if (trimmed) {
      area.value = this.lines.join('\n')
    } else {
      area.value += `${line}\n`
    }
    if (this.nearBottom) {
      area.scrollTop = area.scrollHeight
    }
  }

  private onClear() {
    this.lines = []
    this.nearBottom = true
    const area = this.root.getElementById('area') as HTMLTextAreaElement
    if (area) {
      area.value = ''
      area.scrollTop = 0
    }
  }

  private async onCopy() {
    const text = this.lines.join('\n')
    try {
      await navigator.clipboard.writeText(text)
      logFront(`[logs-panel] логи скопированы в буфер обмена (${this.lines.length} строк)`)
    } catch (e) {
      logFront(`[logs-panel] не удалось скопировать логи: ${String(e)}`)
    }
  }

  private async onSave() {
    const content = this.lines.join('\n')
    const saved = await saveLogsToFile(content).catch(() => null)
    if (saved) logFront(`[logs-panel] логи сохранены в ${saved}`)
  }
}

if (!customElements.get('logs-panel')) {
  customElements.define('logs-panel', LogsPanel)
}