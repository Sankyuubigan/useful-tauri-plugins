/**
 * <image-bundle-tab service="image|video"> — одна вкладка медиа-блока:
 * селектор бандлов («скачать → активировать»), файлы, диск/память и папка
 * набора («Изменить путь…» — диалог → проверка файлов → set; «Сбросить путь» —
 * возврат к дефолту хранилища KingOrchData). Работает поверх BundleService
 * (services.ts) — один рендер на обе модальности.
 */
import { STYLE, esc, formatBytes, toast, progressText } from './ui-shared'
import { notifyBundleChanged } from '../index'
import { serviceFor, type BundleService, type TabInfo } from './services'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { listen } from '@tauri-apps/api/event'

export class BundleTabElement extends HTMLElement {
  private root!: ShadowRoot
  private busy = false
  private unlisten?: () => void
  private appliedBundle = ''
  /** Имя бандла, выбранного в селекте — источник правды всех операций вкладки
   * (скачать/докачать/удалить/активировать смотрят на НЕГО, а не на активный;
   * PRP 29.10.26 D1). Инициализируется активным при первом открытии. */
  private selected = ''
  private service?: BundleService
  /** Статусы бандлов из последнего refresh (fully_downloaded и файлы) —
   * рендер панели и гейт кнопок читают ТОЛЬКО отсюда (SSOT). */
  private statuses = new Map<string, TabInfo>()
  /** Что подтверждает модалка: файлы выбранного бандла или orphan-очистка пула. */
  private modalAction: 'bundle' | 'orphans' = 'bundle'
  /** Имена orphan-файлов из последнего рендера — список для удаления (SSOT). */
  private orphanNames: string[] = []

  connectedCallback() {
    if (this.root) return
    const kind = this.getAttribute('service') ?? 'image'
    const svc = serviceFor(kind)
    if (!svc) {
      this.textContent = `Неизвестный сервис вкладки: «${kind}» (ожидается image|video)`
      return
    }
    this.service = svc
    this.root = this.attachShadow({ mode: 'open' })
    this.root.innerHTML = `
      <style>${STYLE}</style>
      <div>
        <div class="row">
          <label>${esc(svc.kindLabel)}:</label>
          <select id="bundle" style="flex:1; min-width:220px;"><option value="">…</option></select>
          <button id="activate" class="primary" style="display:none;">Активировать</button>
        </div>
        <div class="row"><label>Состояние:</label><span id="state" class="hint" style="flex:1;"></span></div>
        <div class="row"><label>Набор:</label><span id="label" class="hint">…</span></div>
        <div id="note" class="hint"></div>
        <ul class="files" id="files"></ul>
        <div id="orphans" style="display:none;">
          <div class="row">
            <label>Неиспользуемые:</label>
            <span id="orphansSize" class="hint" style="flex:1;"></span>
            <button id="removeOrphans" class="danger">Удалить</button>
          </div>
          <ul class="files" id="orphanFiles"></ul>
        </div>
        <div class="row" style="margin-top:8px;"><label>Диск:</label><span id="disk" class="hint"></span></div>
        <div class="row"><label>Память:</label><span id="mem" class="hint"></span></div>
        <div class="row">
          <label>Папка:</label>
          <span id="dir" class="hint" style="word-break:break-all; flex:1;"></span>
          <button id="changeDir" class="secondary">Изменить путь…</button>
          <button id="resetDir" class="secondary" style="display:none;">Сбросить путь</button>
        </div>
        <div class="row" style="margin-top:10px;">
          <button id="download" class="primary">Скачать набор</button>
          <button id="resume" class="primary" style="display:none;">Докачать файлы</button>
          <button id="deleteFiles" class="danger" style="display:none;">Удалить файлы</button>
        </div>
        <div id="progress" class="progress-container">
          <div class="progress-status" id="progressStatus">0 MB / 0 MB</div>
          <div class="progress-track"><div class="progress-bar" id="progressBar"></div></div>
        </div>
      </div>
      <div id="overlay" class="overlay">
        <div class="box">
          <h3 id="overlayTitle">Удалить файлы набора?</h3>
          <p id="overlayMsg">Файлы с диска будут удалены БЕЗВОЗВРАТНО. Запись тоже исчезнет из списка.</p>
          <div class="overlay-buttons">
            <button id="modalCancel" class="secondary">Отмена</button>
            <button id="modalOk" class="danger">Удалить файлы</button>
          </div>
        </div>
      </div>`

    this.root.getElementById('bundle')!.addEventListener('change', () => this.onBundleChange())
    this.root.getElementById('activate')!.addEventListener('click', () => this.onActivate())
    this.root.getElementById('download')!.addEventListener('click', () => this.onDownload())
    this.root.getElementById('changeDir')!.addEventListener('click', () => this.onChangeDir())
    this.root.getElementById('resetDir')!.addEventListener('click', () => this.onResetDir())
    this.root.getElementById('resume')!.addEventListener('click', () => this.onResume())
    this.root.getElementById('deleteFiles')!.addEventListener('click', () => this.confirmDelete())
    this.root.getElementById('removeOrphans')!.addEventListener('click', () => this.confirmRemoveOrphans())
    this.root.getElementById('modalCancel')!.addEventListener('click', () => this.closeModal())
    this.root.getElementById('modalOk')!.addEventListener('click', () => void this.doDeleteFiles())

    void this.refresh()
    void listen('downloader:progress', (e) => {
      const p = (e as { payload: { downloaded: number; total: number; kind?: string; status?: string } }).payload
      if (p.kind && p.kind !== svc.progressKind) return
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
    const bar = this.root.getElementById('progressBar') as HTMLElement
    bar.style.width = `${total > 0 ? (downloaded / total) * 100 : 0}%`
    const st = this.root.getElementById('progressStatus')!
    st.textContent = progressText(downloaded, total)
  }

  private showProgress(on: boolean) {
    this.root.getElementById('progress')!.style.display = on ? 'block' : 'none'
  }

  /** Загрузка данных вкладки: применённый (активный) бандл, статусы всех
   * бандлов каталога в `statuses` (SSOT), затем рендер панели под ВЫБРАННЫЙ
   * в селекте бандл. Рендер — отдельный метод, чтобы смена селекта не пережимала весь fetch. */
  private async refresh() {
    const svc = this.service
    if (!svc) return
    let info
    try {
      info = await svc.info(null)
    } catch (e) {
      const label = this.root.getElementById('label')
      if (label) label.textContent = `Ошибка: ${e}`
      return
    }

    // Селектор бандлов: активный — текущий; остальные — с меткой скачанности.
    this.appliedBundle = info.bundle_name
    // Выбранный по умолчанию — активный (первое открытие вкладки).
    if (!this.selected) this.selected = info.bundle_name
    const sel = this.root.getElementById('bundle') as HTMLSelectElement
    let entries: Awaited<ReturnType<BundleService['entries']>> = []
    try { entries = await svc.entries() } catch { /* каталог встроен — просто без списка */ }
    sel.innerHTML = ''
    this.statuses = new Map()
    this.statuses.set(info.bundle_name, info)
    for (const entry of entries) {
      if (this.statuses.has(entry.name)) continue
      try { this.statuses.set(entry.name, await svc.info(entry.name)) } catch { /* нет на диске — метка ниже */ }
    }
    if (entries.length === 0) {
      const o = document.createElement('option')
      o.value = info.bundle_name
      o.textContent = info.label
      sel.appendChild(o)
    } else {
      for (const entry of entries) {
        const st = this.statuses.get(entry.name)
        const o = document.createElement('option')
        o.value = entry.name
        if (!st) {
          o.textContent = `${entry.label} (не скачан)`
        } else if (st.fully_downloaded) {
          o.textContent = `${st.label} — скачан${entry.is_default ? ' (по умолчанию)' : ''}`
        } else {
          o.textContent = `${st.label} — частично (${st.files.filter((f) => f.exists).length}/${st.files.length})`
        }
        sel.appendChild(o)
      }
    }
    sel.value = this.selected

    await this.renderPanel()
  }

  /** Статус бандла по имени: из кеша `statuses` (SSOT), при отсутствии — один
   * fetch и запись в кеш. Ошибка → undefined (звонящий обязан сказать об этом
   * юзеру, молчаливых отказов нет). */
  private async statusFor(name: string): Promise<TabInfo | undefined> {
    const svc = this.service
    if (!svc) return undefined
    const cached = this.statuses.get(name)
    if (cached) return cached
    try {
      const info = await svc.info(name || null)
      this.statuses.set(name, info)
      return info
    } catch {
      return undefined
    }
  }

  /** Перечень недостающих файлов статуса (строка состояния + подсказка кнопки). */
  private static missingList(st: TabInfo): string[] {
    return st.files.filter((f) => !f.exists).map((f) => f.filename)
  }

  /** Рендер ВСЕЙ панели под выбранный бандл (`this.selected`): строка
   * состояния, файлы, диск/память, кнопки операций. Контракт PRP 29.10.26 D1:
   * операции вкладки (скачать/докачать/удалить/активировать) работают с
   * ВЫБРАННЫМ бандлом; молчаливого скрытия кнопки «Активировать» нет —
   * недокачанный всегда назван причиной на экране. */
  private async renderPanel() {
    const svc = this.service
    if (!svc) return
    const st = await this.statusFor(this.selected)
    const sel = this.root.getElementById('bundle') as HTMLSelectElement
    const stateEl = this.root.getElementById('state')!
    if (!st) {
      // Честное состояние: инфо недоступно — называем это словами.
      stateEl.textContent = 'статус недоступен (бэкенд вернул ошибку)'
      this.root.getElementById('label')!.textContent = `Ошибка: набор «${this.selected}» недоступен`
      const activateBtn = this.root.getElementById('activate') as HTMLButtonElement
      activateBtn.style.display = this.selected === this.appliedBundle ? 'none' : 'inline-block'
      activateBtn.disabled = true
      activateBtn.title = 'Статус бандла неизвестен — активация заблокирована вкладкой.'
      return
    }
    sel.value = this.selected

    // Строка состояния выбранного (вместо молчаливого гейта активации).
    const missing = BundleTabElement.missingList(st)
    if (this.selected === this.appliedBundle) {
      stateEl.textContent = 'активный'
    } else if (st.fully_downloaded) {
      stateEl.textContent = 'скачан полностью — можно активировать'
    } else if (missing.length === st.files.length) {
      stateEl.textContent = 'не скачан'
    } else {
      stateEl.textContent =
        `частично: файлов ${st.files.length - missing.length} из ${st.files.length}, нет: ${missing.join(', ')}`
    }

    // Кнопка «Активировать»: скрыта для уже активного; для недокачанного —
    // disabled с причиной в подсказке (бэкенд-гейт остаётся финальной
    // проверкой, вкладка лишь заранее показывает причину).
    const activateBtn = this.root.getElementById('activate') as HTMLButtonElement
    if (this.selected === this.appliedBundle) {
      activateBtn.style.display = 'none'
    } else {
      activateBtn.style.display = 'inline-block'
      activateBtn.disabled = !st.fully_downloaded
      activateBtn.title = st.fully_downloaded
        ? ''
        : `Недокачанные файлы (${missing.length}):\n${missing.join('\n')}`
    }

    this.root.getElementById('label')!.textContent =
      `${st.label}${st.fully_downloaded ? '' : ' (не все файлы скачаны)'}`
    this.root.getElementById('note')!.textContent = st.note || ''
    const ul = this.root.getElementById('files')!
    ul.innerHTML = ''
    let anyExists = false
    for (const f of st.files) {
      if (f.exists) anyExists = true
      const li = document.createElement('li')
      li.className = f.exists ? 'ok' : 'missing'
      const size = f.size_bytes ? ` — ${formatBytes(f.size_bytes)}` : ''
      li.textContent = `${f.exists ? '✓' : '○'} ${esc(f.filename)} (${f.role})${size}`
      ul.appendChild(li)
    }
    const totalGb = (st.total_bytes / 1024 / 1024 / 1024).toFixed(1)
    this.root.getElementById('disk')!.textContent =
      `~${totalGb} ГБ на диске · свободно ${st.free_space_gb} ГБ`
    this.root.getElementById('mem')!.textContent =
      `GPU: ${st.vram_fast_gb ?? '?'} ГБ (быстро) / ${st.vram_min_gb ?? '?'} ГБ (мин.) · RAM ≥${st.ram_min_gb ?? '?'} ГБ`
    this.root.getElementById('dir')!.textContent = st.save_dir

    const dlBtn = this.root.getElementById('download') as HTMLButtonElement
    dlBtn.style.display = st.fully_downloaded ? 'none' : 'inline-block'
    dlBtn.textContent = anyExists ? 'Докачать файлы' : 'Скачать набор'
    const resumeBtn = this.root.getElementById('resume') as HTMLButtonElement
    resumeBtn.style.display = st.fully_downloaded ? 'none' : 'inline-block'
    resumeBtn.disabled = !anyExists // «Докачать» без ни одного файла = то же, что «Скачать»
    // «Сбросить путь» виден только когда путь кастомный (вернёт к дефолту
    // хранилища KingOrchData); дефолт сбрасывать нечего.
    this.root.getElementById('resetDir')!.style.display = st.save_dir ? 'inline-block' : 'none'
    this.root.getElementById('deleteFiles')!.style.display = st.fully_downloaded ? 'inline-block' : 'none'

    if (svc.memoryProbe) {
      try {
        this.root.getElementById('mem')!.textContent = await svc.memoryProbe(st.bundle_name)
      } catch { /* NVML может отсутствовать — остаётся строка пресета */ }
    }

    // Неиспользуемые файлы пула (только image-сервис): остатки бандлов,
    // исчезнувших из каталога, + сторонний мусор в корне пула. Ничего не
    // удаляется само — видимый список и явная кнопка (решение юзера).
    const orphBox = this.root.getElementById('orphans')!
    if (svc.poolOrphans) {
      try {
        const info = await svc.poolOrphans()
        this.orphanNames = info.orphans.map((o) => o.filename)
        if (info.orphans.length === 0) {
          orphBox.style.display = 'none'
        } else {
          orphBox.style.display = 'block'
          const ul = this.root.getElementById('orphanFiles')!
          ul.innerHTML = ''
          for (const o of info.orphans) {
            const li = document.createElement('li')
            li.className = 'missing'
            li.textContent = `${esc(o.filename)} — ${formatBytes(o.size_bytes)}`
            ul.appendChild(li)
          }
          this.root.getElementById('orphansSize')!.textContent =
            `${info.orphans.length} шт., ${formatBytes(info.total_bytes)} — не нужны ни одному бандлу каталога`
        }
      } catch {
        // Вспомогательная секция: сбой её загрузки не блокирует вкладку.
        orphBox.style.display = 'none'
      }
    } else {
      orphBox.style.display = 'none'
    }
  }

  /** Смена выбора в селекте: панель целиком перерисовывается под выбранный
   * бандл (строка состояния, файлы, кнопки) — больше не только «показать/
   * скрыть кнопку Activate». */
  private async onBundleChange() {
    const sel = this.root.getElementById('bundle') as HTMLSelectElement
    this.selected = sel.value
    await this.renderPanel()
  }

  private async onActivate() {
    const svc = this.service
    if (!svc || this.busy) return
    const sel = this.root.getElementById('bundle') as HTMLSelectElement
    if (!sel.value) return
    this.busy = true
    try {
      await svc.activate(sel.value)
      notifyBundleChanged()
      toast(`Активный ${svc.kindLabel.toLowerCase()}: ${sel.value}`)
    } catch (e) {
      toast(`Ошибка активации: ${e}`, 'error')
    } finally {
      this.busy = false
      await this.refresh()
    }
  }

  /** «Скачать набор»/«Докачать файлы»: качает ВЫБРАННЫЙ в селекте бандл
   * (PRP 29.10.26 D1 — раньше смотрел на активный и кнопка «докачать выбранный»
   * превращалась в тупик), всегда в настроенную папку (`save_dir` — кастом или
   * дефолт KingOrchData, пустым не бывает — бэкенд `commands.rs` гарантирует
   * дефолт). Выбор пути — ровно одно место на вкладке: кнопка «Изменить путь…»
   * (SSOT решений). Выбранная папка = ПУЛ для всех наборов (25.10.2026):
   * подпапка `${base}/${bundle_name}` не создаётся — она порождала перекачку
   * общих файлов. */
  private async onDownload() {
    const svc = this.service
    if (!svc || this.busy) return
    const st = await this.statusFor(this.selected)
    if (!st) {
      toast('Статус выбранного набора недоступен — скачивание отменено.', 'error')
      return
    }
    this.busy = true
    const btn = this.root.getElementById('download') as HTMLButtonElement
    btn.disabled = true
    this.showProgress(true)
    this.setProgress(0, 0)
    try {
      await svc.download(st.save_dir, st.bundle_name)
      notifyBundleChanged()
      toast(`Набор ${svc.kind === 'video' ? 'видео' : 'изображений'} скачан!`)
    } catch (e) {
      toast(`Ошибка скачивания набора: ${e}`, 'error')
    } finally {
      this.busy = false
      btn.disabled = false
      this.showProgress(false)
      await this.refresh()
    }
  }

  /** «Изменить путь…»: диалог → проверка наличия файлов ВЫБРАННОГО бандла →
   * set → честный тост о нехватке. Папка задаётся ОДНА для всех бандлов
   * (общий пул, 25.10.2026); валидация — по выбранному имени (D1). */
  private async onChangeDir() {
    const svc = this.service
    if (!svc || this.busy) return
    this.busy = true
    const btn = this.root.getElementById('changeDir') as HTMLButtonElement
    btn.disabled = true
    try {
      const sel = await openDialog({ directory: true, title: `Папка набора ${svc.kind === 'video' ? 'VideoGEN' : 'ImageGEN'}` })
      if (!sel) return
      const path = Array.isArray(sel) ? sel[0] : sel
      if (!path) return
      let valid: Awaited<ReturnType<BundleService['validateDir']>>
      try {
        valid = await svc.validateDir(path, this.selected || null)
      } catch (e) {
        toast(`Ошибка проверки набора: ${e}`, 'error')
        return
      }
      await svc.setCustomDir(path)
      notifyBundleChanged()
      if (valid.valid) {
        toast('Папка набора обновлена: все файлы на месте.')
      } else {
        toast(`Папка выбрана. Не хватает файлов: ${valid.missing.length}. Нажмите «Докачать файлы».`)
      }
      await this.refresh()
    } catch (e) {
      toast(`Не удалось сменить папку набора: ${e}`, 'error')
    } finally {
      btn.disabled = false
      this.busy = false
    }
  }

  /** «Сбросить путь»: вернуть папку набора к дефолту хранилища. */
  private async onResetDir() {
    const svc = this.service
    if (!svc || this.busy) return
    this.busy = true
    try {
      await svc.clearCustomDir()
      notifyBundleChanged()
      toast('Папка сброшена: вернулись к хранилищу по умолчанию.')
    } catch (e) {
      toast(`Ошибка: ${e}`, 'error')
    } finally {
      this.busy = false
      await this.refresh()
    }
  }

  /** Custom-папка (resume): докачать недостающие ВЫБРАННОГО бандла (PRP
   * 29.10.26 D1 — раньше качал активный, из-за чего выбранный недокачанный
   * бандл было невозможно догнать). */
  private async onResume() {
    const svc = this.service
    if (!svc || this.busy) return
    const st = await this.statusFor(this.selected)
    if (!st) {
      toast('Статус выбранного набора недоступен — докачка отменена.', 'error')
      return
    }
    this.busy = true
    const btn = this.root.getElementById('resume') as HTMLButtonElement
    btn.disabled = true
    this.showProgress(true)
    this.setProgress(0, 0)
    try {
      await svc.download(st.save_dir, st.bundle_name)
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

  private confirmDelete() {
    this.modalAction = 'bundle'
    this.root.getElementById('overlayTitle')!.textContent = 'Удалить файлы набора?'
    this.root.getElementById('overlayMsg')!.textContent =
      'Файлы с диска будут удалены БЕЗВОЗВРАТНО. Общие с другими бандлами файлы останутся в пуле.'
    this.root.getElementById('overlay')!.classList.add('open')
  }

  /** Подтверждение очистки неиспользуемых файлов пула (image-сервис). */
  private confirmRemoveOrphans() {
    if (this.orphanNames.length === 0) return
    this.modalAction = 'orphans'
    this.root.getElementById('overlayTitle')!.textContent = 'Удалить неиспользуемые файлы пула?'
    this.root.getElementById('overlayMsg')!.textContent =
      `Файлов: ${this.orphanNames.length}. Они не нужны ни одному бандлу каталога и будут удалены БЕЗВОЗВРАТНО.`
    this.root.getElementById('overlay')!.classList.add('open')
  }

  private closeModal() {
    this.root.getElementById('overlay')!.classList.remove('open')
  }

  private async doDeleteFiles() {
    const svc = this.service
    if (!svc || this.busy) return
    this.busy = true
    this.closeModal()
    try {
      if (this.modalAction === 'orphans') {
        if (!svc.removePoolOrphans || this.orphanNames.length === 0) return
        const names = this.orphanNames
        await svc.removePoolOrphans(names)
        notifyBundleChanged()
        toast(`Удалено неиспользуемых файлов: ${names.length}.`)
      } else {
        // Удаляем файлы ВЫБРАННОГО бандла (D1): null удалял бы активный.
        await svc.removeFiles(this.selected || null)
        notifyBundleChanged()
        toast('Файлы набора удалены.')
      }
    } catch (e) {
      toast(`Ошибка удаления: ${e}`, 'error')
    } finally {
      this.busy = false
      await this.refresh()
    }
  }
}

if (!customElements.get('image-bundle-tab')) {
  customElements.define('image-bundle-tab', BundleTabElement)
}
