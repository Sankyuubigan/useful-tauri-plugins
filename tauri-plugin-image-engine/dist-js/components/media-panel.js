/**
 * <image-media-panel> — карточка моделей ImageGEN (PRP 21.10.26, Шаг 3;
 * вердикт юзера 22.10: движок — ОТДЕЛЬНАЯ карточка image-engine-panel,
 * как у llama-engine; эта карточка держит ТОЛЬКО вкладки бандлов):
 * вкладки «Картинки» / «Видео» — каждая image-bundle-tab со своим сервисом.
 * Кастом-папка теперь внутренность вкладки (наследие ImageModelsPanel).
 */
import { STYLE } from './ui-shared';
import './bundle-tab';
export class MediaEnginePanel extends HTMLElement {
    connectedCallback() {
        if (this.root)
            return;
        this.root = this.attachShadow({ mode: 'open' });
        this.root.innerHTML = `
      <style>${STYLE}
        .tabs-row { margin: 0 0 10px; }
        .tab-panel { margin-top: 10px; }
      </style>
      <div>
        <div class="row tabs-row">
          <button id="tabImages" class="tab-btn primary" aria-pressed="true">Картинки</button>
          <button id="tabVideo" class="tab-btn" aria-pressed="false">Видео</button>
        </div>
        <div class="tab-panel" id="panelImages"><image-bundle-tab service="image"></image-bundle-tab></div>
        <div class="tab-panel" id="panelVideo" style="display:none;"><image-bundle-tab service="video"></image-bundle-tab></div>
      </div>`;
        const btnImages = this.root.getElementById('tabImages');
        const btnVideo = this.root.getElementById('tabVideo');
        const panelImages = this.root.getElementById('panelImages');
        const panelVideo = this.root.getElementById('panelVideo');
        const activate = (showVideo) => {
            btnImages.setAttribute('aria-pressed', showVideo ? 'false' : 'true');
            btnVideo.setAttribute('aria-pressed', showVideo ? 'true' : 'false');
            btnImages.classList.toggle('primary', !showVideo);
            btnVideo.classList.toggle('primary', showVideo);
            panelImages.style.display = showVideo ? 'none' : 'block';
            panelVideo.style.display = showVideo ? 'block' : 'none';
        };
        btnImages.addEventListener('click', () => activate(false));
        btnVideo.addEventListener('click', () => activate(true));
    }
}
if (!customElements.get('image-media-panel')) {
    customElements.define('image-media-panel', MediaEnginePanel);
}
