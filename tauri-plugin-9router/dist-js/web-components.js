import { getCombos, getStatus, installOrUpdate, onProgress, openDashboard, setApiKey, setRouterDir, } from './index';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
const STYLE = `
  :host { display: block; color: var(--text, #e6e6e6); font-family: var(--font, system-ui, sans-serif); font-size: 13px; }
  .row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .row + .row { margin-top: 8px; }
  button { padding: 6px 10px; cursor: pointer; border-radius: 6px; border: 1px solid var(--border, #333);
           background: var(--session-hover, #2a2a2a); color: var(--text, #e6e6e6); font: inherit; font-size: 13px; }
  button.primary { background: var(--primary, #4a90d9); color: #fff; border-color: var(--primary, #4a90d9); }
  button:disabled { opacity: .5; cursor: default; }
  .field { margin-top: 8px; }
  .field input { flex: 1; padding: 6px 8px; border-radius: 6px; border: 1px solid var(--border, #333);
                background: var(--session-hover, #2a2a2a); color: var(--text, #e6e6e6); font: inherit; font-size: 13px; }
  .api-ok { color: var(--success, #3fa45b); font-size: 12px; margin-left: 4px; }
  .api-err { color: var(--warning, #d8a13a); font-size: 12px; margin-left: 4px; }
  .status { display: flex; gap: 8px; align-items: center; }
  .dot { width: 9px; height: 9px; border-radius: 50%; background: var(--text-muted, #777); flex: 0 0 auto; }
  .dot.on { background: var(--success, #3fa45b); }
  .dot.warn { background: var(--warning, #d8a13a); }
  .muted { color: var(--text-muted, #999); font-size: 12px; }
  .path { color: var(--text-muted, #999); font-size: 12px; word-break: break-all; }
  .progress-container { display: none; margin-top: 10px; }
  .progress-container.on { display: block; }
  .progress-status { color: var(--text-muted, #999); font-size: 12px; margin-bottom: 5px; word-break: break-all; }
  .progress-track { height: 8px; border-radius: 6px; background: var(--border, #333); overflow: hidden; }
  .progress-bar { height: 100%; width: 0%; background: var(--primary, #4a90d9); transition: width .15s linear; }
  .warn-hint { color: var(--warning, #d8a13a); margin-top: 6px; }
  .combos { margin-top: 10px; display: none; }
  .combos.on { display: block; }
  .combo { padding: 4px 0; border-bottom: 1px solid var(--border, #333); }
  .combo:last-child { border-bottom: 0; }
  .combo .name { font-weight: 600; }
  .combo .models { color: var(--text-muted, #999); font-size: 12px; word-break: break-all; }
`;
export class NineRouterPanel extends HTMLElement {
    constructor() {
        super();
        this.status = null;
        this.combos = [];
        this.busy = false;
        this.offProgress = null;
        this.visibilityObserver = null;
        this.refreshTimer = null;
        this.root = this.attachShadow({ mode: 'open' });
    }
    connectedCallback() {
        this.render();
        void this.refresh();
        onProgress((p) => this.onProgress(p.text, p.done, p.total)).then((off) => {
            this.offProgress = off;
        });
        this.observeVisibility();
        // Страховка от устаревшего статуса: периодический опрос, пока панель жива.
        this.refreshTimer = window.setInterval(() => void this.refresh(), 15000);
    }
    disconnectedCallback() {
        this.offProgress?.();
        this.offProgress = null;
        this.visibilityObserver?.disconnect();
        this.visibilityObserver = null;
        if (this.refreshTimer !== null) {
            clearInterval(this.refreshTimer);
            this.refreshTimer = null;
        }
    }
    /** Панель в Settings монтируется один раз при старте приложения, а раздел
     *  переключается классом `.active`. Перечитываем статус каждый раз, когда
     *  раздел становится видимым, — чтобы панель не «врала» устаревшим статусом. */
    observeVisibility() {
        let host = this;
        while (host && host !== document.body && !host.classList?.contains('view')) {
            host = host.parentElement;
        }
        if (!host || host === document.body)
            return;
        this.visibilityObserver = new MutationObserver(() => {
            if (host.classList.contains('active'))
                void this.refresh();
        });
        this.visibilityObserver.observe(host, { attributes: true, attributeFilter: ['class'] });
    }
    async refresh() {
        try {
            this.status = await getStatus();
        }
        catch (e) {
            this.status = null;
            console.error('[9router] getStatus failed', e);
        }
        this.render();
    }
    onProgress(text, done, total) {
        const bar = this.root.querySelector('.progress-bar');
        const label = this.root.querySelector('.progress-status');
        const box = this.root.querySelector('.progress-container');
        if (box)
            box.classList.add('on');
        if (label)
            label.textContent = text;
        if (bar)
            bar.style.width = total > 0 ? `${Math.min(100, (done / total) * 100).toFixed(1)}%` : '100%';
        // Установка/обновление завершились — принудительно перечитываем статус,
        // чтобы панель показала актуальное состояние, а не «застрявшее» сообщение.
        if (total > 0 && done >= total)
            void this.refresh();
    }
    setBusy(v) {
        this.busy = v;
        this.root.querySelectorAll('button').forEach((b) => (b.disabled = v));
    }
    notifyCombosChanged() {
        window.dispatchEvent(new CustomEvent('9router:combos-changed', { detail: { combos: this.combos } }));
    }
    async onInstall() {
        this.setBusy(true);
        try {
            this.status = await installOrUpdate(true);
            this.combos = await getCombos().catch(() => []);
        }
        catch (e) {
            console.error('[9router] install failed', e);
            const label = this.root.querySelector('.progress-status');
            const box = this.root.querySelector('.progress-container');
            if (box)
                box.classList.add('on');
            if (label)
                label.textContent = `Ошибка: ${String(e)}`;
            return;
        }
        finally {
            this.setBusy(false);
        }
        this.notifyCombosChanged();
        this.render();
    }
    async onOpen() {
        try {
            await openDashboard();
            this.status = await getStatus().catch(() => this.status);
        }
        catch (e) {
            console.error('[9router] openDashboard failed', e);
        }
        this.render();
    }
    async onShowCombos() {
        try {
            this.combos = await getCombos();
            this.status = await getStatus();
        }
        catch (e) {
            console.error('[9router] getCombos failed', e);
        }
        this.notifyCombosChanged();
        this.render();
    }
    async onSaveApiKey() {
        const input = this.root.querySelector('.api-key-input');
        const ok = this.root.querySelector('.api-ok');
        const err = this.root.querySelector('.api-err');
        if (!input)
            return;
        ok && (ok.textContent = '');
        err && (err.textContent = '');
        try {
            this.status = await setApiKey(input.value.trim());
            ok && (ok.textContent = '✓ сохранён');
        }
        catch (e) {
            console.error('[9router] setApiKey failed', e);
            if (err)
                err.textContent = `Ошибка: ${String(e)}`;
        }
        this.render();
    }
    async onSetDir() {
        try {
            const sel = await openDialog({ directory: true });
            if (!sel)
                return;
            const path = Array.isArray(sel) ? sel[0] : sel;
            if (!path)
                return;
            this.status = await setRouterDir(path);
            this.combos = [];
            this.notifyCombosChanged();
            this.render();
        }
        catch (e) {
            console.error('[9router] set dir failed', e);
            const label = this.root.querySelector('.progress-status');
            const box = this.root.querySelector('.progress-container');
            if (box)
                box.classList.add('on');
            if (label)
                label.textContent = `Ошибка: ${String(e)}`;
        }
    }
    render() {
        const s = this.status;
        const installed = s?.installed ?? false;
        const running = s?.running ?? false;
        const dotClass = running ? 'dot on' : installed ? 'dot warn' : 'dot';
        const message = s?.message ?? 'Загрузка...';
        const version = s?.version ? `v${s.version}` : '—';
        const nodeVersion = s?.node_version ? `Node ${s.node_version}` : 'Node —';
        this.root.innerHTML = `
      <style>${STYLE}</style>
      <div class="status">
        <span class="${dotClass}"></span>
        <span>${message}</span>
      </div>
      <div class="row muted">
        <span>9Router: ${version}</span>
        <span>·</span>
        <span>${nodeVersion}</span>
        <span>·</span>
        <span>порт ${s?.port ?? '—'}</span>
      </div>
      <div class="row">
        <button class="primary install">${installed ? 'Обновить версию' : 'Установить'}</button>
        <button class="refresh" ${installed ? '' : 'disabled'}>⟳ Обновить комбо</button>
        <button class="open" ${installed ? '' : 'disabled'}>Открыть Web UI</button>
        <button class="setdir">Изменить путь</button>
        <button class="check">Проверить</button>
      </div>
      ${!installed && (s?.node_present || s?.server_present)
            ? '<div class="muted warn-hint">Частичная установка: найдены не все компоненты 9Router. Нажмите «Установить», чтобы починить.</div>'
            : ''}
      <div class="row field">
        <input class="api-key-input" type="password" placeholder="API-ключ 9Router (для чата через комбо)" autocomplete="off" />
        <button class="savekey">Сохранить ключ</button>
        <span class="api-ok"></span><span class="api-err"></span>
      </div>
      <div class="progress-container">
        <div class="progress-status"></div>
        <div class="progress-track"><div class="progress-bar"></div></div>
      </div>
      <div class="combos ${this.combos.length ? 'on' : ''}">
        ${this.combos
            .map((c) => `<div class="combo"><div class="name">${esc(c.name)}</div>` +
            `<div class="models">${esc(c.models.join(', '))}</div></div>`)
            .join('')}
      </div>
      <div class="row path">${s?.path ? esc(s.path) : ''}</div>
    `;
        this.root.querySelector('.install')?.addEventListener('click', () => void this.onInstall());
        this.root.querySelector('.refresh')?.addEventListener('click', () => void this.onShowCombos());
        this.root.querySelector('.open')?.addEventListener('click', () => void this.onOpen());
        this.root.querySelector('.savekey')?.addEventListener('click', () => void this.onSaveApiKey());
        this.root.querySelector('.setdir')?.addEventListener('click', () => void this.onSetDir());
        this.root.querySelector('.check')?.addEventListener('click', () => void this.refresh());
    }
}
function esc(s) {
    return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}
if (!customElements.get('nine-router-panel')) {
    customElements.define('nine-router-panel', NineRouterPanel);
}
