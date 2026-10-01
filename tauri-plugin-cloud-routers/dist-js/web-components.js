import { getCombos, getStatus, installOrUpdate, onProgress, openDashboard, ROUTER_IDS, setApiKey, setRouterDir, stop, } from './index';
import { checkUpdate, getUpdateState, onUpdateState, setUpdateState } from './updates';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
const STYLE = `
  :host { display: block; color: var(--text, #e6e6e6); font-family: var(--font, system-ui, sans-serif); font-size: 13px; }
  .row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .row + .row { margin-top: 8px; }
  button { padding: 6px 10px; cursor: pointer; border-radius: 6px; border: 1px solid var(--border, #333);
           background: var(--session-hover, #2a2a2a); color: var(--text, #e6e6e6); font: inherit; font-size: 13px; }
  button.primary { background: var(--primary, #4a90d9); color: #fff; border-color: var(--primary, #4a90d9); }
  button:disabled { opacity: .5; cursor: default; }
  button.busy { position: relative; pointer-events: none; color: transparent !important; }
  button.busy::after { content: ''; position: absolute; top: 50%; left: 50%; width: 14px; height: 14px;
                        margin: -7px 0 0 -7px;
                        border: 2px solid var(--primary, #4a90d9); border-top-color: transparent; border-radius: 50%;
                        animation: spin 0.8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .field { margin-top: 8px; }
  .field input { flex: 1; padding: 6px 8px; border-radius: 6px; border: 1px solid var(--border, #333);
                 background: var(--session-hover, #2a2a2a); color: var(--text, #e6e6e6); font: inherit; font-size: 13px; }
  .api-ok { color: var(--success, #3fa45b); font-size: 12px; margin-left: 4px; }
  .api-err { color: var(--warning, #d8a13a); font-size: 12px; margin-left: 4px; }
  .status { display: flex; gap: 8px; align-items: center; }
  .spinner { display: inline-block; width: 12px; height: 12px; vertical-align: -1px;
             border: 2px solid var(--primary, #4a90d9); border-top-color: transparent;
             border-radius: 50%; animation: spin 0.8s linear infinite; }
  .dot { width: 9px; height: 9px; border-radius: 50%; background: var(--text-muted, #777); flex: 0 0 auto; }
  .dot.on { background: var(--success, #3fa45b); }
  .dot.warn { background: var(--warning, #d8a13a); }
  .update-badge { display: inline-block; width: 8px; height: 8px; border-radius: 50%;
                  background: #4caf50; margin-left: 6px; box-shadow: 0 0 6px #4caf50; }
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
  .tabs { display: flex; gap: 4px; margin-bottom: 10px; }
  .tabs button { padding: 4px 10px; font-size: 12px; border-radius: 4px; }
  .tabs button.active { background: var(--primary, #4a90d9); color: #fff; border-color: var(--primary, #4a90d9); }
`;
function esc(s) {
    return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}
function toast(msg, kind = 'success') {
    const el = document.createElement('div');
    el.textContent = msg;
    el.style.cssText =
        `position:fixed; right:16px; bottom:16px; z-index:5000; max-width:420px; padding:10px 14px;` +
            `border-radius:8px; font:13px var(--font, system-ui, sans-serif); box-shadow:0 3px 14px rgba(0,0,0,.4);` +
            `background:${kind === 'success' ? 'var(--primary, #4a90d9)' : 'var(--danger, #b54242)'}; color:#fff;`;
    document.body.appendChild(el);
    setTimeout(() => el.remove(), 3500);
}
function logPlugin(msg) {
    void invoke('plugin:logs|log_frontend_event', { level: 'FE', msg }).catch(() => { });
}
/** Подпись вкладки. Идентификаторы берутся из `ROUTER_IDS` (SSOT). */
const ROUTER_LABELS = {
    '9router': '9Router',
    extremerouter: 'ExtremeRouter',
    omniroute: 'OmniRoute',
};
export class CloudRoutersPanel extends HTMLElement {
    constructor() {
        super();
        this.router = '9router';
        this.states = new Map();
        this.combosByRouter = new Map();
        this.offProgress = null;
        this.visibilityObserver = null;
        this.refreshTimer = null;
        this.root = this.attachShadow({ mode: 'open' });
    }
    // ── Состояние: единственное место чтения/записи, ключ — роутер ──────────────
    stateOf(router) {
        return this.states.get(router) ?? { kind: 'loading' };
    }
    /** Текущий статус активного роутера; null пока он неизвестен или ошибка. */
    get status() {
        const s = this.stateOf(this.router);
        return s.kind === 'ready' ? s.status : null;
    }
    get combos() {
        return this.combosByRouter.get(this.router) ?? [];
    }
    /**
     * Запись состояния — всегда с явным роутером. Иначе ответ на действие, начатое
     * на одной вкладке, лёг бы в слот той, на которую юзер успел переключиться.
     */
    setStatus(router, status) {
        this.states.set(router, { kind: 'ready', status });
    }
    setCombos(router, combos) {
        this.combosByRouter.set(router, combos);
    }
    connectedCallback() {
        this.render();
        this.unsubUpdate = onUpdateState(() => this.renderUpdateState(getUpdateState(this.router)));
        void this.refresh();
        onProgress((p) => {
            if (p.router === this.router)
                this.onProgress(p.text, p.done, p.total);
        }).then((off) => {
            this.offProgress = off;
        });
        this.observeVisibility();
        this.refreshTimer = window.setInterval(() => void this.refresh(), 15000);
    }
    disconnectedCallback() {
        this.unsubUpdate?.();
        this.offProgress?.();
        this.offProgress = null;
        this.visibilityObserver?.disconnect();
        this.visibilityObserver = null;
        if (this.refreshTimer !== null) {
            clearInterval(this.refreshTimer);
            this.refreshTimer = null;
        }
    }
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
    /**
     * Обновить статус роутера. Ответ пишется в слот ИМЕННО `target`, поэтому быстрые
     * клики по вкладкам не могут записать статус чужого роутера (гонка last-write-wins).
     *
     * Кэш НЕ затирается: если статус уже `ready`, он остаётся на экране, пока идёт
     * фоновая проверка (stale-while-revalidate) — переключение вкладок не мигает.
     */
    async refresh(target = this.router) {
        if (!this.states.has(target)) {
            this.states.set(target, { kind: 'loading' });
            this.render();
        }
        try {
            this.states.set(target, { kind: 'ready', status: await getStatus(target) });
        }
        catch (e) {
            const msg = `Не удалось получить статус ${target}: ${String(e)}`;
            logPlugin(`[cloud-routers] ${msg}`);
            toast(msg, 'error');
            this.states.set(target, { kind: 'error', message: msg });
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
        if (total > 0 && done >= total) {
            logPlugin(`[cloud-routers] установка завершена: ${text}`);
            void this.refresh();
        }
    }
    setBtnBusy(selector, v) {
        const btn = this.root.querySelector(selector);
        if (btn) {
            btn.disabled = v;
            btn.classList.toggle('busy', v);
        }
    }
    notifyCombosChanged(router = this.router) {
        window.dispatchEvent(new CustomEvent('cloud-routers:combos-changed', {
            detail: { router, combos: this.combosByRouter.get(router) ?? [] },
        }));
    }
    async onInstall() {
        const router = this.router;
        this.setBtnBusy('.install', true);
        try {
            this.setStatus(router, await installOrUpdate(router, true));
            this.setCombos(router, await getCombos(router).catch(() => []));
        }
        catch (e) {
            const msg = `Ошибка установки: ${String(e)}`;
            logPlugin(`[cloud-routers] ${msg}`);
            toast(msg, 'error');
            const label = this.root.querySelector('.progress-status');
            const box = this.root.querySelector('.progress-container');
            if (box)
                box.classList.add('on');
            if (label)
                label.textContent = `Ошибка: ${String(e)}`;
            return;
        }
        finally {
            this.setBtnBusy('.install', false);
        }
        this.notifyCombosChanged(router);
        this.render();
    }
    async onOpen() {
        const router = this.router;
        this.setBtnBusy('.open', true);
        try {
            await openDashboard(router);
            const fresh = await getStatus(router).catch(() => null);
            if (fresh)
                this.setStatus(router, fresh);
        }
        catch (e) {
            const msg = `Ошибка открытия Web UI: ${String(e)}`;
            logPlugin(`[cloud-routers] ${msg}`);
            toast(msg, 'error');
        }
        finally {
            this.setBtnBusy('.open', false);
        }
        this.render();
    }
    async onStop() {
        const router = this.router;
        this.setBtnBusy('.stop', true);
        try {
            this.setStatus(router, await stop(router));
            this.setCombos(router, []);
            this.notifyCombosChanged(router);
        }
        catch (e) {
            const msg = `Ошибка остановки: ${String(e)}`;
            logPlugin(`[cloud-routers] ${msg}`);
            toast(msg, 'error');
        }
        finally {
            this.setBtnBusy('.stop', false);
        }
        this.render();
    }
    async onShowCombos() {
        const router = this.router;
        this.setBtnBusy('.refresh', true);
        try {
            this.setCombos(router, await getCombos(router));
            this.setStatus(router, await getStatus(router));
        }
        catch (e) {
            const msg = `Ошибка обновления комбо: ${String(e)}`;
            logPlugin(`[cloud-routers] ${msg}`);
            toast(msg, 'error');
        }
        finally {
            this.setBtnBusy('.refresh', false);
        }
        this.notifyCombosChanged(router);
        this.render();
    }
    async onSaveApiKey() {
        const router = this.router;
        const input = this.root.querySelector('.api-key-input');
        const ok = this.root.querySelector('.api-ok');
        const err = this.root.querySelector('.api-err');
        if (!input)
            return;
        ok && (ok.textContent = '');
        err && (err.textContent = '');
        this.setBtnBusy('.save-key', true);
        try {
            this.setStatus(router, await setApiKey(router, input.value.trim()));
            ok && (ok.textContent = '✓ сохранён');
        }
        catch (e) {
            const msg = `Ошибка сохранения API-ключа: ${String(e)}`;
            logPlugin(`[cloud-routers] ${msg}`);
            toast(msg, 'error');
            if (err)
                err.textContent = `Ошибка: ${String(e)}`;
        }
        finally {
            this.setBtnBusy('.save-key', false);
        }
        this.render();
    }
    async onSetDir() {
        const router = this.router;
        this.setBtnBusy('.setdir', true);
        try {
            const sel = await openDialog({ directory: true });
            if (!sel)
                return;
            const path = Array.isArray(sel) ? sel[0] : sel;
            if (!path)
                return;
            this.setStatus(router, await setRouterDir(router, path));
            this.setCombos(router, []);
            this.notifyCombosChanged(router);
            this.render();
        }
        catch (e) {
            const msg = `Ошибка смены пути: ${String(e)}`;
            logPlugin(`[cloud-routers] ${msg}`);
            toast(msg, 'error');
            const label = this.root.querySelector('.progress-status');
            const box = this.root.querySelector('.progress-container');
            if (box)
                box.classList.add('on');
            if (label)
                label.textContent = `Ошибка: ${String(e)}`;
        }
        finally {
            this.setBtnBusy('.setdir', false);
        }
    }
    async onCheckUpdate() {
        const router = this.router;
        const btn = this.root.querySelector('.check-update');
        const label = this.root.querySelector('.progress-status');
        const box = this.root.querySelector('.progress-container');
        if (btn)
            btn.disabled = true;
        // На время проверки кнопка «Обновить» прячется; результат придёт через setUpdateState.
        this.renderUpdateState({ hasUpdate: false });
        try {
            const next = await checkUpdate(router);
            if (box)
                box.classList.add('on');
            if (label) {
                label.textContent = next.hasUpdate
                    ? `Доступно обновление: v${next.tag ?? ''}`
                    : `Актуален${this.status?.version ? ` (v${this.status.version})` : ''}`;
            }
        }
        catch (e) {
            const msg = `Ошибка проверки обновления: ${String(e)}`;
            logPlugin(`[cloud-routers] ${msg}`);
            toast(msg, 'error');
            if (box)
                box.classList.add('on');
            if (label)
                label.textContent = `Ошибка проверки обновления: ${String(e)}`;
        }
        finally {
            if (btn)
                btn.disabled = false;
        }
    }
    async onInstallUpdate() {
        const router = this.router;
        await this.onInstall();
        setUpdateState(router, { hasUpdate: false });
    }
    switchRouter(router) {
        if (router === this.router)
            return;
        this.router = router;
        // Рендер сразу: закэшированное состояние этой вкладки (или скелет, если её
        // ещё не видели). Сброса в «не установлено» здесь быть не должно — отсюда и был flash.
        this.render();
        void this.refresh(router);
    }
    render() {
        const state = this.stateOf(this.router);
        // Состояние НЕИЗВЕСТНО — значит кнопки решений рисовать рано: любая из них
        // («Установить» в первую очередь) была бы догадкой, а не фактом.
        const known = state.kind === 'ready';
        const s = this.status;
        const installed = s?.installed ?? false;
        const running = s?.running ?? false;
        const dotClass = installed && running ? 'dot on' : installed ? 'dot warn' : 'dot';
        const message = known
            ? s.message
            : state.kind === 'error'
                ? state.message
                : 'Проверка состояния…';
        const version = s?.version ? `v${s.version}` : '—';
        const nodeVersion = s?.node_version ? `Node ${s.node_version}` : 'Node —';
        const actions = known
            ? `<button class="primary install" style="${installed ? 'display:none;' : ''}">Установить</button>
         <button class="check-update" style="${installed ? '' : 'display:none;'}">Проверить обновление</button>
         <button class="primary install-update" style="display:none;">Обновить</button>
         <button class="open" ${installed ? '' : 'disabled'}>Открыть Web UI</button>
         <button class="stop" ${running ? '' : 'disabled'}>Остановить</button>
         <button class="refresh" ${installed ? '' : 'disabled'}>⟳ Обновить комбо</button>`
            : `<span class="muted">${state.kind === 'error'
                ? 'Состояние проверить не удалось — повторите через «Обновить комбо»'
                : '<span class="spinner"></span> Проверка состояния…'}</span>`;
        this.root.innerHTML = `
      <style>${STYLE}</style>
      <div class="tabs">
        ${ROUTER_IDS.map((id) => `<button class="${this.router === id ? 'active' : ''}" data-router="${id}">${ROUTER_LABELS[id]}</button>`).join('')}
      </div>
      <div class="status">
        <span class="${dotClass}"></span>
        <span>${message}</span>
        <span id="updateBadge" class="update-badge" style="display:none;"></span>
      </div>
      <div class="row muted">
        <span>${this.router}: ${version}</span>
        <span>·</span>
        <span>${nodeVersion}</span>
        <span>·</span>
        <span>порт ${s?.port ?? '—'}</span>
      </div>
      <div class="row">
        ${actions}
        <button class="setdir">Изменить путь</button>
      </div>
      <div class="row muted">
        <span>Каталог базы данных: ${s?.data_dir ? esc(s.data_dir) : '—'}</span>
      </div>
      <div class="row muted">
        <span>Статус БД: ${s?.db_present ? 'найдена' : '⚠ не найдена'}</span>
      </div>
      ${!installed && (s?.node_present || s?.server_present)
            ? '<div class="muted warn-hint">Частичная установка: найдены не все компоненты. Нажмите «Установить», чтобы починить.</div>'
            : ''}
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
      <div class="row path">Путь установки программы: ${s?.path ? esc(s.path) : '—'}</div>
    `;
        this.root.querySelectorAll('.tabs button').forEach((b) => {
            b.addEventListener('click', () => this.switchRouter(b.dataset.router));
        });
        this.root.querySelector('.install')?.addEventListener('click', () => void this.onInstall());
        this.root.querySelector('.check-update')?.addEventListener('click', () => void this.onCheckUpdate());
        this.root.querySelector('.install-update')?.addEventListener('click', () => void this.onInstallUpdate());
        this.root.querySelector('.stop')?.addEventListener('click', () => void this.onStop());
        this.root.querySelector('.refresh')?.addEventListener('click', () => void this.onShowCombos());
        this.root.querySelector('.open')?.addEventListener('click', () => void this.onOpen());
        this.root.querySelector('.setdir')?.addEventListener('click', () => void this.onSetDir());
        this.renderUpdateState(getUpdateState(this.router));
    }
    renderUpdateState(s) {
        const badge = this.root.querySelector('#updateBadge');
        const updateBtn = this.root.querySelector('.install-update');
        // И бейдж, и кнопка выводятся из ОДНОГО состояния: раньше в `else` прятался
        // только бейдж, и кнопка «Обновить» оставалась висеть после установки.
        const display = s.hasUpdate ? 'inline-block' : 'none';
        if (badge)
            badge.style.display = display;
        if (updateBtn)
            updateBtn.style.display = display;
    }
}
if (!customElements.get('cloud-routers-panel')) {
    customElements.define('cloud-routers-panel', CloudRoutersPanel);
}
