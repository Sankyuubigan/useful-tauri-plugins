import { getAppVersion, getReleaseHistory, installRelease, checkForUpdate, downloadAndInstallUpdate, getSupportUrl, } from './index';
import { open } from '@tauri-apps/plugin-shell';
/**
 * <about-updates-panel repo="owner/repo"></about-updates-panel>
 *
 * Фреймворк-агностичная плашка «О приложении»: версия, проверка обновлений,
 * история релизов и откат. Работает в vanilla TS, React, Vue, Svelte и т.п.
 *
 * Атрибут `repo` опционален. Сам репозиторий для логики задаётся в
 * tauri.conf.json хоста (plugins.about-updates.repo).
 */
class AboutUpdatesPanel extends HTMLElement {
    constructor() {
        super(...arguments);
        this.repo = '';
        this.busy = false;
    }
    static get observedAttributes() {
        return ['repo'];
    }
    connectedCallback() {
        this.repo = this.getAttribute('repo') ?? '';
        this.root = this.attachShadow({ mode: 'open' });
        this.render();
    }
    attributeChangedCallback() {
        this.repo = this.getAttribute('repo') ?? '';
        if (this.root)
            this.render();
    }
    setStatus(s) {
        const el = this.root.getElementById('status');
        if (el)
            el.textContent = s;
    }
    async render() {
        if (!this.root)
            return;
        this.root.innerHTML = `
      <style>
        :host { display: block; color: var(--text, #333); font-family: var(--font, system-ui, sans-serif); }
        button { margin: 6px 6px 0 0; padding: 6px 10px; cursor: pointer; border-radius: 6px;
                 border: 1px solid var(--border, #ccc); background: var(--session-hover, #eee);
                 color: var(--text, #333); font: inherit; }
        button.primary { background: var(--primary, #4a90d9); color: #fff; border-color: var(--primary, #4a90d9); }
        button:disabled { opacity: .5; cursor: default; }
        .muted { color: var(--text-muted, #888); font-size: 12px; }
        .row { display: flex; gap: 10px; align-items: center; flex-wrap: wrap; margin-top: 8px; }
        pre { white-space: pre-wrap; max-height: 160px; overflow: auto; background: var(--bg-color, #f6f6f6);
              padding: 8px; border: 1px solid var(--border, #ccc); border-radius: 6px; margin: 6px 0 0;
              color: var(--text, #333); }
      </style>
      <div>
        <div><strong>Версия:</strong> <span id="ver">…</span></div>
        <div class="row">
          <button id="check" class="primary">Проверить обновления</button>
          <button id="rollback">История / откат</button>
        </div>
        <div id="status" class="muted" style="margin-top:8px"></div>
        <div id="history" style="margin-top:8px"></div>
      </div>`;
        const ver = await getAppVersion().catch(() => '?');
        this.root.getElementById('ver').textContent = ver;
        const supportUrl = await getSupportUrl().catch(() => null);
        if (supportUrl) {
            const sup = document.createElement('button');
            sup.textContent = 'Поддержать автора';
            sup.addEventListener('click', () => {
                open(supportUrl).catch(() => window.open(supportUrl, '_blank'));
            });
            this.root.getElementById('rollback').parentElement.appendChild(sup);
        }
        this.root.getElementById('check').addEventListener('click', () => this.onCheck());
        this.root.getElementById('rollback').addEventListener('click', () => this.onRollback());
    }
    async onCheck() {
        if (this.busy)
            return;
        this.busy = true;
        this.setStatus('Проверка…');
        try {
            const update = await checkForUpdate();
            if (update) {
                this.setStatus(`Доступно обновление ${update.version}. Установка…`);
                await downloadAndInstallUpdate(update);
                this.setStatus('Обновление установлено, перезапуск…');
            }
            else {
                this.setStatus('У вас последняя версия.');
            }
        }
        catch (e) {
            this.setStatus('Ошибка: ' + e.message);
        }
        finally {
            this.busy = false;
        }
    }
    async onRollback() {
        if (this.busy)
            return;
        this.busy = true;
        this.setStatus('Загрузка истории релизов…');
        try {
            const history = await getReleaseHistory();
            const box = this.root.getElementById('history');
            box.innerHTML = '';
            const title = document.createElement('div');
            title.className = 'muted';
            title.textContent = 'Доступные релизы:';
            box.appendChild(title);
            for (const r of history) {
                const row = document.createElement('div');
                row.style.margin = '4px 0';
                const btn = document.createElement('button');
                btn.textContent = r.isCurrent ? `${r.version} (текущая)` : `Откатить на ${r.version}`;
                btn.disabled = r.isCurrent;
                btn.addEventListener('click', async () => {
                    this.setStatus(`Откат на ${r.version}…`);
                    try {
                        await installRelease(r.downloadUrl);
                    }
                    catch (e) {
                        this.setStatus('Ошибка отката: ' + e.message);
                    }
                });
                row.appendChild(btn);
                box.appendChild(row);
            }
            this.setStatus('');
        }
        catch (e) {
            this.setStatus('Ошибка: ' + e.message);
        }
        finally {
            this.busy = false;
        }
    }
}
if (!customElements.get('about-updates-panel')) {
    customElements.define('about-updates-panel', AboutUpdatesPanel);
}
