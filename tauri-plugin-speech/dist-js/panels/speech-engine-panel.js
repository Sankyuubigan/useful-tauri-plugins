import { ttsGetSettings, ttsSaveSettings, ttsDefaultDirs, ttsGetEngineStatus, ttsEngineBackends, ttsDownloadEngine, ttsDeleteEngine, ttsCheckUpdate, ttsUnload, } from '../index';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import { PANEL_STYLES } from './styles';
/**
 * <speech-engine-panel></speech-engine-panel>
 *
 * Плашка «Движок CrispASR»: установка/обновление prebuilt-бинаря, выбор
 * бэкенда, папки движка/моделей, выгрузка (освобождение VRAM).
 *
 * Кнопки выводятся из состояния ВЫБРАННОГО бэкенда (см. `renderEngineState`):
 * «Установить» — только когда его нет на диске, «Обновить» — только когда
 * доступна более свежая версия, «Удалить» — когда он установлен. Раньше кнопка
 * «скачать движок» была всегда видима с одной подписью, а статус считался по
 * всем бэкендам сразу — юзер не мог понять, что нажимать.
 */
class SpeechEnginePanel extends HTMLElement {
    constructor() {
        super(...arguments);
        this.initialized = false;
        this.busy = false;
        this.settings = {};
        this.defaults = { engine_dir: '', models_dir: '' };
        this.backends = [];
        /** Установленные бэкенды с диска: id → версия из `version.txt` (может быть null). */
        this.localBackends = new Map();
        this.state = {
            engineDir: '',
            modelsDir: '',
            backend: '',
            installed: false,
            installedVersion: null,
            updateAvailable: false,
            latestVersion: null,
        };
        this.download = null;
        this.unlisteners = [];
    }
    connectedCallback() {
        this.root = this.attachShadow({ mode: 'open' });
        this.render();
        void this.init();
    }
    render() {
        this.root.innerHTML = `
      <style>${PANEL_STYLES}</style>
      <div>
        <h3>Движок</h3>
        <label for="backend">Тип бэкенда:</label>
        <div class="row">
          <select id="backend"></select>
          <span id="backends_none" class="hint warn" hidden>не удалось получить список бинарей (нет сети?)</span>
        </div>
        <p class="hint">
          Статус: <span id="engine_status">—</span><span id="update_badge" class="badge ok" hidden>обновление доступно</span>
        </p>
        <div class="row">
          <button id="install" class="primary">Установить</button>
          <button id="checkUpdate">Проверить обновление</button>
          <button id="installUpdate" class="primary" hidden>Обновить</button>
          <button id="remove" class="danger" hidden>Удалить</button>
          <button id="engine_dir_browse">Изменить путь</button>
          <button id="unload">Выгрузить (VRAM)</button>
        </div>
        <p class="hint">Путь к движку: <code id="engine_dir_code"></code></p>
        <div id="progress" class="progress" style="display:none"><div></div></div>
        <div id="status" class="muted" style="margin-top:8px"></div>

        <div id="delete_dialog" class="dlg-overlay" hidden>
          <div class="dlg-box">
            <h3>Удалить движок CrispASR?</h3>
            <p>Будет удалена папка: <span class="strong" id="delete_path"></span></p>
            <p>Модели (GGUF) лежат в отдельной папке и останутся на месте.</p>
            <div class="dlg-buttons">
              <button id="delete_cancel">Отмена</button>
              <button id="delete_ok" class="danger">Удалить</button>
            </div>
          </div>
        </div>

        <hr />

        <h3>Папка моделей TTS</h3>
        <div class="row">
          <input id="models_dir_input" type="text" readonly value="" style="flex:1; min-width:200px;" />
          <button id="models_dir_browse">Выбрать папку</button>
        </div>
        <p class="hint">Внутри создаётся подпапка на каждый пресет; все GGUF качаются туда автоматически.</p>
      </div>`;
        const on = (id, fn) => this.root.getElementById(id).addEventListener('click', fn);
        on('engine_dir_browse', () => void this.pickDir('engine_dir'));
        on('models_dir_browse', () => void this.pickDir('models_dir'));
        on('install', () => void this.install());
        on('checkUpdate', () => void this.checkUpdate());
        // Установка и обновление — одна команда: она и качает, и перезаписывает version.txt.
        on('installUpdate', () => void this.install());
        on('remove', () => this.openDeleteDialog());
        on('delete_cancel', () => this.closeDeleteDialog());
        on('delete_ok', () => void this.deleteEngine());
        on('unload', () => void this.unload());
        this.root.getElementById('backend').addEventListener('change', (e) => {
            this.state.backend = e.target.value;
            void this.saveSettings();
            // Смена бэкенда = смена того, о чём говорит статус и какие кнопки нужны.
            this.applySelectedBackendFromLocal();
            this.renderEngineState();
        });
    }
    async init() {
        if (this.initialized)
            return;
        this.initialized = true;
        // Прогресс идёт из общего движка загрузок проекта (`downloader:progress`),
        // а не из плагина речи: плагин отдал свой загрузчик в пользу единого.
        const un = await listen('downloader:progress', (ev) => {
            const p = ev.payload;
            if (p.kind !== 'engine')
                return;
            this.download = { current: p.downloaded, total: p.total, speed: p.speed_bps };
            this.renderProgress();
        }).catch(() => null);
        if (un)
            this.unlisteners.push(un);
        const [s, d] = await Promise.all([
            ttsGetSettings().catch(() => ({})),
            ttsDefaultDirs().catch(() => ({ engine_dir: '', models_dir: '' })),
        ]);
        this.settings = s;
        this.defaults = d;
        this.state.engineDir = s.engine_dir || d.engine_dir;
        this.state.modelsDir = s.models_dir || d.models_dir;
        this.state.backend = s.engine_backend || '';
        const modelsInput = this.root.getElementById('models_dir_input');
        modelsInput.value = this.state.modelsDir;
        // Локальный статус — мгновенно и без сети: кнопки расставляются ДО похода в GitHub.
        await this.refreshLocalStatus();
        this.renderEngineState();
        // Список бэкендов и проверка обновлений — сеть; могут упасть, и это не сломает UI.
        void this.loadBackends();
    }
    /** Перечитывает локальный статус движка с диска (без сети). */
    async refreshLocalStatus() {
        let st;
        try {
            st = await ttsGetEngineStatus();
        }
        catch (e) {
            this.setStatus('ошибка чтения статуса движка: ' + e.message);
            return;
        }
        this.localBackends = new Map((st.installed_backends || []).map((b) => [b.id, b.installed_version]));
        this.state.engineDir = st.engine_dir || this.state.engineDir;
        this.state.modelsDir = st.models_dir || this.state.modelsDir;
        if (!this.state.backend)
            this.state.backend = st.selected_backend || '';
        // Статус выбранного бэкенда — ровно то, что команда посчитала по диску.
        this.state.installed = st.installed;
        this.state.installedVersion = st.installed ? st.installed_version : null;
    }
    /** Пересчитывает `installed`/`installedVersion` под текущий выбор бэкенда. */
    /**
     * Сбрасывает сетевое состояние апдейта.
     *
     * Результат проверки обновлений относится к КОНКРЕТНОМУ бэкенду в КОНКРЕТНОЙ
     * папке движка. Сменился бэкенд или папка — про новую цель мы ничего не
     * знаем, поэтому гасить надо (иначе бейдж «обновление доступно» остаётся
     * висеть от прежнего бэкенда, а кнопки «Обновить» нет — ровно тот баг,
     * что и чинили: апдейт одного бэкенда выдавался за все).
     */
    clearRemoteUpdateState() {
        this.state.updateAvailable = false;
        this.state.latestVersion = null;
    }
    /** Пересчитывает `installed`/`installedVersion` под текущий выбор бэкенда. */
    applySelectedBackendFromLocal() {
        this.clearRemoteUpdateState();
        if (!this.state.backend) {
            this.state.installed = false;
            this.state.installedVersion = null;
            return;
        }
        this.state.installed = this.localBackends.has(this.state.backend);
        this.state.installedVersion = this.localBackends.get(this.state.backend) ?? null;
    }
    /** Список бэкендов из GitHub. Недоступность сети — не повод ломать локальный UI. */
    async loadBackends() {
        try {
            this.backends = await ttsEngineBackends();
        }
        catch (e) {
            const span = this.root.getElementById('backends_none');
            if (span)
                span.hidden = false;
            this.setStatus('список бэкендов недоступен: ' + e.message);
            this.renderBackendSelect(); // покажем локально установленные, с ними уже можно работать
            return;
        }
        this.renderBackendSelect();
    }
    /** Селект: подписи с пометкой «— установлен» и «(рекомендуется)». */
    renderBackendSelect() {
        const sel = this.root.getElementById('backend');
        const prev = this.state.backend;
        sel.innerHTML = '';
        // Первый бэкенд в списке — приоритетный: так же ранжирует `classify_backend`
        // в Rust (cuda13 → cuda → cpu), отдельного поля «рекомендуется» в API нет.
        this.backends.forEach((b, i) => {
            const o = document.createElement('option');
            o.value = b.id;
            const marks = [
                this.localBackends.has(b.id) ? '— установлен' : '',
                i === 0 ? '(рекомендуется)' : '',
            ].filter(Boolean);
            o.textContent = marks.length ? `${b.label} ${marks.join(' ')}` : b.label;
            sel.appendChild(o);
        });
        // Бэкенды, установленные с диска, но исчезнувшие из свежего релиза, — не теряем.
        for (const id of this.localBackends.keys()) {
            if (this.backends.some((b) => b.id === id))
                continue;
            const o = document.createElement('option');
            o.value = id;
            o.textContent = `${id} — установлен (нет в свежем релизе)`;
            sel.appendChild(o);
        }
        if (sel.options.length === 0) {
            const none = document.createElement('option');
            none.value = '';
            none.textContent = '— список недоступен (нет сети) —';
            sel.appendChild(none);
        }
        if (prev && Array.from(sel.options).some((o) => o.value === prev))
            sel.value = prev;
        this.state.backend = sel.value;
        // Селект мог сменить выбранный бэкенд (например, прежнего нет в списке) —
        // состояние пересчитываем ДО отрисовки кнопок, иначе они будут от старого.
        this.applySelectedBackendFromLocal();
        this.renderEngineState();
    }
    /**
     * Единственное место, где статус, бейдж и кнопки выводятся из состояния.
     * Установка/обновление/удаление меняют состояние → перерисовка идёт отсюда.
     */
    renderEngineState() {
        const s = this.state;
        const btn = (id) => this.root.getElementById(id);
        const status = this.root.getElementById('engine_status');
        if (status)
            status.textContent = this.statusText();
        const badge = this.root.getElementById('update_badge');
        if (badge)
            badge.hidden = !s.updateAvailable;
        btn('install').hidden = s.installed;
        btn('checkUpdate').hidden = !s.installed;
        btn('installUpdate').hidden = !(s.installed && s.updateAvailable);
        btn('remove').hidden = !s.installed;
        const dir = this.root.getElementById('engine_dir_code');
        if (dir)
            dir.textContent = s.engineDir || '—';
    }
    /** Текст статуса. Никогда не врёт: неизвестное отдаётся как неизвестное. */
    statusText() {
        const s = this.state;
        if (!s.backend)
            return 'бэкенд не выбран';
        if (!s.installed)
            return `«${s.backend}» не установлен`;
        const parts = [`«${s.backend}» установлен`];
        if (s.installedVersion) {
            parts.push(`версия ${s.installedVersion}`);
        }
        else {
            // Не знаем версию — значит и не можем сказать, что она актуальная.
            parts.push('версия неизвестна');
        }
        if (s.updateAvailable && s.latestVersion)
            parts.push(`есть обновление ${s.latestVersion}`);
        return parts.join(' · ');
    }
    /** Проверка обновлений по сети. Сетевая ошибка НЕ трогает локальный статус. */
    async checkUpdate() {
        const btn = this.root.getElementById('checkUpdate');
        const label = btn.textContent;
        btn.disabled = true;
        btn.textContent = 'Проверка…';
        try {
            const res = await ttsCheckUpdate();
            if (!res || !res.ok) {
                const err = res?.error || 'нет ответа от сервера';
                // «Не смогли проверить» ≠ «обновлений нет»: состояние НЕ трогаем.
                this.setStatus('проверка обновлений недоступна: ' + err);
                return;
            }
            this.state.latestVersion = res.latest ?? null;
            // Апдейт считаем ТОЛЬКО для выбранного бэкенда. Раньше брался любой
            // бэкенд из списка, и апдейт одного показывался как «есть обновления» для всех.
            const own = (res.engines ?? []).find((e) => e.id === this.state.backend);
            if (this.state.installed) {
                const known = own?.installed_version ?? this.state.installedVersion;
                this.state.installedVersion = known;
                // Бейдж ставим ТОЛЬКО по данным бэкенда. Если его нет в релизе или
                // версия неизвестна — мы не знаем про апдейт ничего, и «обновление
                // доступно» было бы выдумкой (core §2.2): в этом случае кнопки
                // «Обновить» нет, статус честно пишет «версия неизвестна».
                this.state.updateAvailable = own?.update_available === true;
            }
            else {
                this.state.updateAvailable = false;
            }
            this.renderEngineState();
            this.setStatus(!this.state.installed
                ? `«${this.state.backend}» не установлен — можно установить`
                : this.state.updateAvailable
                    ? 'найдено обновление движка'
                    : this.state.installedVersion
                        ? 'установленная версия актуальна'
                        : 'версия движка неизвестна — обновите движок вручную');
        }
        catch (e) {
            this.setStatus('проверка обновлений недоступна: ' + e.message);
        }
        finally {
            btn.disabled = false;
            btn.textContent = label || 'Проверить обновление';
        }
    }
    /** Установка ИЛИ обновление выбранного бэкенда — одна команда, как в llama-плашке. */
    async install() {
        if (this.busy)
            return;
        if (!this.state.backend) {
            this.setStatus('выберите бэкенд');
            return;
        }
        this.busy = true;
        const wasInstalled = this.state.installed;
        this.setStatus(wasInstalled ? 'обновляю движок…' : 'устанавливаю движок…');
        this.download = { current: 0, total: 0, speed: 0 };
        this.renderProgress();
        this.setButtonsDisabled(true);
        try {
            await ttsDownloadEngine(this.state.backend, this.state.engineDir || this.defaults.engine_dir);
            this.state.updateAvailable = false;
            await this.refreshLocalStatus();
            // Подпись выбранного бэкенда в списке меняется на «— установлен».
            // renderBackendSelect() сам пересчитывает состояние и перерисовывает кнопки.
            this.renderBackendSelect();
            this.setStatus(wasInstalled ? 'движок обновлён' : 'движок установлен');
        }
        catch (e) {
            this.setStatus('ошибка: ' + e.message);
        }
        finally {
            this.download = null;
            this.renderProgress();
            this.setButtonsDisabled(false);
            this.busy = false;
        }
    }
    openDeleteDialog() {
        const path = this.root.getElementById('delete_path');
        if (path)
            path.textContent = `${this.state.engineDir}\\${this.state.backend}`;
        const dlg = this.root.getElementById('delete_dialog');
        dlg.hidden = false;
    }
    closeDeleteDialog() {
        this.root.getElementById('delete_dialog').hidden = true;
    }
    async deleteEngine() {
        if (this.busy)
            return;
        this.closeDeleteDialog();
        this.busy = true;
        this.setStatus('удаляю движок…');
        this.setButtonsDisabled(true);
        try {
            const res = await ttsDeleteEngine(this.state.backend, this.state.engineDir || this.defaults.engine_dir);
            const freed = res.freed_bytes ? ` (${(res.freed_bytes / 1048576).toFixed(1)} МБ)` : '';
            this.setStatus(`движок удалён${freed}`);
            this.state.updateAvailable = false;
            this.state.latestVersion = null;
            await this.refreshLocalStatus();
            this.renderBackendSelect(); // пересчитывает состояние и перерисовывает кнопки
        }
        catch (e) {
            this.setStatus('ошибка удаления: ' + e.message);
        }
        finally {
            this.setButtonsDisabled(false);
            this.busy = false;
        }
    }
    async unload() {
        try {
            await ttsUnload();
            this.setStatus('движок выгружен (VRAM освобождён)');
        }
        catch (e) {
            this.setStatus('ошибка выгрузки: ' + e.message);
        }
    }
    /** Блокирует кнопки на время скачивания/удаления — иначе можно кликнуть дважды. */
    setButtonsDisabled(disabled) {
        for (const id of ['install', 'checkUpdate', 'installUpdate', 'remove', 'unload']) {
            const b = this.root.getElementById(id);
            b.disabled = disabled;
        }
    }
    async pickDir(field) {
        const picked = await open({ directory: true }).catch(() => null);
        if (!picked || typeof picked !== 'string')
            return;
        if (field === 'engine_dir') {
            this.settings.engine_dir = picked;
            this.state.engineDir = picked;
        }
        else {
            this.settings.models_dir = picked;
            this.state.modelsDir = picked;
            this.root.getElementById('models_dir_input').value = picked;
        }
        await this.saveSettings();
        // Сменилась папка движка — перечитываем локальный статус под новый путь.
        if (field === 'engine_dir') {
            // Сетевой апдейт относился к прежней папке — про новую мы ничего не знаем.
            this.clearRemoteUpdateState();
            await this.refreshLocalStatus();
            this.renderEngineState();
        }
    }
    async saveSettings() {
        this.settings.engine_dir = this.state.engineDir;
        this.settings.models_dir = this.state.modelsDir;
        this.settings.engine_backend = this.state.backend;
        try {
            await ttsSaveSettings(this.settings);
        }
        catch (e) {
            this.setStatus('не удалось сохранить настройки: ' + e.message);
        }
    }
    setStatus(s) {
        const el = this.root.getElementById('status');
        if (el)
            el.textContent = s;
    }
    renderProgress() {
        const box = this.root.getElementById('progress');
        const bar = box.firstElementChild;
        const d = this.download;
        if (!d) {
            box.style.display = 'none';
            return;
        }
        box.style.display = 'block';
        bar.style.width = d.total > 0 ? `${Math.min(100, (d.current / d.total) * 100)}%` : '100%';
        // Числа в статусе: без них длинная загрузка выглядит как зависание
        // (именно на это жаловался юзер: «нажал, ничего не происходит»).
        const mb = (b) => (b / 1048576).toFixed(1);
        const speed = d.speed > 1024 ? ` · ${mb(d.speed)} МБ/с` : '';
        const size = d.total > 0 ? `${mb(d.current)} / ${mb(d.total)} МБ` : `${mb(d.current)} МБ`;
        this.setStatus(`скачивание… ${size}${speed}`);
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
