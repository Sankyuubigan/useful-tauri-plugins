import { ensureStarted, getCombos, getStatus, installOrUpdate, onProgress, onStatusChange, openDashboard, reinstall, ROUTER_IDS, setRouterDir, stop, } from './index';
import { checkUpdate, getUpdateState, onUpdateState, setUpdateState } from './updates';
import { onLoginState } from './login_events';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
const STYLE = `
  :host {
    --bg-base: #0c0e14;
    --bg-surface: #141721;
    --bg-surface-hover: #1b202e;
    --bg-card: rgba(22, 27, 38, 0.75);
    --border-subtle: rgba(255, 255, 255, 0.07);
    --border-strong: rgba(255, 255, 255, 0.14);
    
    --text-main: #f1f5f9;
    --text-muted: #94a3b8;
    --text-dim: #64748b;
    
    --accent: #6366f1;
    --accent-hover: #4f46e5;
    --accent-glow: rgba(99, 102, 241, 0.25);
    
    --success: #10b981;
    --success-bg: rgba(16, 185, 129, 0.12);
    --success-glow: rgba(16, 185, 129, 0.35);
    
    --warning: #f59e0b;
    --warning-bg: rgba(245, 158, 11, 0.12);
    
    --danger: #ef4444;
    --danger-bg: rgba(239, 68, 68, 0.12);

    display: block;
    color: var(--text-main);
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Inter', sans-serif;
    font-size: 13px;
    line-height: 1.5;
    box-sizing: border-box;
    padding: 16px;
    background: radial-gradient(circle at top right, rgba(99, 102, 241, 0.05), transparent 40%), var(--bg-base);
    border-radius: 12px;
  }

  *, *::before, *::after {
    box-sizing: border-box;
  }

  /* ── Табы роутеров ── */
  .tabs-nav {
    display: flex;
    gap: 6px;
    background: rgba(0, 0, 0, 0.25);
    padding: 4px;
    border-radius: 10px;
    border: 1px solid var(--border-subtle);
    margin-bottom: 16px;
    overflow-x: auto;
  }
  .tabs-nav button {
    flex: 1;
    min-width: 90px;
    background: transparent;
    border: 1px solid transparent;
    color: var(--text-muted);
    font-weight: 500;
    font-size: 13px;
    padding: 7px 14px;
    border-radius: 8px;
    cursor: pointer;
    transition: all 0.2s cubic-bezier(0.4, 0, 0.2, 1);
    white-space: nowrap;
    text-align: center;
  }
  .tabs-nav button:hover {
    color: var(--text-main);
    background: rgba(255, 255, 255, 0.04);
  }
  .tabs-nav button.active {
    background: var(--bg-surface);
    color: #fff;
    border-color: var(--border-strong);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3), 0 0 1px rgba(255, 255, 255, 0.2);
  }

  /* ── Карточка Статуса (Hero) ── */
  .status-hero {
    background: var(--bg-card);
    backdrop-filter: blur(12px);
    border: 1px solid var(--border-subtle);
    border-radius: 12px;
    padding: 16px;
    margin-bottom: 14px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.2);
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .status-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
  }
  .status-title-row {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .router-name {
    font-size: 17px;
    font-weight: 700;
    letter-spacing: -0.02em;
    color: #fff;
  }
  .status-pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    border-radius: 9999px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    border: 1px solid var(--border-subtle);
    background: var(--bg-surface);
  }
  .status-pill.online {
    color: var(--success);
    background: var(--success-bg);
    border-color: rgba(16, 185, 129, 0.25);
  }
  .status-pill.warn {
    color: var(--warning);
    background: var(--warning-bg);
    border-color: rgba(245, 158, 11, 0.25);
  }
  .status-pill.offline {
    color: var(--text-dim);
  }

  .pulse-dot {
    position: relative;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: currentColor;
  }
  .status-pill.online .pulse-dot::after {
    content: '';
    position: absolute;
    inset: -3px;
    border-radius: 50%;
    background: var(--success);
    opacity: 0.75;
    animation: ping 2s cubic-bezier(0, 0, 0.2, 1) infinite;
  }
  @keyframes ping {
    75%, 100% {
      transform: scale(2.2);
      opacity: 0;
    }
  }

  .status-message {
    font-size: 13px;
    color: var(--text-muted);
    display: flex;
    align-items: center;
    gap: 8px;
  }

  /* ── Сетка параметров (Grid Specs) ── */
  .specs-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(130px, 1fr));
    gap: 8px;
    margin-bottom: 14px;
  }
  .spec-item {
    background: var(--bg-surface);
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    padding: 8px 10px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .spec-label {
    font-size: 10.5px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-dim);
    font-weight: 600;
  }
  .spec-value {
    font-size: 12.5px;
    color: var(--text-main);
    font-weight: 500;
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  /* Копирование адреса Web UI: адрес живёт в обычном браузере, не только во
     вкладке приложения (в чужой программе вкладок может не быть). */
  .copy-url {
    padding: 3px 10px;
    border-radius: 6px;
    font-size: 11.5px;
    flex-shrink: 0;
  }

  /* ── Экшены и Кнопки ── */
  .actions-bar {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
    margin-bottom: 14px;
  }
  
  button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 7px 13px;
    cursor: pointer;
    border-radius: 7px;
    border: 1px solid var(--border-subtle);
    background: var(--bg-surface);
    color: var(--text-main);
    font-family: inherit;
    font-size: 12.5px;
    font-weight: 500;
    transition: all 0.15s ease;
    user-select: none;
  }
  button:hover:not(:disabled) {
    background: var(--bg-surface-hover);
    border-color: var(--border-strong);
    transform: translateY(-1px);
  }
  button:active:not(:disabled) {
    transform: translateY(0);
  }
  button:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  
  button.primary {
    background: linear-gradient(135deg, var(--accent), #4338ca);
    border-color: rgba(255, 255, 255, 0.15);
    color: #fff;
    box-shadow: 0 2px 10px var(--accent-glow);
  }
  button.primary:hover:not(:disabled) {
    background: linear-gradient(135deg, #4f46e5, #3730a3);
    box-shadow: 0 4px 14px var(--accent-glow);
  }
  
  button.start {
    background: linear-gradient(135deg, #10b981, #059669);
    border-color: rgba(255, 255, 255, 0.1);
    color: #fff;
    box-shadow: 0 2px 8px var(--success-glow);
  }
  button.start:hover:not(:disabled) {
    background: linear-gradient(135deg, #059669, #047857);
  }
  
  button.stop:hover:not(:disabled) {
    background: var(--danger-bg);
    border-color: rgba(239, 68, 68, 0.3);
    color: #fca5a5;
  }

  button.busy {
    position: relative;
    pointer-events: none;
    color: transparent !important;
  }
  button.busy svg { opacity: 0; }
  button.busy::after {
    content: '';
    position: absolute;
    width: 14px;
    height: 14px;
    border: 2px solid #ffffff;
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 0.7s linear infinite;
  }

  /* ── Предупреждения и Баннеры ── */
  .banner {
    padding: 10px 12px;
    border-radius: 8px;
    margin-bottom: 12px;
    font-size: 12px;
    line-height: 1.4;
    display: flex;
    align-items: flex-start;
    gap: 8px;
  }
  .banner-warn {
    background: var(--warning-bg);
    border: 1px solid rgba(245, 158, 11, 0.25);
    color: #fbbf24;
  }
  .banner-info {
    background: rgba(99, 102, 241, 0.08);
    border: 1px solid rgba(99, 102, 241, 0.2);
    color: #c7d2fe;
  }
  .banner code {
    background: rgba(0, 0, 0, 0.35);
    padding: 1px 5px;
    border-radius: 4px;
    font-family: ui-monospace, monospace;
    font-size: 11px;
    color: #fff;
  }

  /* ── Прогресс ── */
  .progress-container {
    display: none;
    background: var(--bg-surface);
    border: 1px solid var(--border-subtle);
    padding: 10px;
    border-radius: 8px;
    margin-bottom: 12px;
  }
  .progress-container.on { display: block; }
  .progress-status {
    color: var(--text-muted);
    font-size: 12px;
    margin-bottom: 6px;
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  .progress-track {
    height: 6px;
    border-radius: 6px;
    background: rgba(255, 255, 255, 0.06);
    overflow: hidden;
  }
  .progress-bar {
    height: 100%;
    width: 0%;
    background: linear-gradient(90deg, var(--accent), #a855f7);
    border-radius: 6px;
    box-shadow: 0 0 10px rgba(168, 85, 247, 0.5);
    transition: width 0.2s ease;
  }
  /* Этап идёт, но процент неизвестен (npm install, распаковка, postinstall).
     Бегущая полоса означает «работа идёт» и не выдаёт себя за завершение:
     заполненная полоса на этом этапе была прямой ложью в интерфейсе. */
  .progress-bar.indeterminate {
    width: 35%;
    transition: none;
    animation: progress-indeterminate 1.4s ease-in-out infinite;
  }
  @keyframes progress-indeterminate {
    0% { margin-left: -35%; }
    100% { margin-left: 100%; }
  }
  @media (prefers-reduced-motion: reduce) {
    .progress-bar.indeterminate { animation: none; width: 100%; opacity: 0.5; }
  }

  /* ── Футер с путями ── */
  .path-box {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 11px;
    color: var(--text-dim);
    padding: 6px 10px;
    background: rgba(0, 0, 0, 0.2);
    border-radius: 6px;
    border: 1px solid rgba(255, 255, 255, 0.03);
    word-break: break-all;
    font-family: ui-monospace, monospace;
  }

  .spinner {
    display: inline-block;
    width: 12px;
    height: 12px;
    border: 2px solid var(--accent);
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin { to { transform: rotate(360deg); } }

  .update-badge {
    background: var(--success);
    color: #000;
    font-size: 10px;
    font-weight: 700;
    padding: 2px 6px;
    border-radius: 999px;
    animation: pulse-update 1.5s infinite;
  }
  @keyframes pulse-update {
    0%, 100% { opacity: 1; transform: scale(1); }
    50% { opacity: 0.8; transform: scale(0.96); }
  }
`;
function esc(s) {
    return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}
function toast(msg, kind = 'success') {
    const el = document.createElement('div');
    el.textContent = msg;
    el.style.cssText =
        `position:fixed; right:20px; bottom:20px; z-index:9999; max-width:400px; padding:10px 16px;` +
            `border-radius:10px; font: 500 13px system-ui, sans-serif; backdrop-filter: blur(12px);` +
            `box-shadow:0 12px 30px rgba(0,0,0,.5), 0 0 0 1px rgba(255,255,255,0.1);` +
            `background:${kind === 'success' ? 'rgba(16, 185, 129, 0.95)' : 'rgba(239, 68, 68, 0.95)'}; color:#fff;` +
            `animation: toast-in 0.2s cubic-bezier(0.16, 1, 0.3, 1);`;
    document.body.appendChild(el);
    setTimeout(() => {
        el.style.opacity = '0';
        el.style.transition = 'opacity 0.25s ease';
        setTimeout(() => el.remove(), 250);
    }, 3200);
}
function logPlugin(msg) {
    void invoke('plugin:logs|log_frontend_event', { level: 'FE', msg }).catch(() => { });
}
const ROUTER_LABELS = {
    '9router': '9Router',
    extremerouter: 'ExtremeRouter',
    omniroute: 'OmniRoute',
    gateway: 'Gateway',
};
const NATIVE_GATEWAY = 'gateway';
const isNativeGateway = (r) => r === NATIVE_GATEWAY;
export class CloudRoutersPanel extends HTMLElement {
    constructor() {
        super();
        this.router = '9router';
        this.states = new Map();
        this.combosByRouter = new Map();
        this.offProgress = null;
        this.offStatus = null;
        this.visibilityObserver = null;
        this.refreshTimer = null;
        /**
         * Роутеры, для которых установка/переустановка ВЫПОЛНЯЕТСЯ прямо сейчас.
         * Почему нельзя полагаться на backend-статус: во время установки статусы
         * честно сообщают «неполная установка» (node.exe уже есть, tgz ещё качается),
         * и каждые 15 секунд refresh рисовал бы предупреждение о поломке, хотя
         * установка в разгаре.
         */
        this.installing = new Set();
        /**
         * Последний прогресс установки по роутерам. Хранится в state, потому что
         * periodic render() пересоздаёт DOM, а `onProgress` мутирует DOM императивно —
         * без этого долгие стадии без событий (распаковка, npm, postinstall)
         * «сбрасывали» бы полосу прогресса между событиями. `total < 0` — ошибка.
         */
        this.progressByRouter = new Map();
        /** Роутер, у которого «Переустановить» ожидает второй клик-подтверждения. */
        this.reinstallArm = null;
        this.reinstallArmTimer = null;
        /**
         * Слушатель postMessage от дашборда шлюза. Дашборд (в iframe или отдельной
         * вкладке) после изменения конфигурации шлёт `{source: 'cloud-routers-gateway',
         * type: 'config-changed'}` родительскому окну; панель перечитывает комбо и
         * рассылает существующее `cloud-routers:combos-changed`, чтобы дропдаун
         * моделей чата обновился без переоткрытия настроек.
         */
        this.hostMsgListener = null;
        this.root = this.attachShadow({ mode: 'open' });
    }
    stateOf(router) {
        return this.states.get(router) ?? { kind: 'loading' };
    }
    get status() {
        const s = this.stateOf(this.router);
        return s.kind === 'ready' ? s.status : null;
    }
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
                this.onProgress(p.router, p.text, p.done, p.total);
        }).then((off) => {
            this.offProgress = off;
        });
        this.observeVisibility();
        this.refreshTimer = window.setInterval(() => void this.refresh(), 15000);
        // События жизни шлюза от супервизора (тик 3 с): смерть/подъём отражаются
        // на пилюле немедленно, а не через 15-с опрос — 15-с интервал остаётся
        // фолбэком (источник правды один: getStatus с живой пробой порта).
        onStatusChange((p) => {
            if (p.router === this.router)
                void this.refresh(p.router);
        }).then((off) => {
            this.offStatus = off;
        });
        // Мост от дашборда шлюза: правки комбо в его вкладке «Комбо» доезжают в
        // чат без переоткрытия настроек (снятие слушателя — в disconnectedCallback).
        this.hostMsgListener = (e) => void this.onHostMessage(e);
        window.addEventListener('message', this.hostMsgListener);
    }
    /**
     * Сообщение от дашборда шлюза: перечитать комбо и статус активного роутера
     * и разослать существующее событие combos-changed. Фильтр по `source`
     * обязателен: postMessage получают все слушатели окна, и чужие сообщения
     * (расширения, другие страницы) не должны дёргать перечитывание.
     */
    async onHostMessage(e) {
        const data = e.data;
        if (!data || data.source !== 'cloud-routers-gateway' || data.type !== 'config-changed')
            return;
        const router = this.router;
        this.setCombos(router, await getCombos(router).catch(() => []));
        const fresh = await getStatus(router).catch(() => null);
        if (fresh)
            this.setStatus(router, fresh);
        this.notifyCombosChanged(router);
        this.render();
    }
    disconnectedCallback() {
        this.unsubUpdate?.();
        this.offProgress?.();
        this.offProgress = null;
        this.offStatus?.();
        this.offStatus = null;
        this.visibilityObserver?.disconnect();
        this.visibilityObserver = null;
        if (this.hostMsgListener !== null) {
            window.removeEventListener('message', this.hostMsgListener);
            this.hostMsgListener = null;
        }
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
    onProgress(router, text, done, total) {
        // Прогресс сохраняется в state ДО DOM-правок: render() (каждые 15 секунд,
        // а также любые setStatus) пересоздаёт DOM, и без этого слепка полоса
        // прогресса слетала бы посреди установки — на долгих стадиях без событий
        // это выглядело как «крутилка закончилась, хотя файлы ещё качаются».
        this.progressByRouter.set(router, { text, done, total });
        this.applyProgress(router);
    }
    /**
     * Восстановить прогресс-бар из сохранённого state (после render()).
     * Императивные DOM-правки имеет право делать ТОЛЬКО этот метод и
     * `applyProgress`: оба синхронизированы с `progressByRouter`, поэтому
     * перерисовка не может потерять текущий ход установки.
     */
    applyProgress(router) {
        const entry = this.progressByRouter.get(router);
        if (!entry || router !== this.router)
            return;
        const bar = this.root.querySelector('.progress-bar');
        const label = this.root.querySelector('.progress-status');
        const box = this.root.querySelector('.progress-container');
        if (!box || !bar || !label)
            return;
        box.classList.add('on');
        if (entry.total < 0) {
            // Ошибка установки: текст без процентной части; total < 0 означает
            // «это не прогресс», полоса не анимируется.
            bar.classList.remove('indeterminate');
            bar.style.width = '0%';
            label.textContent = entry.text;
            return;
        }
        // `total === 0` означает «процент НЕИЗВЕСТЕН», а не «всё готово».
        // Раньше здесь ставилась ширина 100%, и каждый этап без измеримого
        // прогресса (скачивание Node, распаковка, npm install, postinstall)
        // выглядел как завершённый: полоса прыгала в конец, а установка
        // продолжалась ещё минуту. Неизвестный процент показывается бегущей
        // полосой — она и означает «идёт, жди».
        const indeterminate = entry.total <= 0;
        bar.classList.toggle('indeterminate', indeterminate);
        if (!indeterminate) {
            bar.style.width = `${Math.min(100, (entry.done / entry.total) * 100).toFixed(1)}%`;
        }
        const pct = indeterminate ? '…' : `${Math.min(100, (entry.done / entry.total) * 100).toFixed(0)}%`;
        label.innerHTML = `<span>${esc(entry.text)}</span><span>${pct}</span>`;
        // Завершённость НЕ выводится из арифметики прогресса. Прогресс — это
        // сообщение о ходе работы, а вердикт «установлено» приходит из возврата
        // команды, который разбирает `onInstall`. Раньше условие
        // `done >= total` срабатывало посреди установки (на попадании в кэш tgz)
        // и объявляло «установка завершена» за минуту до реального конца.
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
    async onInstall(force = true) {
        const router = this.router;
        this.setBtnBusy('.install', true);
        this.setBtnBusy('.install-update', true);
        try {
            this.setStatus(router, await installOrUpdate(router, force));
            this.setCombos(router, await getCombos(router).catch(() => []));
            this.progressByRouter.delete(router);
        }
        catch (e) {
            this.reportInstallError(router, 'установки', String(e));
            // Статус перечитываем ДАЖЕ при ошибке: он несёт failed_update — плашку
            // «обновление не удалось, работает прежняя версия». Без этого панель
            // осталась бы в состоянии, неотличимом от «обновлений не было».
            // null здесь означает «перечитать не удалось» — тогда оставляем прежнее
            // состояние, а не подменяем его пустым.
            const fresh = await getStatus(router).catch(() => null);
            if (fresh)
                this.setStatus(router, fresh);
            // Перерисовка обязательна и в ветке ошибки: плашка «обновление не
            // удалось» появляется именно из свежего статуса. Без неё данные
            // обновились бы молча и пользователь увидел бы прежнюю картину —
            // то есть «ничего не изменилось».
            this.render();
            throw e;
        }
        finally {
            this.setBtnBusy('.install', false);
            this.setBtnBusy('.install-update', false);
        }
        this.notifyCombosChanged(router);
        this.render();
    }
    /**
     * Ошибка установки/переустановки: тост + лог + строка прогресса превращается
     * в ошибку. Ошибка сохраняется в `progressByRouter`, а не только в DOM:
     * последующие перерисовки (refresh каждые 15с) не должны её терять.
     */
    reportInstallError(router, actionLabel, errorText) {
        const msg = `Ошибка ${actionLabel}: ${errorText}`;
        logPlugin(`[cloud-routers] ${msg}`);
        toast(msg, 'error');
        this.progressByRouter.set(router, { text: `Ошибка: ${errorText}`, done: 0, total: -1 });
        const label = this.root.querySelector('.progress-status');
        const box = this.root.querySelector('.progress-container');
        if (box)
            box.classList.add('on');
        if (label)
            label.textContent = `Ошибка: ${errorText}`;
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
    async onStart() {
        const router = this.router;
        this.setBtnBusy('.start', true);
        try {
            const status = await ensureStarted(router);
            this.setStatus(router, status);
            this.setCombos(router, await getCombos(router).catch(() => []));
        }
        catch (e) {
            const msg = `Ошибка запуска: ${String(e)}`;
            logPlugin(`[cloud-routers] ${msg}`);
            toast(msg, 'error');
        }
        finally {
            this.setBtnBusy('.start', false);
        }
        this.notifyCombosChanged(router);
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
        this.renderUpdateState({ hasUpdate: false });
        try {
            const next = await checkUpdate(router);
            if (box)
                box.classList.add('on');
            if (label) {
                label.textContent = next.hasUpdate
                    ? `🔥 Доступно обновление: v${next.tag ?? ''}`
                    : `Установлена актуальная версия ${this.status?.version ? `(v${this.status.version})` : ''}`;
            }
        }
        catch (e) {
            const msg = `Ошибка проверки обновления: ${String(e)}`;
            logPlugin(`[cloud-routers] ${msg}`);
            toast(msg, 'error');
            if (box)
                box.classList.add('on');
            if (label)
                label.textContent = `Ошибка проверки: ${String(e)}`;
        }
        finally {
            if (btn)
                btn.disabled = false;
        }
    }
    /** Скопировать адрес Web UI: юзер открывает дашборд в обычном браузере, а не
     * только вкладкой приложения. Адрес — из статуса роутера
     * (`RouterStatus.base_url`), доп. источников нет (SSOT). */
    async onCopyUrl() {
        const url = this.status?.base_url;
        if (!url)
            return;
        try {
            await navigator.clipboard.writeText(url);
            toast('Адрес Web UI скопирован в буфер', 'success');
            logPlugin(`[cloud-routers] адрес скопирован: ${url}`);
        }
        catch (e) {
            const msg = `Не удалось скопировать адрес: ${String(e)}`;
            logPlugin(`[cloud-routers] ${msg}`);
            toast(msg, 'error');
        }
    }
    async onInstallUpdate() {
        const router = this.router;
        try {
            // force=false: обновление — это обновление. force=true звал полную
            // переустановку (заново Node + снос dist + npm с нуля) там, где хватило
            // обновить версию, и стоил минут вместо секунд.
            await this.onInstall(false);
        }
        catch {
            // Значок обновления НЕ снимаем: попытка не удалась, значит версия,
            // которую предлагали поставить, всё ещё доступна. Снимать его здесь
            // было прямой ложью — «обновлений нет» после неудачного обновления.
            return;
        }
        setUpdateState(router, { hasUpdate: false });
    }
    /**
     * Снять «armed»-состояние кнопки «Переустановить» (таймер отката).
     * Вызывается при втором клике, смене вкладки и истечении окна подтверждения.
     */
    disarmReinstallArm() {
        if (this.reinstallArmTimer !== null) {
            window.clearTimeout(this.reinstallArmTimer);
            this.reinstallArmTimer = null;
        }
        this.reinstallArm = null;
    }
    /**
     * Кнопка «Переустановить»: остановить сервер, снести установку и данные
     * (настройки дашборда, включая пароль; комбо) и поставить последнюю версию
     * заново. Двойное подтверждение вместо `confirm()`: первый клик поднимает
     * предупреждение с 6-секундным окном, второй клик в этом окне — запуск.
     */
    async onReinstall() {
        const router = this.router;
        if (this.installing.has(router))
            return;
        // Первый клик: предупреждение вместо немедленного уничтожения данных.
        // Нажать «Переустановить» второй раз сокрее случайно нельзя — окно всего
        // 6 секунд, и первый клик обязан увидеть предупреждение.
        if (this.reinstallArm !== router) {
            this.reinstallArm = router;
            const label = this.root.querySelector('.progress-status');
            const box = this.root.querySelector('.progress-container');
            if (box)
                box.classList.add('on');
            if (label) {
                label.innerHTML = `<span>Переустановка удалит установку и данные роутера (настройки и пароль дашборда, комбо). Нажмите <b>«Переустановить»</b> ещё раз в течение 6 секунд.</span><span></span>`;
            }
            if (this.reinstallArmTimer !== null)
                window.clearTimeout(this.reinstallArmTimer);
            this.reinstallArmTimer = window.setTimeout(() => {
                this.disarmReinstallArm();
                this.render();
            }, 6000);
            return;
        }
        this.disarmReinstallArm();
        this.installing.add(router);
        this.setBtnBusy('.reinstall', true);
        try {
            this.setStatus(router, await reinstall(router));
            this.setCombos(router, await getCombos(router).catch(() => []));
            this.progressByRouter.delete(router);
        }
        catch (e) {
            this.reportInstallError(router, 'переустановки', String(e));
            // Статус перечитываем и при ошибке: он честно показывает, чем всё
            // кончилось (например, «не установлен» после сноса папок).
            const fresh = await getStatus(router).catch(() => null);
            if (fresh)
                this.setStatus(router, fresh);
        }
        finally {
            this.installing.delete(router);
            this.setBtnBusy('.reinstall', false);
        }
        this.notifyCombosChanged(router);
        this.render();
    }
    switchRouter(router) {
        if (router === this.router)
            return;
        this.disarmReinstallArm();
        this.router = router;
        this.render();
        void this.refresh(router);
    }
    render() {
        const state = this.stateOf(this.router);
        const known = state.kind === 'ready';
        const s = this.status;
        const installed = s?.installed ?? false;
        const running = s?.running ?? false;
        const native = isNativeGateway(this.router);
        // Pill UI статуса
        let statusPill = `<span class="status-pill offline"><span class="pulse-dot"></span> Остановлен</span>`;
        if (!known) {
            statusPill = `<span class="status-pill"><span class="spinner"></span> Проверка</span>`;
        }
        else if (installed && running) {
            statusPill = `<span class="status-pill online"><span class="pulse-dot"></span> В сети</span>`;
        }
        else if (installed && !running) {
            statusPill = `<span class="status-pill warn"><span class="pulse-dot"></span> Готов к запуску</span>`;
        }
        const message = known
            ? s.message
            : state.kind === 'error'
                ? state.message
                : 'Синхронизация состояния шлюза…';
        // Плашка неудачного обновления. Показывается по данным backend
        // (`failed_update`), а не по локальному флагу: сбой мог произойти вчера,
        // в другом окне или до перезапуска приложения — локальный флаг этого не
        // знает, а пользователь должен. Текст прямо говорит, что рабочая версия
        // цела: обновление транзакционное и не сносит установленную версию.
        const failedUpdate = s?.failed_update;
        const failedBanner = failedUpdate
            ? `
        <div class="banner banner-warn">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="flex-shrink:0;margin-top:2px;"><path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3Z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
          <div>
            Обновление${failedUpdate.target_version ? ` до <b>v${esc(failedUpdate.target_version)}</b>` : ''} не удалось —
            действующая версия не тронута и продолжает работать.
            <div style="margin-top:6px;white-space:pre-wrap;font-family:ui-monospace,monospace;font-size:11px;opacity:.85;">${esc(failedUpdate.reason)}</div>
            <div style="margin-top:6px;">Нажмите <b>«Обновить сейчас»</b>, чтобы повторить попытку.</div>
          </div>
        </div>`
            : '';
        const version = s?.version ? `v${s.version}` : '—';
        const port = s?.port ? `${s.port}` : '—';
        const dbStatus = s?.has_npm_runtime ? (s?.db_present ? 'Подключена' : 'Не найдена') : 'N/A';
        // Генерация кнопок действий
        const actions = known
            ? `
        ${s?.can_install ? `<button class="primary install" style="${installed ? 'display:none;' : ''}">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
          Установить
        </button>` : ''}

        ${!running ? `<button class="start" ${installed ? '' : 'disabled'}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><polygon points="5 3 19 12 5 21 5 3"/></svg>
          Запустить
        </button>` : ''}

        <button class="stop" ${running ? '' : 'disabled'}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><rect x="4" y="4" width="16" height="16" rx="2"/></svg>
          Остановить
        </button>

        <button class="open" ${installed ? '' : 'disabled'}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></svg>
          Web UI
        </button>

        ${s?.can_check_update ? `
          <button class="check-update" style="${installed ? '' : 'display:none;'}">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21.5 2v6h-6M21.34 15.57a10 10 0 1 1-.57-8.38l5.67-5.67"/></svg>
            Обновления
          </button>
        ` : ''}

        <button class="primary install-update" style="display:none;">Обновить сейчас</button>

        <button class="setdir" title="Изменить директорию установки">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
          Путь
        </button>

        ${!native && installed ? `
          <button class="reinstall" ${this.installing.has(this.router) ? 'disabled' : ''} title="Снести установку и данные роутера (настройки и пароль дашборда, комбо) и поставить заново">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3Z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
            Переустановить
          </button>
        ` : ''}
      `
            : `<span style="color:var(--text-dim)"><span class="spinner"></span> Загрузка действий...</span>`;
        this.root.innerHTML = `
      <style>${STYLE}</style>

      <!-- Табы -->
      <div class="tabs-nav">
        ${ROUTER_IDS.map((id) => `<button class="${this.router === id ? 'active' : ''}" data-router="${id}">${ROUTER_LABELS[id]}</button>`).join('')}
      </div>

      <!-- Главная статус-карточка -->
      <div class="status-hero">
        <div class="status-header">
          <div class="status-title-row">
            <span class="router-name">${ROUTER_LABELS[this.router]}</span>
            ${statusPill}
            <span id="updateBadge" class="update-badge" style="display:none;">UPDATE</span>
          </div>
          <span style="font-family:ui-monospace, monospace; font-size:12px; color:var(--text-dim);">${version}</span>
        </div>
        <div class="status-message">
          <span>${message}</span>
        </div>
      </div>

      <!-- Сетка параметров -->
      <div class="specs-grid">
        <div class="spec-item">
          <span class="spec-label">Порт</span>
          <span class="spec-value" style="color: #38bdf8;">${port}</span>
        </div>
        ${s?.base_url ? `
          <!-- Адрес Web UI: юзер открывает дашборд в обычном браузере, копируя
               его отсюда (правило SSOT: адрес — только из RouterStatus.base_url,
               никаких вторых источников). -->
          <div class="spec-item" style="grid-column: span 2;">
            <span class="spec-label">Адрес</span>
            <span class="spec-value" style="display:flex; align-items:center; gap:8px; min-width:0;">
              <span style="overflow:hidden; text-overflow:ellipsis; white-space:nowrap;" title="${esc(s.base_url)}">${esc(s.base_url)}</span>
              <button class="copy-url" title="Скопировать адрес, чтобы открыть дашборд в обычном браузере">Скопировать</button>
            </span>
          </div>
        ` : ''}
        ${s?.has_npm_runtime ? `
          <div class="spec-item">
            <span class="spec-label">Runtime</span>
            <span class="spec-value">${s?.node_version ? `Node ${s.node_version}` : '—'}</span>
          </div>
          <div class="spec-item">
            <span class="spec-label">База данных</span>
            <span class="spec-value" style="color:${s?.db_present ? 'var(--success)' : 'var(--warning)'};">${dbStatus}</span>
          </div>
        ` : ''}
        <div class="spec-item" style="grid-column: span 2;">
          <span class="spec-label">Каталог данных</span>
          <span class="spec-value" title="${s?.data_dir ?? ''}">${s?.data_dir ? esc(s.data_dir) : 'Не задан'}</span>
        </div>
      </div>

      <!-- Тулбар действий -->
      <div class="actions-bar">
        ${actions}
      </div>

      <!-- Уведомления/Баннеры -->
      ${failedBanner}

      ${native ? `
        <div class="banner banner-info">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="flex-shrink:0;margin-top:2px;"><circle cx="12" cy="12" r="10"/><line x1="12" y1="16" x2="12" y2="12"/><line x1="12" y1="8" x2="12.01" y2="8"/></svg>
          <div>
            Шлюз настраивается в своём UI. Жми <b>«Web UI»</b> — дашборд откроется вкладкой прямо в приложении.
          </div>
        </div>
      ` : ''}

      ${!installed && !native && (s?.node_present || s?.server_present) && !this.installing.has(this.router) ? `
        <!-- Плашка показывается ТОЛЬКО когда установка не идёт: во время
             установки статус честно «не установлен» (node есть, tgz ещё
             качается), и предупреждение о поломке здесь ложь. -->
        <div class="banner banner-warn">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="flex-shrink:0;margin-top:2px;"><path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3Z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
          <div>Неполная установка: отсутствуют компоненты пакета. Нажмите <b>«Установить»</b>, чтобы восстановить целостность.</div>
        </div>
      ` : ''}

      <!-- Прогресс установки / загрузки -->
      <div class="progress-container">
        <div class="progress-status"><span>Подготовка...</span><span>0%</span></div>
        <div class="progress-track"><div class="progress-bar"></div></div>
      </div>

      <!-- Путь к бинарнику/установке -->
      <div class="path-box" title="${s?.path ? esc(s.path) : ''}">
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/></svg>
        <span>Путь: ${s?.path ? esc(s.path) : 'Не определен'}</span>
      </div>
    `;
        // Слушатели событий
        this.root.querySelectorAll('.tabs-nav button').forEach((b) => {
            b.addEventListener('click', () => this.switchRouter(b.dataset.router));
        });
        this.root.querySelector('.install')?.addEventListener('click', () => {
            // Ошибка уже показана тостом и в строке прогресса; промис заново бросает
            // её наружу, чтобы вызывающий знал исход — здесь глотаем осознанно,
            // иначе в консоли браузера остаётся необработанный rejection.
            void this.onInstall().catch(() => { });
        });
        this.root.querySelector('.check-update')?.addEventListener('click', () => void this.onCheckUpdate());
        this.root.querySelector('.install-update')?.addEventListener('click', () => void this.onInstallUpdate());
        this.root.querySelector('.copy-url')?.addEventListener('click', () => void this.onCopyUrl());
        this.root.querySelector('.start')?.addEventListener('click', () => void this.onStart());
        this.root.querySelector('.stop')?.addEventListener('click', () => void this.onStop());
        this.root.querySelector('.open')?.addEventListener('click', () => void this.onOpen());
        this.root.querySelector('.setdir')?.addEventListener('click', () => void this.onSetDir());
        this.root.querySelector('.reinstall')?.addEventListener('click', () => void this.onReinstall());
        this.renderUpdateState(getUpdateState(this.router));
        // Прогресс установки восстанавливаем ПОСЛЕ пересоздания DOM: иначе каждые
        // 15 секунд (refresh → render) полоса прогресса сбрасывалась бы, и на
        // долгих стадиях без событий (распаковка, npm, postinstall) установка
        // выглядела бы завершённой или умершей.
        this.applyProgress(this.router);
    }
    renderUpdateState(s) {
        const badge = this.root.querySelector('#updateBadge');
        const updateBtn = this.root.querySelector('.install-update');
        const display = s.hasUpdate ? 'inline-flex' : 'none';
        if (badge)
            badge.style.display = display;
        if (updateBtn)
            updateBtn.style.display = display;
    }
}
if (!customElements.get('cloud-routers-panel')) {
    customElements.define('cloud-routers-panel', CloudRoutersPanel);
}
// Состояние экрана логина: слушатель уровня модуля пакета (не компонента) —
// тост честный и когда панель настроек закрыта. Успех молчит: выключенный
// экран логина остаётся выключенным, тосты на каждый старт — шум.
void onLoginState((p) => {
    logPlugin(`no_login: ${p.router} — ${p.detail}`);
    toast(p.detail, 'error');
}).catch(() => { });
