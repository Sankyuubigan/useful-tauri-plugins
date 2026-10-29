/**
 * <image-media-panel> — карточка моделей ImageGEN (PRP 21.10.26, Шаг 3;
 * вердикт юзера 22.10: движок — ОТДЕЛЬНАЯ карточка image-engine-panel,
 * как у llama-engine; эта карточка держит ТОЛЬКО вкладки бандлов):
 * вкладки «Картинки» / «Видео» — каждая image-bundle-tab со своим сервисом.
 * Кастом-папка теперь внутренность вкладки (наследие ImageModelsPanel).
 */
import { STYLE } from './ui-shared'
import './bundle-tab'
import './lora-tab'

export class MediaEnginePanel extends HTMLElement {
  private root!: ShadowRoot

  connectedCallback() {
    if (this.root) return
    this.root = this.attachShadow({ mode: 'open' })
    this.root.innerHTML = `
      <style>${STYLE}
        .tabs-row { margin: 0 0 10px; }
        .tab-panel { margin-top: 10px; }
      </style>
      <div>
        <div class="row tabs-row">
          <button id="tabImages" class="tab-btn primary" aria-pressed="true">Картинки</button>
          <button id="tabVideo" class="tab-btn" aria-pressed="false">Видео</button>
          <button id="tabLoras" class="tab-btn" aria-pressed="false">LoRA</button>
        </div>
        <div class="tab-panel" id="panelImages"><image-bundle-tab service="image"></image-bundle-tab></div>
        <div class="tab-panel" id="panelVideo" style="display:none;"><image-bundle-tab service="video"></image-bundle-tab></div>
        <div class="tab-panel" id="panelLoras" style="display:none;"><image-lora-tab></image-lora-tab></div>
      </div>`

    const btnImages = this.root.getElementById('tabImages') as HTMLButtonElement
    const btnVideo = this.root.getElementById('tabVideo') as HTMLButtonElement
    const btnLoras = this.root.getElementById('tabLoras') as HTMLButtonElement
    const panelImages = this.root.getElementById('panelImages') as HTMLElement
    const panelVideo = this.root.getElementById('panelVideo') as HTMLElement
    const panelLoras = this.root.getElementById('panelLoras') as HTMLElement

    const activate = (tab: 'images' | 'video' | 'loras') => {
      const show = (el: HTMLElement, on: boolean) => { el.style.display = on ? 'block' : 'none' }
      show(panelImages, tab === 'images')
      show(panelVideo, tab === 'video')
      show(panelLoras, tab === 'loras')
      const set = (btn: HTMLButtonElement, on: boolean) => {
        btn.setAttribute('aria-pressed', on ? 'true' : 'false')
        btn.classList.toggle('primary', on)
      }
      set(btnImages, tab === 'images')
      set(btnVideo, tab === 'video')
      set(btnLoras, tab === 'loras')
    }

    btnImages.addEventListener('click', () => activate('images'))
    btnVideo.addEventListener('click', () => activate('video'))
    btnLoras.addEventListener('click', () => activate('loras'))
  }
}

if (!customElements.get('image-media-panel')) {
  customElements.define('image-media-panel', MediaEnginePanel)
}
