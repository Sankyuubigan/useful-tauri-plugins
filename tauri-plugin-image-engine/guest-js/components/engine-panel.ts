/**
 * <image-engine-panel> — статус/установка движка sd.cpp (перенос из
 * web-components.ts без изменений поведения: install/update/remove, бекенды).
 */
import { STYLE, toast, progressText } from './ui-shared'
import {
  getImageEngineStatus,
  installImageEngine,
  installImageEngineUpdate,
  removeImageEngine,
  setImageEngineDir,
  setImageEngineVariant,
  type ImageEngineStatus,
} from '../index'
import { checkUpdate, getUpdateState, onUpdateState, setUpdateState } from '../updates'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { listen } from '@tauri-apps/api/event'

export class ImageEnginePanel extends HTMLElement {
  private root!: ShadowRoot
  private busy = false
  private applied = 'auto'
  private unlisten?: () => void
  private unsubUpdate?: () => void

  connectedCallback() {
    if (this.root) return
    this.root = this.attachShadow({ mode: 'open' })
    this.root.innerHTML = `
      <style>${STYLE}</style>
      <div>
        <div class="row"><label>Статус:</label><span id="status" class="hint">Проверка…</span><span id="updateBadge" class="update-badge" style="display:none;"></span></div>
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

    this.unsubUpdate = onUpdateState((s) => this.renderUpdateState(s))
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
    this.unsubUpdate?.()
    this.unlisten?.()
  }

  private setStatus(s: string) {
    const el = this.root.getElementById('status')
    if (el) el.textContent = s
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
    this.renderUpdateState(getUpdateState())
  }

  /**
   * Единственное место, где видны бейдж и кнопка «Обновить»: оба выводятся
   * из одного состояния. Раньше в ветке `else` прятался только бейдж — кнопка
   * оставалась на экране после установки обновления.
   */
  private renderUpdateState(s: { hasUpdate: boolean; tag?: string }) {
    const badge = this.root.getElementById('updateBadge') as HTMLElement | null
    const updateBtn = this.root.getElementById('installUpdate') as HTMLElement | null
    const display = s.hasUpdate ? 'inline-block' : 'none'
    if (badge) badge.style.display = display
    if (updateBtn) updateBtn.style.display = display
    if (s.hasUpdate) {
      this.setStatus(`Доступно обновление движка: ${s.tag ?? ''}`)
    }
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
    // На время проверки прячем кнопку «Обновить»; результат придёт через setUpdateState,
    // который двигает и кнопку, и бейдж, и точку в навигации хоста.
    this.renderUpdateState({ hasUpdate: false })
    try {
      const next = await checkUpdate()
      this.setStatus(
        next.hasUpdate
          ? `Доступно обновление движка: ${next.tag ?? ''}`
          : 'Движок изображений актуален',
      )
    } catch (e) {
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
      setUpdateState({ hasUpdate: false })
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

if (!customElements.get('image-engine-panel')) {
  customElements.define('image-engine-panel', ImageEnginePanel)
}
