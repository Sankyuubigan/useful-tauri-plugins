/**
 * <image-bundle-tab service="image|video"> — одна вкладка медиа-блока:
 * селектор бандлов («скачать → активировать»), файлы, диск/память и папка
 * набора («Изменить путь…» — диалог → проверка файлов → set; «Сбросить путь» —
 * возврат к дефолту хранилища KingOrchData). Работает поверх BundleService
 * (services.ts) — один рендер на обе модальности.
 */
import { STYLE, esc, formatBytes, toast, progressText } from './ui-shared';
import { notifyBundleChanged } from '../index';
import { serviceFor } from './services';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { listen } from '@tauri-apps/api/event';
export class BundleTabElement extends HTMLElement {
    constructor() {
        super(...arguments);
        this.busy = false;
        this.appliedBundle = '';
        /** Статусы бандлов из последнего refresh (fully_downloaded и файлы) —
         * источник гейта кнопки «Активировать». */
        this.statuses = new Map();
    }
    connectedCallback() {
        if (this.root)
            return;
        const kind = this.getAttribute('service') ?? 'image';
        const svc = serviceFor(kind);
        if (!svc) {
            this.textContent = `Неизвестный сервис вкладки: «${kind}» (ожидается image|video)`;
            return;
        }
        this.service = svc;
        this.root = this.attachShadow({ mode: 'open' });
        this.root.innerHTML = `
      <style>${STYLE}</style>
      <div>
        <div class="row">
          <label>${esc(svc.kindLabel)}:</label>
          <select id="bundle" style="flex:1; min-width:220px;"><option value="">…</option></select>
          <button id="activate" class="primary" style="display:none;">Активировать</button>
        </div>
        <div class="row"><label>Набор:</label><span id="label" class="hint">…</span></div>
        <div id="note" class="hint"></div>
        <ul class="files" id="files"></ul>
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
      </div>`;
        this.root.getElementById('bundle').addEventListener('change', () => this.onBundleChange());
        this.root.getElementById('activate').addEventListener('click', () => this.onActivate());
        this.root.getElementById('download').addEventListener('click', () => this.onDownload());
        this.root.getElementById('changeDir').addEventListener('click', () => this.onChangeDir());
        this.root.getElementById('resetDir').addEventListener('click', () => this.onResetDir());
        this.root.getElementById('resume').addEventListener('click', () => this.onResume());
        this.root.getElementById('deleteFiles').addEventListener('click', () => this.confirmDelete());
        this.root.getElementById('modalCancel').addEventListener('click', () => this.closeModal());
        this.root.getElementById('modalOk').addEventListener('click', () => void this.doDeleteFiles());
        void this.refresh();
        void listen('downloader:progress', (e) => {
            const p = e.payload;
            if (p.kind && p.kind !== svc.progressKind)
                return;
            this.setProgress(p.downloaded, p.total);
            if (p.status === 'done' || p.status === 'error') {
                setTimeout(() => { this.showProgress(false); void this.refresh(); }, 400);
            }
        }).then((u) => { this.unlisten = u; }).catch(() => { });
    }
    disconnectedCallback() {
        this.unlisten?.();
    }
    setProgress(downloaded, total) {
        const bar = this.root.getElementById('progressBar');
        bar.style.width = `${total > 0 ? (downloaded / total) * 100 : 0}%`;
        const st = this.root.getElementById('progressStatus');
        st.textContent = progressText(downloaded, total);
    }
    showProgress(on) {
        this.root.getElementById('progress').style.display = on ? 'block' : 'none';
    }
    async refresh() {
        const svc = this.service;
        if (!svc)
            return;
        let info;
        try {
            info = await svc.info(null);
        }
        catch (e) {
            const label = this.root.getElementById('label');
            if (label)
                label.textContent = `Ошибка: ${e}`;
            return;
        }
        // Селектор бандлов: активный — текущий; остальные — с меткой скачанности.
        this.appliedBundle = info.bundle_name;
        const sel = this.root.getElementById('bundle');
        let entries = [];
        try {
            entries = await svc.entries();
        }
        catch { /* каталог встроен — просто без списка */ }
        sel.innerHTML = '';
        this.statuses = new Map();
        this.statuses.set(info.bundle_name, info);
        for (const entry of entries) {
            if (this.statuses.has(entry.name))
                continue;
            try {
                this.statuses.set(entry.name, await svc.info(entry.name));
            }
            catch { /* нет на диске — метка ниже */ }
        }
        if (entries.length === 0) {
            const o = document.createElement('option');
            o.value = info.bundle_name;
            o.textContent = info.label;
            sel.appendChild(o);
        }
        else {
            for (const entry of entries) {
                const st = this.statuses.get(entry.name);
                const o = document.createElement('option');
                o.value = entry.name;
                if (!st) {
                    o.textContent = `${entry.label} (не скачан)`;
                }
                else if (st.fully_downloaded) {
                    o.textContent = `${st.label} — скачан${entry.is_default ? ' (по умолчанию)' : ''}`;
                }
                else {
                    o.textContent = `${st.label} — частично (${st.files.filter((f) => f.exists).length}/${st.files.length})`;
                }
                sel.appendChild(o);
            }
        }
        sel.value = info.bundle_name;
        this.root.getElementById('activate').style.display = 'none';
        this.root.getElementById('label').textContent =
            `${info.label}${info.fully_downloaded ? '' : ' (не все файлы скачаны)'}`;
        this.root.getElementById('note').textContent = info.note || '';
        const ul = this.root.getElementById('files');
        ul.innerHTML = '';
        let anyExists = false;
        for (const f of info.files) {
            if (f.exists)
                anyExists = true;
            const li = document.createElement('li');
            li.className = f.exists ? 'ok' : 'missing';
            const size = f.size_bytes ? ` — ${formatBytes(f.size_bytes)}` : '';
            li.textContent = `${f.exists ? '✓' : '○'} ${esc(f.filename)} (${f.role})${size}`;
            ul.appendChild(li);
        }
        const totalGb = (info.total_bytes / 1024 / 1024 / 1024).toFixed(1);
        this.root.getElementById('disk').textContent =
            `~${totalGb} ГБ на диске · свободно ${info.free_space_gb} ГБ`;
        const memLine = `GPU: ${info.vram_fast_gb ?? '?'} ГБ (быстро) / ${info.vram_min_gb ?? '?'} ГБ (мин.) · RAM ≥${info.ram_min_gb ?? '?'} ГБ`;
        this.root.getElementById('mem').textContent = memLine;
        this.root.getElementById('dir').textContent = info.save_dir;
        const dlBtn = this.root.getElementById('download');
        dlBtn.style.display = info.fully_downloaded ? 'none' : 'inline-block';
        dlBtn.textContent = anyExists ? 'Докачать файлы' : 'Скачать набор';
        const resumeBtn = this.root.getElementById('resume');
        resumeBtn.style.display = info.fully_downloaded ? 'none' : 'inline-block';
        resumeBtn.disabled = !anyExists; // «Докачать» без ни одного файла = то же, что «Скачать»
        // «Сбросить путь» виден только когда путь кастомный (вернёт к дефолту
        // хранилища KingOrchData); дефолт сбрасывать нечего.
        this.root.getElementById('resetDir').style.display = info.save_dir ? 'inline-block' : 'none';
        this.root.getElementById('deleteFiles').style.display = info.fully_downloaded ? 'inline-block' : 'none';
        if (svc.memoryProbe) {
            try {
                this.root.getElementById('mem').textContent = await svc.memoryProbe(info.bundle_name);
            }
            catch { /* NVML может отсутствовать — остаётся строка пресета */ }
        }
    }
    onBundleChange() {
        const sel = this.root.getElementById('bundle');
        if (sel.value === this.appliedBundle) {
            this.root.getElementById('activate').style.display = 'none';
            return;
        }
        // Гейт активации (решение юзера 25.10.2026): недокачанный бандл нельзя
        // активировать — бэкенд это запрещает, кнопка честно не появляется.
        const st = this.statuses.get(sel.value);
        const activatable = !st || st.fully_downloaded;
        this.root.getElementById('activate').style.display = activatable ? 'inline-block' : 'none';
    }
    async onActivate() {
        const svc = this.service;
        if (!svc || this.busy)
            return;
        const sel = this.root.getElementById('bundle');
        if (!sel.value)
            return;
        this.busy = true;
        try {
            await svc.activate(sel.value);
            notifyBundleChanged();
            toast(`Активный ${svc.kindLabel.toLowerCase()}: ${sel.value}`);
        }
        catch (e) {
            toast(`Ошибка активации: ${e}`, 'error');
        }
        finally {
            this.busy = false;
            await this.refresh();
        }
    }
    /** Сборка целевой папки скачивания: если хоть один файл уже на диске —
     * продолжаем в текущей папке; иначе диалог. Выбранная папка = ПУЛ для всех
     * наборов (25.10.2026): подпапка `${base}/${bundle_name}` больше не создаётся —
     * она порождала перекачку общих файлов и «3 файла есть, 1 нет» при
     * переключении активного бандла (инцидент 25.10.2026). */
    async onDownload() {
        const svc = this.service;
        if (!svc || this.busy)
            return;
        this.busy = true;
        const btn = this.root.getElementById('download');
        btn.disabled = true;
        this.showProgress(true);
        this.setProgress(0, 0);
        try {
            const cur = await svc.info(null);
            let target = cur.save_dir;
            const anyExists = cur.files.some((f) => f.exists);
            if (!anyExists || !target) {
                const sel = await openDialog({ directory: true, title: 'Куда скачать набор?' });
                if (!sel)
                    return;
                const base = Array.isArray(sel) ? sel[0] : sel;
                if (!base)
                    return;
                target = base;
            }
            await svc.download(target, cur.bundle_name);
            notifyBundleChanged();
            toast(`Набор ${svc.kind === 'video' ? 'видео' : 'изображений'} скачан!`);
        }
        catch (e) {
            toast(`Ошибка скачивания набора: ${e}`, 'error');
        }
        finally {
            this.busy = false;
            btn.disabled = false;
            this.showProgress(false);
            await this.refresh();
        }
    }
    /** «Изменить путь…»: диалог → проверка наличия файлов → set → честный тост
     * о нехватке. Выбранная папка становится пулом этого набора. */
    async onChangeDir() {
        const svc = this.service;
        if (!svc || this.busy)
            return;
        this.busy = true;
        const btn = this.root.getElementById('changeDir');
        btn.disabled = true;
        try {
            const sel = await openDialog({ directory: true, title: `Папка набора ${svc.kind === 'video' ? 'VideoGEN' : 'ImageGEN'}` });
            if (!sel)
                return;
            const path = Array.isArray(sel) ? sel[0] : sel;
            if (!path)
                return;
            let valid;
            try {
                valid = await svc.validateDir(path, null);
            }
            catch (e) {
                toast(`Ошибка проверки набора: ${e}`, 'error');
                return;
            }
            await svc.setCustomDir(path);
            notifyBundleChanged();
            if (valid.valid) {
                toast('Папка набора обновлена: все файлы на месте.');
            }
            else {
                toast(`Папка выбрана. Не хватает файлов: ${valid.missing.length}. Нажмите «Докачать файлы».`);
            }
            await this.refresh();
        }
        catch (e) {
            toast(`Не удалось сменить папку набора: ${e}`, 'error');
        }
        finally {
            btn.disabled = false;
            this.busy = false;
        }
    }
    /** «Сбросить путь»: вернуть папку набора к дефолту хранилища. */
    async onResetDir() {
        const svc = this.service;
        if (!svc || this.busy)
            return;
        this.busy = true;
        try {
            await svc.clearCustomDir();
            notifyBundleChanged();
            toast('Папка сброшена: вернулись к хранилищу по умолчанию.');
        }
        catch (e) {
            toast(`Ошибка: ${e}`, 'error');
        }
        finally {
            this.busy = false;
            await this.refresh();
        }
    }
    /** Custom-папка (resume): докачать недостающие в текущую папку. */
    async onResume() {
        const svc = this.service;
        if (!svc || this.busy)
            return;
        this.busy = true;
        const btn = this.root.getElementById('resume');
        btn.disabled = true;
        this.showProgress(true);
        this.setProgress(0, 0);
        try {
            const cur = await svc.info(null);
            await svc.download(cur.save_dir, cur.bundle_name);
            notifyBundleChanged();
            toast('Недостающие файлы набора скачаны!');
        }
        catch (e) {
            toast(`Ошибка докачивания набора: ${e}`, 'error');
        }
        finally {
            this.busy = false;
            if (btn)
                btn.disabled = false;
            this.showProgress(false);
            await this.refresh();
        }
    }
    confirmDelete() {
        this.root.getElementById('overlay').classList.add('open');
    }
    closeModal() {
        this.root.getElementById('overlay').classList.remove('open');
    }
    async doDeleteFiles() {
        const svc = this.service;
        if (!svc || this.busy)
            return;
        this.busy = true;
        this.closeModal();
        try {
            await svc.removeFiles();
            notifyBundleChanged();
            toast('Файлы набора удалены.');
        }
        catch (e) {
            toast(`Ошибка удаления: ${e}`, 'error');
        }
        finally {
            this.busy = false;
            await this.refresh();
        }
    }
}
if (!customElements.get('image-bundle-tab')) {
    customElements.define('image-bundle-tab', BundleTabElement);
}
