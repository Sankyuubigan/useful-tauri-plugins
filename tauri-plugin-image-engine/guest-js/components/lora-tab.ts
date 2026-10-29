/**
 * <image-lora-tab> — вкладка «LoRA» медиа-панели: каталог HF-репозитория
 * («King Orch Loras», подпапки = базы моделей), скачивание выбранных файлов
 * в единую папку loras. Sidecar-описание `<стем>.md` качается вместе с
 * весами автоматически — агент ({{ video_loras }}) сразу видит новые лоры.
 */
import { STYLE, esc, formatBytes, toast, progressText } from './ui-shared'
import { downloadRemoteLora, listRemoteLoras, type RemoteLoraFile } from '../index'
import { listen } from '@tauri-apps/api/event'

export class LoraTabElement extends HTMLElement {
  private root!: ShadowRoot
  private busy = false
  private unlisten?: () => void
  private files: RemoteLoraFile[] = []
  private repo = ''
  private localDir = ''

  connectedCallback() {
    if (this.root) return
    this.root = this.attachShadow({ mode: 'open' })
    this.root.innerHTML = `
      <style>${STYLE}</style>
      <div>
        <div class="row">
          <label>Каталог:</label>
          <span id="repo" class="hint" style="flex:1; word-break:break-all;">…</span>
          <button id="refresh" class="secondary">Обновить</button>
        </div>
        <div class="row"><label>Папка:</label><span id="dir" class="hint" style="word-break:break-all; flex:1;"></span></div>
        <div class="hint" id="note">Лора качается вместе со своим описанием (.md) — агент сразу видит, что появилось. Установленное отмечено ✓.</div>
        <div id="groups"></div>
        <div class="row" style="margin-top:10px;">
          <button id="downloadAll" class="primary" style="display:none;"></button>
        </div>
        <div id="progress" class="progress-container">
          <div class="progress-status" id="progressStatus">0 MB / 0 MB</div>
          <div class="progress-track"><div class="progress-bar" id="progressBar"></div></div>
        </div>
      </div>`

    this.root.getElementById('refresh')!.addEventListener('click', () => void this.refresh())
    this.root.getElementById('downloadAll')!.addEventListener('click', () => void this.downloadAll())

    void this.refresh()
    void listen('downloader:progress', (e) => {
      const p = (e as { payload: { downloaded: number; total: number; kind?: string; status?: string } }).payload
      if (p.kind !== 'lora') return
      const bar = this.root.getElementById('progressBar') as HTMLElement
      bar.style.width = `${p.total > 0 ? (p.downloaded / p.total) * 100 : 0}%`
      this.root.getElementById('progressStatus')!.textContent = progressText(p.downloaded, p.total)
      if (p.status === 'done' || p.status === 'error') {
        setTimeout(() => { this.root.getElementById('progress')!.style.display = 'none' }, 400)
      }
    }).then((u) => { this.unlisten = u }).catch(() => {})
  }

  disconnectedCallback() {
    this.unlisten?.()
  }

  private async refresh() {
    if (this.busy) return
    const groupsEl = this.root.getElementById('groups')!
    this.root.getElementById('repo')!.textContent = 'загрузка каталога…'
    try {
      const info = await listRemoteLoras()
      this.files = info.files
      this.repo = info.repo
      this.localDir = info.local_dir
      this.root.getElementById('repo')!.textContent = `huggingface.co/${info.repo}`
      this.root.getElementById('dir')!.textContent = info.local_dir
      this.renderGroups()
    } catch (e) {
      this.root.getElementById('repo')!.textContent = 'каталог недоступен'
      groupsEl.innerHTML = `<div class="hint">Ошибка: ${esc(String(e))}</div>`
    }
  }

  private renderGroups() {
    const groupsEl = this.root.getElementById('groups')!
    groupsEl.innerHTML = ''
    const bases = [...new Set(this.files.map((f) => f.base))]
    for (const base of bases) {
      const box = document.createElement('div')
      const title = document.createElement('div')
      title.className = 'hint'
      title.style.margin = '8px 0 4px'
      title.textContent = `База: ${base}`
      box.appendChild(title)
      const ul = document.createElement('ul')
      ul.className = 'files'
      for (const f of this.files.filter((x) => x.base === base)) {
        // Sidecar-файлы не показываем отдельными строками: они едут вместе
        // с весами (чистый список — только лоры).
        if (!f.is_lora) continue
        const li = document.createElement('li')
        li.className = f.exists_locally ? 'ok' : 'missing'
        const hasSidecar = this.files.some(
          (s) => !s.is_lora && s.base === f.base && s.filename === `${f.filename.replace(/\.[^.]+$/, '')}.md`,
        )
        const size = f.size_bytes ? ` — ${formatBytes(f.size_bytes)}` : ''
        li.textContent = `${f.exists_locally ? '✓' : '○'} ${esc(f.filename)}${size}${hasSidecar ? ' (с описанием)' : ''}`
        if (!f.exists_locally) {
          const btn = document.createElement('button')
          btn.className = 'secondary'
          btn.textContent = 'Скачать'
          btn.dataset.path = f.path
          btn.addEventListener('click', () => void this.downloadOne(f))
          li.appendChild(document.createTextNode(' '))
          li.appendChild(btn)
        }
        ul.appendChild(li)
      }
      box.appendChild(ul)
      groupsEl.appendChild(box)
    }
    const missing = this.files.filter((f) => f.is_lora && !f.exists_locally)
    const allBtn = this.root.getElementById('downloadAll') as HTMLButtonElement
    allBtn.style.display = missing.length > 0 ? 'inline-block' : 'none'
    allBtn.textContent = `Скачать всё недокачанное (${missing.length})`
  }

  /** Скачать веса + их sidecar-описание (если есть в каталоге). */
  private async fetchWithSidecar(f: RemoteLoraFile) {
    await downloadRemoteLora(f.path)
    const stem = f.filename.replace(/\.[^.]+$/, '')
    const sidecar = this.files.find(
      (s) => !s.is_lora && s.base === f.base && s.filename === `${stem}.md`,
    )
    if (sidecar) {
      await downloadRemoteLora(sidecar.path).catch((e) => {
        // Описание вспомогательное: его провал не отменяет скачанные веса.
        toast(`Описание ${sidecar.filename} не скачалось: ${e}`, 'error')
      })
    }
  }

  private async downloadOne(f: RemoteLoraFile) {
    if (this.busy) return
    this.busy = true
    this.root.getElementById('progress')!.style.display = 'block'
    try {
      await this.fetchWithSidecar(f)
      toast(`${f.filename} скачан.`)
    } catch (e) {
      toast(`Ошибка скачивания ${f.filename}: ${e}`, 'error')
    } finally {
      this.busy = false
      await this.refresh()
    }
  }

  private async downloadAll() {
    if (this.busy) return
    const missing = this.files.filter((f) => f.is_lora && !f.exists_locally)
    if (missing.length === 0) return
    this.busy = true
    const btn = this.root.getElementById('downloadAll') as HTMLButtonElement
    btn.disabled = true
    this.root.getElementById('progress')!.style.display = 'block'
    let failed = 0
    try {
      for (const f of missing) {
        try {
          await this.fetchWithSidecar(f)
        } catch (e) {
          failed += 1
          toast(`${f.filename}: ${e}`, 'error')
        }
      }
      toast(failed === 0 ? 'Все лоры скачаны!' : `Скачано с ошибками: ${failed} шт.`)
    } finally {
      this.busy = false
      btn.disabled = false
      await this.refresh()
    }
  }
}

if (!customElements.get('image-lora-tab')) {
  customElements.define('image-lora-tab', LoraTabElement)
}
