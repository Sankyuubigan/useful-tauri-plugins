import { ttsEngineBackends, ttsGetSettings, ttsDefaultDirs, ttsListModels, ttsListVoices, ttsCheckUpdate, ttsDownloadEngine, ttsDownloadModel, ttsAddVoice, ttsDeleteVoice, ttsVoiceAudio, ttsVoiceAvatar, ttsSaveSettings, ttsUnload, } from './index';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
/** Общие стили плашек — темизируются через CSS-переменные хоста. */
const PANEL_STYLES = `
  :host { display: block; color: var(--text, #333); font-family: var(--font, system-ui, sans-serif); }
  * { box-sizing: border-box; }
  button { margin: 4px 4px 0 0; padding: 5px 10px; cursor: pointer; border-radius: 6px;
           border: 1px solid var(--border, #ccc); background: var(--session-hover, #eee);
           color: var(--text, #333); font: inherit; }
  button.primary { background: var(--primary, #4a90d9); color: #fff; border-color: var(--primary, #4a90d9); }
  button:disabled { opacity: .5; cursor: default; }
  select, input, textarea { font: inherit; color: var(--text, #333); background: var(--bg-color, #fff);
           border: 1px solid var(--border, #ccc); border-radius: 6px; padding: 4px 6px; }
  .muted { color: var(--text-muted, #888); font-size: 12px; }
  .row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin-top: 6px; }
  .field { margin-top: 8px; }
  .field label { display: block; font-size: 13px; margin-bottom: 2px; }
  .field input[type="text"], .field select { width: 100%; }
  .progress { height: 6px; background: var(--session-hover, #eee); border-radius: 3px; margin-top: 6px; overflow: hidden; }
  .progress > div { height: 100%; background: var(--primary, #4a90d9); width: 0; transition: width .2s; }
  .voice { display: flex; align-items: center; gap: 8px; padding: 6px 0; border-bottom: 1px solid var(--border, #eee); }
  .voice img { width: 40px; height: 40px; border-radius: 50%; object-fit: cover; background: var(--session-hover, #eee); }
  .voice .info { flex: 1; min-width: 0; }
  .voice .name { font-weight: 600; }
  ul { list-style: none; padding: 0; margin: 8px 0 0; }
  li { display: flex; align-items: center; gap: 8px; padding: 5px 0; border-bottom: 1px solid var(--border, #eee); }
  li .label { flex: 1; }
  .badge { font-size: 11px; padding: 1px 6px; border-radius: 10px; background: var(--session-hover, #eee);
           color: var(--text-muted, #888); }
  .badge.ok { background: #e6f4ea; color: #1d7f3c; }
  .badge.warn { background: #fdf3d7; color: #9a6b00; }
`;
/**
 * <speech-engine-panel></speech-engine-panel>
 *
 * Плашка «Движок TTS»: установка/обновление prebuilt CrispASR, выбор бэкенда,
 * папки движка/моделей, выгрузка (освобождение VRAM).
 */
class SpeechEnginePanel extends HTMLElement {
    constructor() {
        super(...arguments);
        this.busy = false;
        this.settings = {};
        this.defaults = { engine_dir: '', models_dir: '' };
        this.backend = '';
        this.backends = [];
        this.status = '';
        this.updateInfo = '';
        this.download = null;
        this.unlisteners = [];
    }
    connectedCallback() {
        this.root = this.attachShadow({ mode: 'open' });
        this.render();
        this.init();
    }
    render() {
        this.root.innerHTML = `
      <style>${PANEL_STYLES}</style>
      <div>
        <div class="row">
          <button id="refresh">Проверить обновления</button>
          <button id="unload">Выгрузить (VRAM)</button>
        </div>
        <div class="field">
          <label for="engine_dir_input">Папка движка CrispASR:</label>
          <div class="row">
            <input id="engine_dir_input" type="text" placeholder="…/crispasr" />
            <button id="engine_dir_browse">Обзор…</button>
          </div>
        </div>
        <div class="field">
          <label for="models_dir_input">Папка моделей TTS:</label>
          <div class="row">
            <input id="models_dir_input" type="text" placeholder="…/tts_models" />
            <button id="models_dir_browse">Обзор…</button>
          </div>
        </div>
        <div class="field">
          <label for="backend">Бэкенд движка:</label>
          <div class="row">
            <select id="backend"></select>
          </div>
        </div>
        <div class="field">
          <button id="install" class="primary">Скачать движок</button>
        </div>
        <div id="progress" class="progress" style="display:none"><div></div></div>
        <div id="status" class="muted" style="margin-top:8px"></div>
      </div>`;
        this.root.getElementById('engine_dir_input').addEventListener('input', (e) => {
            this.settings.engine_dir = e.target.value;
        });
        this.root.getElementById('models_dir_input').addEventListener('input', (e) => {
            this.settings.models_dir = e.target.value;
        });
        this.root.getElementById('engine_dir_browse').addEventListener('click', () => this.pickDir('engine_dir'));
        this.root.getElementById('models_dir_browse').addEventListener('click', () => this.pickDir('models_dir'));
        this.root.getElementById('backend').addEventListener('change', (e) => {
            this.backend = e.target.value;
            this.saveSettings();
        });
        this.root.getElementById('install').addEventListener('click', () => this.install());
        this.root.getElementById('refresh').addEventListener('click', () => this.checkUpdate());
        this.root.getElementById('unload').addEventListener('click', async () => {
            try {
                await ttsUnload();
                this.setStatus('движок выгружен');
            }
            catch (e) {
                this.setStatus('ошибка: ' + e.message);
            }
        });
    }
    async init() {
        const [s, d, b] = await Promise.all([
            ttsGetSettings().catch(() => ({})),
            ttsDefaultDirs().catch(() => ({ engine_dir: '', models_dir: '' })),
            ttsEngineBackends().catch(() => []),
        ]);
        this.settings = s;
        this.defaults = d;
        this.backends = b;
        if (!this.settings.engine_dir)
            this.settings.engine_dir = d.engine_dir;
        if (!this.settings.models_dir)
            this.settings.models_dir = d.models_dir;
        if (!this.settings.engine_backend && b.length)
            this.settings.engine_backend = b[0].id;
        this.root.getElementById('engine_dir_input').value = this.settings.engine_dir;
        this.root.getElementById('models_dir_input').value = this.settings.models_dir;
        const sel = this.root.getElementById('backend');
        sel.innerHTML = b.map(x => `<option value="${x.id}">${x.label}</option>`).join('');
        if (b.length === 0) {
            const none = document.createElement('option');
            none.value = '';
            none.textContent = 'сеть недоступна…';
            sel.appendChild(none);
        }
        sel.value = this.settings.engine_backend || b[0]?.id || '';
        this.backend = sel.value;
        this.unlisteners.push(await listen('tts-download', (ev) => {
            const p = ev.payload;
            this.download = { current: p.downloaded, total: p.total };
            this.updateProgress();
        }));
        this.checkUpdate();
    }
    async pickDir(field) {
        const picked = await open({ directory: true }).catch(() => null);
        if (picked && typeof picked === 'string') {
            if (field === 'engine_dir')
                this.settings.engine_dir = picked;
            else
                this.settings.models_dir = picked;
            this.root.getElementById(field + '_input').value = picked;
            this.saveSettings();
        }
    }
    async saveSettings() {
        try {
            await ttsSaveSettings(this.settings);
        }
        catch { /* ignore */ }
    }
    setStatus(s) {
        this.status = s;
        const el = this.root.getElementById('status');
        if (el)
            el.textContent = [s, this.updateInfo].filter(Boolean).join(' • ');
    }
    updateProgress() {
        const box = this.root.getElementById('progress');
        const bar = box.firstElementChild;
        if (this.download && this.download.total > 0) {
            box.style.display = 'block';
            bar.style.width = `${Math.min(100, (this.download.current / this.download.total) * 100)}%`;
            this.setStatus(`скачивание… ${Math.round(this.download.total / 1048576)} МБ`);
        }
        else if (this.download) {
            box.style.display = 'block';
            bar.style.width = '100%';
        }
        else {
            box.style.display = 'none';
        }
    }
    async checkUpdate() {
        const res = await ttsCheckUpdate().catch(() => null);
        if (!res)
            return;
        if (!res.ok) {
            this.updateInfo = 'обновления недоступны: ' + (res.error ?? '?');
        }
        else {
            const need = (res.engines ?? []).filter(e => e.update_available);
            this.updateInfo = need.length
                ? `последняя версия ${res.latest}: обновления для ${need.map(e => e.label).join(', ')}`
                : `установлена последняя версия (${res.latest})`;
        }
        this.setStatus(this.status);
    }
    async install() {
        if (this.busy)
            return;
        if (!this.backend) {
            this.setStatus('выберите бэкенд');
            return;
        }
        this.busy = true;
        this.setStatus('скачиваю движок…');
        this.download = { current: 0, total: 0 };
        this.updateProgress();
        try {
            await ttsDownloadEngine(this.backend, this.settings.engine_dir || this.defaults.engine_dir);
            this.setStatus('движок скачан');
            await this.checkUpdate();
        }
        catch (e) {
            this.setStatus('ошибка: ' + e.message);
        }
        finally {
            this.download = null;
            this.updateProgress();
            this.busy = false;
        }
    }
    disconnectedCallback() {
        for (const u of this.unlisteners)
            u();
        this.unlisteners = [];
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
    constructor() {
        super(...arguments);
        this.models = [];
        this.download = null;
        this.busyId = '';
        this.modelsDir = '';
        this.unlisteners = [];
    }
    connectedCallback() {
        this.root = this.attachShadow({ mode: 'open' });
        this.render();
        this.init();
    }
    static get observedAttributes() { return ['models-dir']; }
    attributeChangedCallback() {
        if (this.root)
            this.init();
    }
    render() {
        this.root.innerHTML = `
      <style>${PANEL_STYLES}</style>
      <div>
        <div class="row"><strong>Установленные модели (GGUF)</strong></div>
        <div id="progress" class="progress" style="display:none"><div></div></div>
        <div id="models"></div>
        <div id="status" class="muted" style="margin-top:8px"></div>
      </div>`;
    }
    async init() {
        const attr = this.getAttribute('models-dir');
        if (!this.modelsDir)
            this.modelsDir = attr || '';
        if (!this.modelsDir) {
            const s = await ttsGetSettings().catch(() => ({}));
            this.modelsDir = s.models_dir || '';
            if (!this.modelsDir) {
                const d = await ttsDefaultDirs().catch(() => ({ models_dir: '' }));
                this.modelsDir = d.models_dir;
            }
        }
        this.models = await ttsListModels(this.modelsDir).catch(() => []);
        this.renderList();
        this.unlisteners.push(await listen('tts-download', (ev) => {
            const p = ev.payload;
            if (p.kind !== 'model')
                return;
            this.download = { current: p.downloaded, total: p.total };
            this.updateProgress();
        }));
    }
    renderList() {
        const box = this.root.getElementById('models');
        if (this.models.length === 0) {
            box.textContent = 'нет списка моделей (сеть?)';
            return;
        }
        box.innerHTML = '<ul>' + this.models.map(m => `
      <li>
        <div class="label">${m.label}
          <span class="badge ${m.installed ? 'ok' : 'warn'}">${m.installed ? 'установлена' : `${m.size}`}</span>
        </div>
        ${m.installed ? '' : `<button data-id="${m.id}" class="primary install">Скачать</button>`}
      </li>`).join('') + '</ul>';
        for (const btn of box.querySelectorAll('button.install')) {
            btn.addEventListener('click', () => this.install(btn.dataset['id'] || ''));
        }
    }
    setStatus(s) {
        const el = this.root.getElementById('status');
        if (el)
            el.textContent = s;
    }
    updateProgress() {
        const box = this.root.getElementById('progress');
        const bar = box.firstElementChild;
        if (this.download && this.download.total > 0) {
            box.style.display = 'block';
            bar.style.width = `${Math.min(100, (this.download.current / this.download.total) * 100)}%`;
        }
        else {
            box.style.display = 'none';
        }
    }
    async install(id) {
        if (this.busyId)
            return;
        this.busyId = id;
        this.setStatus(`скачиваю ${id}…`);
        this.download = { current: 0, total: 0 };
        this.updateProgress();
        try {
            await ttsDownloadModel(id, this.modelsDir);
            this.models = await ttsListModels(this.modelsDir).catch(() => this.models);
            this.renderList();
            this.setStatus('модель скачана');
        }
        catch (e) {
            this.setStatus('ошибка: ' + e.message);
        }
        finally {
            this.download = null;
            this.updateProgress();
            this.busyId = '';
        }
    }
    disconnectedCallback() {
        for (const u of this.unlisteners)
            u();
        this.unlisteners = [];
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
    constructor() {
        super(...arguments);
        this.voices = [];
        this.modelsDir = '';
        this.unlisteners = [];
    }
    connectedCallback() {
        this.root = this.attachShadow({ mode: 'open' });
        this.render();
        this.init();
    }
    static get observedAttributes() { return ['models-dir']; }
    attributeChangedCallback() {
        if (this.root)
            this.init();
    }
    render() {
        this.root.innerHTML = `
      <style>${PANEL_STYLES}</style>
      <div>
        <div class="row"><strong>Хранилище голосов</strong></div>
        <div class="row">
          <input id="name" type="text" placeholder="Имя голоса" style="flex:1" />
          <button id="pick_audio" class="primary">+ Добавить (WAV)</button>
        </div>
        <div id="voices"></div>
        <div id="status" class="muted" style="margin-top:8px"></div>
      </div>`;
        this.root.getElementById('name').addEventListener('keydown', (e) => {
            if (e.key === 'Enter')
                this.addVoice();
        });
        this.root.getElementById('pick_audio').addEventListener('click', () => this.addVoice());
    }
    async init() {
        const attr = this.getAttribute('models-dir');
        if (!this.modelsDir)
            this.modelsDir = attr || '';
        if (!this.modelsDir) {
            const s = await ttsGetSettings().catch(() => ({}));
            this.modelsDir = s.models_dir || '';
            if (!this.modelsDir) {
                const d = await ttsDefaultDirs().catch(() => ({ models_dir: '' }));
                this.modelsDir = d.models_dir;
            }
        }
        await this.refresh();
    }
    async refresh() {
        this.voices = await ttsListVoices(this.modelsDir).catch(() => []);
        this.renderList();
    }
    setStatus(s) {
        const el = this.root.getElementById('status');
        if (el)
            el.textContent = s;
    }
    async renderList() {
        const box = this.root.getElementById('voices');
        if (this.voices.length === 0) {
            box.textContent = 'нет сохранённых голосов';
            return;
        }
        box.innerHTML = '';
        for (const v of this.voices) {
            const row = document.createElement('div');
            row.className = 'voice';
            const img = document.createElement('img');
            if (v.has_avatar) {
                const b = await ttsVoiceAvatar(this.modelsDir, v.id).catch(() => null);
                if (b)
                    img.src = 'data:image/jpeg;base64,' + btoa(String.fromCharCode(...b));
            }
            row.appendChild(img);
            const info = document.createElement('div');
            info.className = 'info';
            const name = document.createElement('div');
            name.className = 'name';
            name.textContent = v.name;
            const ref = document.createElement('div');
            ref.className = 'muted';
            ref.textContent = v.ref_text ? `«${v.ref_text}»` : v.id;
            info.append(name, ref);
            row.appendChild(info);
            const play = document.createElement('button');
            play.textContent = '▶';
            play.title = 'Прослушать';
            play.addEventListener('click', () => this.playVoice(v.id));
            row.appendChild(play);
            const del = document.createElement('button');
            del.textContent = '✕';
            del.title = 'Удалить';
            del.addEventListener('click', () => this.deleteVoice(v.id));
            row.appendChild(del);
            box.appendChild(row);
        }
    }
    async playVoice(id) {
        try {
            const data = await ttsVoiceAudio(this.modelsDir, id);
            const url = URL.createObjectURL(new Blob([new Uint8Array(data)], { type: 'audio/wav' }));
            const audio = new Audio(url);
            audio.onended = () => URL.revokeObjectURL(url);
            await audio.play();
        }
        catch (e) {
            this.setStatus('не удалось проиграть: ' + e.message);
        }
    }
    async deleteVoice(id) {
        try {
            await ttsDeleteVoice(this.modelsDir, id);
            await this.refresh();
        }
        catch (e) {
            this.setStatus('ошибка: ' + e.message);
        }
    }
    async addVoice() {
        const nameInput = this.root.getElementById('name');
        const name = nameInput.value.trim();
        if (!name) {
            this.setStatus('укажите имя голоса');
            return;
        }
        const picked = await open({
            multiple: false,
            filters: [{ name: 'Audio', extensions: ['wav', 'ogg', 'mp3', 'flac'] }],
        }).catch(() => null);
        if (!picked)
            return;
        const src = Array.isArray(picked) ? picked[0] : picked;
        if (!src)
            return;
        this.setStatus('обрабатываю голос…');
        try {
            await ttsAddVoice({
                modelsDir: this.modelsDir,
                name,
                srcAudio: src,
                refText: '',
                avatar: '',
                denoise: false,
                denoiseStrength: 0.9,
            });
            nameInput.value = '';
            await this.refresh();
            this.setStatus('голос добавлен');
        }
        catch (e) {
            this.setStatus('ошибка: ' + e.message);
        }
    }
    disconnectedCallback() {
        for (const u of this.unlisteners)
            u();
        this.unlisteners = [];
    }
}
if (!customElements.get('speech-engine-panel')) {
    customElements.define('speech-engine-panel', SpeechEnginePanel);
}
if (!customElements.get('speech-models-panel')) {
    customElements.define('speech-models-panel', SpeechModelsPanel);
}
if (!customElements.get('speech-voice-storage')) {
    customElements.define('speech-voice-storage', SpeechVoiceStorage);
}
