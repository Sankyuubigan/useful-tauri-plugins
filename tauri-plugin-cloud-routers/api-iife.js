// Generated from guest-js/ by esbuild (npm run build:global). Do not edit by hand.
"use strict";
(() => {
  // guest-js/shims/core.ts
  var g = window;
  function coreInvoke(...args) {
    const core = g.__TAURI__?.core;
    if (core?.invoke) return core.invoke(...args);
    const internals = g.__TAURI_INTERNALS__;
    if (internals?.invoke) return internals.invoke(...args);
    return Promise.reject(new Error("window.__TAURI__.core.invoke \u043D\u0435\u0434\u043E\u0441\u0442\u0443\u043F\u0435\u043D"));
  }
  function invoke(cmd, args) {
    return coreInvoke(cmd, args ?? {});
  }

  // guest-js/shims/event.ts
  var g2 = window;
  async function listen(name, cb) {
    const event = g2.__TAURI__?.event ?? g2.__TAURI_INTERNALS__?.event;
    if (!event?.listen) throw new Error("window.__TAURI__.event.listen \u043D\u0435\u0434\u043E\u0441\u0442\u0443\u043F\u0435\u043D");
    return event.listen(name, (e) => cb(e));
  }

  // guest-js/login_events.ts
  var LOGIN_STATE_EVENT = "cloud-routers-login-state";
  function onLoginState(cb) {
    return listen(LOGIN_STATE_EVENT, (e) => cb(e.payload));
  }

  // guest-js/updates.ts
  var states = /* @__PURE__ */ new Map();
  var subs = /* @__PURE__ */ new Set();
  var pending = /* @__PURE__ */ new Set();
  function getUpdateState(router) {
    if (router) return states.get(router) ?? { hasUpdate: false };
    return { hasUpdate: Array.from(states.values()).some((s) => s.hasUpdate) };
  }
  function onUpdateState(fn) {
    subs.add(fn);
    fn(getUpdateState());
    return () => {
      subs.delete(fn);
    };
  }
  function setUpdateState(router, next) {
    states.set(router, next);
    const aggregate = getUpdateState();
    subs.forEach((fn) => fn(aggregate));
  }
  async function checkUpdate(router) {
    if (pending.has(router)) return getUpdateState(router);
    pending.add(router);
    try {
      const tag = await checkRouterUpdate(router);
      const next = tag ? { hasUpdate: true, tag } : { hasUpdate: false };
      setUpdateState(router, next);
      return next;
    } finally {
      pending.delete(router);
    }
  }

  // guest-js/shims/dialog.ts
  var g3 = window;
  async function open(opts) {
    const dialog = g3.__TAURI__?.dialog;
    if (!dialog?.open) throw new Error("window.__TAURI__.dialog.open \u043D\u0435\u0434\u043E\u0441\u0442\u0443\u043F\u0435\u043D");
    return dialog.open(opts);
  }

  // guest-js/web-components.ts
  var STYLE = `
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

  /* \u2500\u2500 \u0422\u0430\u0431\u044B \u0440\u043E\u0443\u0442\u0435\u0440\u043E\u0432 \u2500\u2500 */
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

  /* \u2500\u2500 \u041A\u0430\u0440\u0442\u043E\u0447\u043A\u0430 \u0421\u0442\u0430\u0442\u0443\u0441\u0430 (Hero) \u2500\u2500 */
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

  /* \u2500\u2500 \u0421\u0435\u0442\u043A\u0430 \u043F\u0430\u0440\u0430\u043C\u0435\u0442\u0440\u043E\u0432 (Grid Specs) \u2500\u2500 */
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
  /* \u041A\u043E\u043F\u0438\u0440\u043E\u0432\u0430\u043D\u0438\u0435 \u0430\u0434\u0440\u0435\u0441\u0430 Web UI: \u0430\u0434\u0440\u0435\u0441 \u0436\u0438\u0432\u0451\u0442 \u0432 \u043E\u0431\u044B\u0447\u043D\u043E\u043C \u0431\u0440\u0430\u0443\u0437\u0435\u0440\u0435, \u043D\u0435 \u0442\u043E\u043B\u044C\u043A\u043E \u0432\u043E
     \u0432\u043A\u043B\u0430\u0434\u043A\u0435 \u043F\u0440\u0438\u043B\u043E\u0436\u0435\u043D\u0438\u044F (\u0432 \u0447\u0443\u0436\u043E\u0439 \u043F\u0440\u043E\u0433\u0440\u0430\u043C\u043C\u0435 \u0432\u043A\u043B\u0430\u0434\u043E\u043A \u043C\u043E\u0436\u0435\u0442 \u043D\u0435 \u0431\u044B\u0442\u044C). */
  .copy-url {
    padding: 3px 10px;
    border-radius: 6px;
    font-size: 11.5px;
    flex-shrink: 0;
  }

  /* \u2500\u2500 \u042D\u043A\u0448\u0435\u043D\u044B \u0438 \u041A\u043D\u043E\u043F\u043A\u0438 \u2500\u2500 */
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

  /* \u2500\u2500 \u041F\u0440\u0435\u0434\u0443\u043F\u0440\u0435\u0436\u0434\u0435\u043D\u0438\u044F \u0438 \u0411\u0430\u043D\u043D\u0435\u0440\u044B \u2500\u2500 */
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

  /* \u2500\u2500 \u041F\u0440\u043E\u0433\u0440\u0435\u0441\u0441 \u2500\u2500 */
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
  /* \u042D\u0442\u0430\u043F \u0438\u0434\u0451\u0442, \u043D\u043E \u043F\u0440\u043E\u0446\u0435\u043D\u0442 \u043D\u0435\u0438\u0437\u0432\u0435\u0441\u0442\u0435\u043D (npm install, \u0440\u0430\u0441\u043F\u0430\u043A\u043E\u0432\u043A\u0430, postinstall).
     \u0411\u0435\u0433\u0443\u0449\u0430\u044F \u043F\u043E\u043B\u043E\u0441\u0430 \u043E\u0437\u043D\u0430\u0447\u0430\u0435\u0442 \xAB\u0440\u0430\u0431\u043E\u0442\u0430 \u0438\u0434\u0451\u0442\xBB \u0438 \u043D\u0435 \u0432\u044B\u0434\u0430\u0451\u0442 \u0441\u0435\u0431\u044F \u0437\u0430 \u0437\u0430\u0432\u0435\u0440\u0448\u0435\u043D\u0438\u0435:
     \u0437\u0430\u043F\u043E\u043B\u043D\u0435\u043D\u043D\u0430\u044F \u043F\u043E\u043B\u043E\u0441\u0430 \u043D\u0430 \u044D\u0442\u043E\u043C \u044D\u0442\u0430\u043F\u0435 \u0431\u044B\u043B\u0430 \u043F\u0440\u044F\u043C\u043E\u0439 \u043B\u043E\u0436\u044C\u044E \u0432 \u0438\u043D\u0442\u0435\u0440\u0444\u0435\u0439\u0441\u0435. */
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

  /* \u2500\u2500 \u041A\u043E\u043C\u0431\u043E / \u0414\u043E\u0441\u0442\u0443\u043F\u043D\u044B\u0435 \u0440\u043E\u0443\u0442\u044B \u2500\u2500 */
  .combos-wrapper {
    display: none;
    background: var(--bg-card);
    border: 1px solid var(--border-subtle);
    border-radius: 10px;
    padding: 12px;
    margin-bottom: 14px;
  }
  .combos-wrapper.on { display: block; }
  .combos-title {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    font-weight: 700;
    color: var(--text-dim);
    margin-bottom: 8px;
  }
  .combo-item {
    padding: 8px 0;
    border-bottom: 1px solid var(--border-subtle);
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .combo-item:last-child { border-bottom: 0; padding-bottom: 0; }
  .combo-header {
    font-weight: 600;
    color: #fff;
    font-size: 13px;
  }
  .combo-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .model-chip {
    font-family: ui-monospace, monospace;
    font-size: 11px;
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid var(--border-subtle);
    color: var(--text-muted);
    padding: 2px 6px;
    border-radius: 4px;
  }

  /* \u2500\u2500 \u0424\u0443\u0442\u0435\u0440 \u0441 \u043F\u0443\u0442\u044F\u043C\u0438 \u2500\u2500 */
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
    return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
  }
  function toast(msg, kind = "success") {
    const el = document.createElement("div");
    el.textContent = msg;
    el.style.cssText = `position:fixed; right:20px; bottom:20px; z-index:9999; max-width:400px; padding:10px 16px;border-radius:10px; font: 500 13px system-ui, sans-serif; backdrop-filter: blur(12px);box-shadow:0 12px 30px rgba(0,0,0,.5), 0 0 0 1px rgba(255,255,255,0.1);background:${kind === "success" ? "rgba(16, 185, 129, 0.95)" : "rgba(239, 68, 68, 0.95)"}; color:#fff;animation: toast-in 0.2s cubic-bezier(0.16, 1, 0.3, 1);`;
    document.body.appendChild(el);
    setTimeout(() => {
      el.style.opacity = "0";
      el.style.transition = "opacity 0.25s ease";
      setTimeout(() => el.remove(), 250);
    }, 3200);
  }
  function logPlugin(msg) {
    void invoke("plugin:logs|log_frontend_event", { level: "FE", msg }).catch(() => {
    });
  }
  var ROUTER_LABELS = {
    "9router": "9Router",
    extremerouter: "ExtremeRouter",
    omniroute: "OmniRoute",
    gateway: "Gateway"
  };
  var NATIVE_GATEWAY = "gateway";
  var isNativeGateway = (r) => r === NATIVE_GATEWAY;
  var CloudRoutersPanel = class extends HTMLElement {
    constructor() {
      super();
      this.router = "9router";
      this.states = /* @__PURE__ */ new Map();
      this.combosByRouter = /* @__PURE__ */ new Map();
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
      this.installing = /* @__PURE__ */ new Set();
      /**
       * Последний прогресс установки по роутерам. Хранится в state, потому что
       * periodic render() пересоздаёт DOM, а `onProgress` мутирует DOM императивно —
       * без этого долгие стадии без событий (распаковка, npm, postinstall)
       * «сбрасывали» бы полосу прогресса между событиями. `total < 0` — ошибка.
       */
      this.progressByRouter = /* @__PURE__ */ new Map();
      /** Роутер, у которого «Переустановить» ожидает второй клик-подтверждения. */
      this.reinstallArm = null;
      this.reinstallArmTimer = null;
      this.root = this.attachShadow({ mode: "open" });
    }
    stateOf(router) {
      return this.states.get(router) ?? { kind: "loading" };
    }
    get status() {
      const s = this.stateOf(this.router);
      return s.kind === "ready" ? s.status : null;
    }
    get combos() {
      return this.combosByRouter.get(this.router) ?? [];
    }
    setStatus(router, status) {
      this.states.set(router, { kind: "ready", status });
    }
    setCombos(router, combos) {
      this.combosByRouter.set(router, combos);
    }
    connectedCallback() {
      this.render();
      this.unsubUpdate = onUpdateState(() => this.renderUpdateState(getUpdateState(this.router)));
      void this.refresh();
      onProgress((p) => {
        if (p.router === this.router) this.onProgress(p.router, p.text, p.done, p.total);
      }).then((off) => {
        this.offProgress = off;
      });
      this.observeVisibility();
      this.refreshTimer = window.setInterval(() => void this.refresh(), 15e3);
      onStatusChange((p) => {
        if (p.router === this.router) void this.refresh(p.router);
      }).then((off) => {
        this.offStatus = off;
      });
    }
    disconnectedCallback() {
      this.unsubUpdate?.();
      this.offProgress?.();
      this.offProgress = null;
      this.offStatus?.();
      this.offStatus = null;
      this.visibilityObserver?.disconnect();
      this.visibilityObserver = null;
      if (this.refreshTimer !== null) {
        clearInterval(this.refreshTimer);
        this.refreshTimer = null;
      }
    }
    observeVisibility() {
      let host = this;
      while (host && host !== document.body && !host.classList?.contains("view")) {
        host = host.parentElement;
      }
      if (!host || host === document.body) return;
      this.visibilityObserver = new MutationObserver(() => {
        if (host.classList.contains("active")) void this.refresh();
      });
      this.visibilityObserver.observe(host, { attributes: true, attributeFilter: ["class"] });
    }
    async refresh(target = this.router) {
      if (!this.states.has(target)) {
        this.states.set(target, { kind: "loading" });
        this.render();
      }
      try {
        this.states.set(target, { kind: "ready", status: await getStatus(target) });
      } catch (e) {
        const msg = `\u041D\u0435 \u0443\u0434\u0430\u043B\u043E\u0441\u044C \u043F\u043E\u043B\u0443\u0447\u0438\u0442\u044C \u0441\u0442\u0430\u0442\u0443\u0441 ${target}: ${String(e)}`;
        logPlugin(`[cloud-routers] ${msg}`);
        toast(msg, "error");
        this.states.set(target, { kind: "error", message: msg });
      }
      this.render();
    }
    onProgress(router, text, done, total) {
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
      if (!entry || router !== this.router) return;
      const bar = this.root.querySelector(".progress-bar");
      const label = this.root.querySelector(".progress-status");
      const box = this.root.querySelector(".progress-container");
      if (!box || !bar || !label) return;
      box.classList.add("on");
      if (entry.total < 0) {
        bar.classList.remove("indeterminate");
        bar.style.width = "0%";
        label.textContent = entry.text;
        return;
      }
      const indeterminate = entry.total <= 0;
      bar.classList.toggle("indeterminate", indeterminate);
      if (!indeterminate) {
        bar.style.width = `${Math.min(100, entry.done / entry.total * 100).toFixed(1)}%`;
      }
      const pct = indeterminate ? "\u2026" : `${Math.min(100, entry.done / entry.total * 100).toFixed(0)}%`;
      label.innerHTML = `<span>${esc(entry.text)}</span><span>${pct}</span>`;
    }
    setBtnBusy(selector, v) {
      const btn = this.root.querySelector(selector);
      if (btn) {
        btn.disabled = v;
        btn.classList.toggle("busy", v);
      }
    }
    notifyCombosChanged(router = this.router) {
      window.dispatchEvent(
        new CustomEvent("cloud-routers:combos-changed", {
          detail: { router, combos: this.combosByRouter.get(router) ?? [] }
        })
      );
    }
    async onInstall(force = true) {
      const router = this.router;
      this.setBtnBusy(".install", true);
      this.setBtnBusy(".install-update", true);
      try {
        this.setStatus(router, await installOrUpdate(router, force));
        this.setCombos(router, await getCombos(router).catch(() => []));
        this.progressByRouter.delete(router);
      } catch (e) {
        this.reportInstallError(router, "\u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043A\u0438", String(e));
        const fresh = await getStatus(router).catch(() => null);
        if (fresh) this.setStatus(router, fresh);
        this.render();
        throw e;
      } finally {
        this.setBtnBusy(".install", false);
        this.setBtnBusy(".install-update", false);
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
      const msg = `\u041E\u0448\u0438\u0431\u043A\u0430 ${actionLabel}: ${errorText}`;
      logPlugin(`[cloud-routers] ${msg}`);
      toast(msg, "error");
      this.progressByRouter.set(router, { text: `\u041E\u0448\u0438\u0431\u043A\u0430: ${errorText}`, done: 0, total: -1 });
      const label = this.root.querySelector(".progress-status");
      const box = this.root.querySelector(".progress-container");
      if (box) box.classList.add("on");
      if (label) label.textContent = `\u041E\u0448\u0438\u0431\u043A\u0430: ${errorText}`;
    }
    async onOpen() {
      const router = this.router;
      this.setBtnBusy(".open", true);
      try {
        await openDashboard(router);
        const fresh = await getStatus(router).catch(() => null);
        if (fresh) this.setStatus(router, fresh);
      } catch (e) {
        const msg = `\u041E\u0448\u0438\u0431\u043A\u0430 \u043E\u0442\u043A\u0440\u044B\u0442\u0438\u044F Web UI: ${String(e)}`;
        logPlugin(`[cloud-routers] ${msg}`);
        toast(msg, "error");
      } finally {
        this.setBtnBusy(".open", false);
      }
      this.render();
    }
    async onStart() {
      const router = this.router;
      this.setBtnBusy(".start", true);
      try {
        const status = await ensureStarted(router);
        this.setStatus(router, status);
        this.setCombos(router, await getCombos(router).catch(() => []));
      } catch (e) {
        const msg = `\u041E\u0448\u0438\u0431\u043A\u0430 \u0437\u0430\u043F\u0443\u0441\u043A\u0430: ${String(e)}`;
        logPlugin(`[cloud-routers] ${msg}`);
        toast(msg, "error");
      } finally {
        this.setBtnBusy(".start", false);
      }
      this.notifyCombosChanged(router);
      this.render();
    }
    async onStop() {
      const router = this.router;
      this.setBtnBusy(".stop", true);
      try {
        this.setStatus(router, await stop(router));
        this.setCombos(router, []);
        this.notifyCombosChanged(router);
      } catch (e) {
        const msg = `\u041E\u0448\u0438\u0431\u043A\u0430 \u043E\u0441\u0442\u0430\u043D\u043E\u0432\u043A\u0438: ${String(e)}`;
        logPlugin(`[cloud-routers] ${msg}`);
        toast(msg, "error");
      } finally {
        this.setBtnBusy(".stop", false);
      }
      this.render();
    }
    async onShowCombos() {
      const router = this.router;
      this.setBtnBusy(".refresh", true);
      try {
        this.setCombos(router, await getCombos(router));
        this.setStatus(router, await getStatus(router));
      } catch (e) {
        const msg = `\u041E\u0448\u0438\u0431\u043A\u0430 \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u044F \u043A\u043E\u043C\u0431\u043E: ${String(e)}`;
        logPlugin(`[cloud-routers] ${msg}`);
        toast(msg, "error");
      } finally {
        this.setBtnBusy(".refresh", false);
      }
      this.notifyCombosChanged(router);
      this.render();
    }
    async onSetDir() {
      const router = this.router;
      this.setBtnBusy(".setdir", true);
      try {
        const sel = await open({ directory: true });
        if (!sel) return;
        const path = Array.isArray(sel) ? sel[0] : sel;
        if (!path) return;
        this.setStatus(router, await setRouterDir(router, path));
        this.setCombos(router, []);
        this.notifyCombosChanged(router);
        this.render();
      } catch (e) {
        const msg = `\u041E\u0448\u0438\u0431\u043A\u0430 \u0441\u043C\u0435\u043D\u044B \u043F\u0443\u0442\u0438: ${String(e)}`;
        logPlugin(`[cloud-routers] ${msg}`);
        toast(msg, "error");
        const label = this.root.querySelector(".progress-status");
        const box = this.root.querySelector(".progress-container");
        if (box) box.classList.add("on");
        if (label) label.textContent = `\u041E\u0448\u0438\u0431\u043A\u0430: ${String(e)}`;
      } finally {
        this.setBtnBusy(".setdir", false);
      }
    }
    async onCheckUpdate() {
      const router = this.router;
      const btn = this.root.querySelector(".check-update");
      const label = this.root.querySelector(".progress-status");
      const box = this.root.querySelector(".progress-container");
      if (btn) btn.disabled = true;
      this.renderUpdateState({ hasUpdate: false });
      try {
        const next = await checkUpdate(router);
        if (box) box.classList.add("on");
        if (label) {
          label.textContent = next.hasUpdate ? `\u{1F525} \u0414\u043E\u0441\u0442\u0443\u043F\u043D\u043E \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u0435: v${next.tag ?? ""}` : `\u0423\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D\u0430 \u0430\u043A\u0442\u0443\u0430\u043B\u044C\u043D\u0430\u044F \u0432\u0435\u0440\u0441\u0438\u044F ${this.status?.version ? `(v${this.status.version})` : ""}`;
        }
      } catch (e) {
        const msg = `\u041E\u0448\u0438\u0431\u043A\u0430 \u043F\u0440\u043E\u0432\u0435\u0440\u043A\u0438 \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u044F: ${String(e)}`;
        logPlugin(`[cloud-routers] ${msg}`);
        toast(msg, "error");
        if (box) box.classList.add("on");
        if (label) label.textContent = `\u041E\u0448\u0438\u0431\u043A\u0430 \u043F\u0440\u043E\u0432\u0435\u0440\u043A\u0438: ${String(e)}`;
      } finally {
        if (btn) btn.disabled = false;
      }
    }
    /** Скопировать адрес Web UI: юзер открывает дашборд в обычном браузере, а не
     * только вкладкой приложения. Адрес — из статуса роутера
     * (`RouterStatus.base_url`), доп. источников нет (SSOT). */
    async onCopyUrl() {
      const url = this.status?.base_url;
      if (!url) return;
      try {
        await navigator.clipboard.writeText(url);
        toast("\u0410\u0434\u0440\u0435\u0441 Web UI \u0441\u043A\u043E\u043F\u0438\u0440\u043E\u0432\u0430\u043D \u0432 \u0431\u0443\u0444\u0435\u0440", "success");
        logPlugin(`[cloud-routers] \u0430\u0434\u0440\u0435\u0441 \u0441\u043A\u043E\u043F\u0438\u0440\u043E\u0432\u0430\u043D: ${url}`);
      } catch (e) {
        const msg = `\u041D\u0435 \u0443\u0434\u0430\u043B\u043E\u0441\u044C \u0441\u043A\u043E\u043F\u0438\u0440\u043E\u0432\u0430\u0442\u044C \u0430\u0434\u0440\u0435\u0441: ${String(e)}`;
        logPlugin(`[cloud-routers] ${msg}`);
        toast(msg, "error");
      }
    }
    async onInstallUpdate() {
      const router = this.router;
      try {
        await this.onInstall(false);
      } catch {
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
      if (this.installing.has(router)) return;
      if (this.reinstallArm !== router) {
        this.reinstallArm = router;
        const label = this.root.querySelector(".progress-status");
        const box = this.root.querySelector(".progress-container");
        if (box) box.classList.add("on");
        if (label) {
          label.innerHTML = `<span>\u041F\u0435\u0440\u0435\u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043A\u0430 \u0443\u0434\u0430\u043B\u0438\u0442 \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043A\u0443 \u0438 \u0434\u0430\u043D\u043D\u044B\u0435 \u0440\u043E\u0443\u0442\u0435\u0440\u0430 (\u043D\u0430\u0441\u0442\u0440\u043E\u0439\u043A\u0438 \u0438 \u043F\u0430\u0440\u043E\u043B\u044C \u0434\u0430\u0448\u0431\u043E\u0440\u0434\u0430, \u043A\u043E\u043C\u0431\u043E). \u041D\u0430\u0436\u043C\u0438\u0442\u0435 <b>\xAB\u041F\u0435\u0440\u0435\u0443\u0441\u0442\u0430\u043D\u043E\u0432\u0438\u0442\u044C\xBB</b> \u0435\u0449\u0451 \u0440\u0430\u0437 \u0432 \u0442\u0435\u0447\u0435\u043D\u0438\u0435 6 \u0441\u0435\u043A\u0443\u043D\u0434.</span><span></span>`;
        }
        if (this.reinstallArmTimer !== null) window.clearTimeout(this.reinstallArmTimer);
        this.reinstallArmTimer = window.setTimeout(() => {
          this.disarmReinstallArm();
          this.render();
        }, 6e3);
        return;
      }
      this.disarmReinstallArm();
      this.installing.add(router);
      this.setBtnBusy(".reinstall", true);
      try {
        this.setStatus(router, await reinstall(router));
        this.setCombos(router, await getCombos(router).catch(() => []));
        this.progressByRouter.delete(router);
      } catch (e) {
        this.reportInstallError(router, "\u043F\u0435\u0440\u0435\u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043A\u0438", String(e));
        const fresh = await getStatus(router).catch(() => null);
        if (fresh) this.setStatus(router, fresh);
      } finally {
        this.installing.delete(router);
        this.setBtnBusy(".reinstall", false);
      }
      this.notifyCombosChanged(router);
      this.render();
    }
    switchRouter(router) {
      if (router === this.router) return;
      this.disarmReinstallArm();
      this.router = router;
      this.render();
      void this.refresh(router);
    }
    render() {
      const state = this.stateOf(this.router);
      const known = state.kind === "ready";
      const s = this.status;
      const installed = s?.installed ?? false;
      const running = s?.running ?? false;
      const native = isNativeGateway(this.router);
      let statusPill = `<span class="status-pill offline"><span class="pulse-dot"></span> \u041E\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D</span>`;
      if (!known) {
        statusPill = `<span class="status-pill"><span class="spinner"></span> \u041F\u0440\u043E\u0432\u0435\u0440\u043A\u0430</span>`;
      } else if (installed && running) {
        statusPill = `<span class="status-pill online"><span class="pulse-dot"></span> \u0412 \u0441\u0435\u0442\u0438</span>`;
      } else if (installed && !running) {
        statusPill = `<span class="status-pill warn"><span class="pulse-dot"></span> \u0413\u043E\u0442\u043E\u0432 \u043A \u0437\u0430\u043F\u0443\u0441\u043A\u0443</span>`;
      }
      const message = known ? s.message : state.kind === "error" ? state.message : "\u0421\u0438\u043D\u0445\u0440\u043E\u043D\u0438\u0437\u0430\u0446\u0438\u044F \u0441\u043E\u0441\u0442\u043E\u044F\u043D\u0438\u044F \u0448\u043B\u044E\u0437\u0430\u2026";
      const failedUpdate = s?.failed_update;
      const failedBanner = failedUpdate ? `
        <div class="banner banner-warn">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="flex-shrink:0;margin-top:2px;"><path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3Z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
          <div>
            \u041E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u0435${failedUpdate.target_version ? ` \u0434\u043E <b>v${esc(failedUpdate.target_version)}</b>` : ""} \u043D\u0435 \u0443\u0434\u0430\u043B\u043E\u0441\u044C \u2014
            \u0434\u0435\u0439\u0441\u0442\u0432\u0443\u044E\u0449\u0430\u044F \u0432\u0435\u0440\u0441\u0438\u044F \u043D\u0435 \u0442\u0440\u043E\u043D\u0443\u0442\u0430 \u0438 \u043F\u0440\u043E\u0434\u043E\u043B\u0436\u0430\u0435\u0442 \u0440\u0430\u0431\u043E\u0442\u0430\u0442\u044C.
            <div style="margin-top:6px;white-space:pre-wrap;font-family:ui-monospace,monospace;font-size:11px;opacity:.85;">${esc(failedUpdate.reason)}</div>
            <div style="margin-top:6px;">\u041D\u0430\u0436\u043C\u0438\u0442\u0435 <b>\xAB\u041E\u0431\u043D\u043E\u0432\u0438\u0442\u044C \u0441\u0435\u0439\u0447\u0430\u0441\xBB</b>, \u0447\u0442\u043E\u0431\u044B \u043F\u043E\u0432\u0442\u043E\u0440\u0438\u0442\u044C \u043F\u043E\u043F\u044B\u0442\u043A\u0443.</div>
          </div>
        </div>` : "";
      const version = s?.version ? `v${s.version}` : "\u2014";
      const port = s?.port ? `${s.port}` : "\u2014";
      const dbStatus = s?.has_npm_runtime ? s?.db_present ? "\u041F\u043E\u0434\u043A\u043B\u044E\u0447\u0435\u043D\u0430" : "\u041D\u0435 \u043D\u0430\u0439\u0434\u0435\u043D\u0430" : "N/A";
      const actions = known ? `
        ${s?.can_install ? `<button class="primary install" style="${installed ? "display:none;" : ""}">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
          \u0423\u0441\u0442\u0430\u043D\u043E\u0432\u0438\u0442\u044C
        </button>` : ""}

        ${!running ? `<button class="start" ${installed ? "" : "disabled"}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><polygon points="5 3 19 12 5 21 5 3"/></svg>
          \u0417\u0430\u043F\u0443\u0441\u0442\u0438\u0442\u044C
        </button>` : ""}

        <button class="stop" ${running ? "" : "disabled"}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><rect x="4" y="4" width="16" height="16" rx="2"/></svg>
          \u041E\u0441\u0442\u0430\u043D\u043E\u0432\u0438\u0442\u044C
        </button>

        <button class="open" ${installed ? "" : "disabled"}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></svg>
          Web UI
        </button>

        ${s?.can_check_update ? `
          <button class="check-update" style="${installed ? "" : "display:none;"}">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21.5 2v6h-6M21.34 15.57a10 10 0 1 1-.57-8.38l5.67-5.67"/></svg>
            \u041E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u044F
          </button>
        ` : ""}

        <button class="primary install-update" style="display:none;">\u041E\u0431\u043D\u043E\u0432\u0438\u0442\u044C \u0441\u0435\u0439\u0447\u0430\u0441</button>

        <button class="refresh" ${installed ? "" : "disabled"} title="\u041F\u0435\u0440\u0435\u0447\u0438\u0442\u0430\u0442\u044C \u043A\u043E\u043D\u0444\u0438\u0433\u0443\u0440\u0430\u0446\u0438\u044E">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/><path d="M8 16H3v5"/></svg>
          \u041A\u043E\u043C\u0431\u043E
        </button>

        <button class="setdir" title="\u0418\u0437\u043C\u0435\u043D\u0438\u0442\u044C \u0434\u0438\u0440\u0435\u043A\u0442\u043E\u0440\u0438\u044E \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043A\u0438">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
          \u041F\u0443\u0442\u044C
        </button>

        ${!native && installed ? `
          <button class="reinstall" ${this.installing.has(this.router) ? "disabled" : ""} title="\u0421\u043D\u0435\u0441\u0442\u0438 \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043A\u0443 \u0438 \u0434\u0430\u043D\u043D\u044B\u0435 \u0440\u043E\u0443\u0442\u0435\u0440\u0430 (\u043D\u0430\u0441\u0442\u0440\u043E\u0439\u043A\u0438 \u0438 \u043F\u0430\u0440\u043E\u043B\u044C \u0434\u0430\u0448\u0431\u043E\u0440\u0434\u0430, \u043A\u043E\u043C\u0431\u043E) \u0438 \u043F\u043E\u0441\u0442\u0430\u0432\u0438\u0442\u044C \u0437\u0430\u043D\u043E\u0432\u043E">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3Z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
            \u041F\u0435\u0440\u0435\u0443\u0441\u0442\u0430\u043D\u043E\u0432\u0438\u0442\u044C
          </button>
        ` : ""}
      ` : `<span style="color:var(--text-dim)"><span class="spinner"></span> \u0417\u0430\u0433\u0440\u0443\u0437\u043A\u0430 \u0434\u0435\u0439\u0441\u0442\u0432\u0438\u0439...</span>`;
      this.root.innerHTML = `
      <style>${STYLE}</style>

      <!-- \u0422\u0430\u0431\u044B -->
      <div class="tabs-nav">
        ${ROUTER_IDS.map(
        (id) => `<button class="${this.router === id ? "active" : ""}" data-router="${id}">${ROUTER_LABELS[id]}</button>`
      ).join("")}
      </div>

      <!-- \u0413\u043B\u0430\u0432\u043D\u0430\u044F \u0441\u0442\u0430\u0442\u0443\u0441-\u043A\u0430\u0440\u0442\u043E\u0447\u043A\u0430 -->
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

      <!-- \u0421\u0435\u0442\u043A\u0430 \u043F\u0430\u0440\u0430\u043C\u0435\u0442\u0440\u043E\u0432 -->
      <div class="specs-grid">
        <div class="spec-item">
          <span class="spec-label">\u041F\u043E\u0440\u0442</span>
          <span class="spec-value" style="color: #38bdf8;">${port}</span>
        </div>
        ${s?.base_url ? `
          <!-- \u0410\u0434\u0440\u0435\u0441 Web UI: \u044E\u0437\u0435\u0440 \u043E\u0442\u043A\u0440\u044B\u0432\u0430\u0435\u0442 \u0434\u0430\u0448\u0431\u043E\u0440\u0434 \u0432 \u043E\u0431\u044B\u0447\u043D\u043E\u043C \u0431\u0440\u0430\u0443\u0437\u0435\u0440\u0435, \u043A\u043E\u043F\u0438\u0440\u0443\u044F
               \u0435\u0433\u043E \u043E\u0442\u0441\u044E\u0434\u0430 (\u043F\u0440\u0430\u0432\u0438\u043B\u043E SSOT: \u0430\u0434\u0440\u0435\u0441 \u2014 \u0442\u043E\u043B\u044C\u043A\u043E \u0438\u0437 RouterStatus.base_url,
               \u043D\u0438\u043A\u0430\u043A\u0438\u0445 \u0432\u0442\u043E\u0440\u044B\u0445 \u0438\u0441\u0442\u043E\u0447\u043D\u0438\u043A\u043E\u0432). -->
          <div class="spec-item" style="grid-column: span 2;">
            <span class="spec-label">\u0410\u0434\u0440\u0435\u0441</span>
            <span class="spec-value" style="display:flex; align-items:center; gap:8px; min-width:0;">
              <span style="overflow:hidden; text-overflow:ellipsis; white-space:nowrap;" title="${esc(s.base_url)}">${esc(s.base_url)}</span>
              <button class="copy-url" title="\u0421\u043A\u043E\u043F\u0438\u0440\u043E\u0432\u0430\u0442\u044C \u0430\u0434\u0440\u0435\u0441, \u0447\u0442\u043E\u0431\u044B \u043E\u0442\u043A\u0440\u044B\u0442\u044C \u0434\u0430\u0448\u0431\u043E\u0440\u0434 \u0432 \u043E\u0431\u044B\u0447\u043D\u043E\u043C \u0431\u0440\u0430\u0443\u0437\u0435\u0440\u0435">\u0421\u043A\u043E\u043F\u0438\u0440\u043E\u0432\u0430\u0442\u044C</button>
            </span>
          </div>
        ` : ""}
        ${s?.has_npm_runtime ? `
          <div class="spec-item">
            <span class="spec-label">Runtime</span>
            <span class="spec-value">${s?.node_version ? `Node ${s.node_version}` : "\u2014"}</span>
          </div>
          <div class="spec-item">
            <span class="spec-label">\u0411\u0430\u0437\u0430 \u0434\u0430\u043D\u043D\u044B\u0445</span>
            <span class="spec-value" style="color:${s?.db_present ? "var(--success)" : "var(--warning)"};">${dbStatus}</span>
          </div>
        ` : ""}
        <div class="spec-item" style="grid-column: span 2;">
          <span class="spec-label">\u041A\u0430\u0442\u0430\u043B\u043E\u0433 \u0434\u0430\u043D\u043D\u044B\u0445</span>
          <span class="spec-value" title="${s?.data_dir ?? ""}">${s?.data_dir ? esc(s.data_dir) : "\u041D\u0435 \u0437\u0430\u0434\u0430\u043D"}</span>
        </div>
      </div>

      <!-- \u0422\u0443\u043B\u0431\u0430\u0440 \u0434\u0435\u0439\u0441\u0442\u0432\u0438\u0439 -->
      <div class="actions-bar">
        ${actions}
      </div>

      <!-- \u0423\u0432\u0435\u0434\u043E\u043C\u043B\u0435\u043D\u0438\u044F/\u0411\u0430\u043D\u043D\u0435\u0440\u044B -->
      ${failedBanner}

      ${native ? `
        <div class="banner banner-info">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="flex-shrink:0;margin-top:2px;"><circle cx="12" cy="12" r="10"/><line x1="12" y1="16" x2="12" y2="12"/><line x1="12" y1="8" x2="12.01" y2="8"/></svg>
          <div>
            \u0428\u043B\u044E\u0437 \u043D\u0430\u0441\u0442\u0440\u0430\u0438\u0432\u0430\u0435\u0442\u0441\u044F \u0432 \u0441\u0432\u043E\u0451\u043C UI. \u0416\u043C\u0438 <b>\xABWeb UI\xBB</b> \u2014 \u0434\u0430\u0448\u0431\u043E\u0440\u0434 \u043E\u0442\u043A\u0440\u043E\u0435\u0442\u0441\u044F \u0432\u043A\u043B\u0430\u0434\u043A\u043E\u0439 \u043F\u0440\u044F\u043C\u043E \u0432 \u043F\u0440\u0438\u043B\u043E\u0436\u0435\u043D\u0438\u0438.
          </div>
        </div>
      ` : ""}

      ${!installed && !native && (s?.node_present || s?.server_present) && !this.installing.has(this.router) ? `
        <!-- \u041F\u043B\u0430\u0448\u043A\u0430 \u043F\u043E\u043A\u0430\u0437\u044B\u0432\u0430\u0435\u0442\u0441\u044F \u0422\u041E\u041B\u042C\u041A\u041E \u043A\u043E\u0433\u0434\u0430 \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043A\u0430 \u043D\u0435 \u0438\u0434\u0451\u0442: \u0432\u043E \u0432\u0440\u0435\u043C\u044F
             \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043A\u0438 \u0441\u0442\u0430\u0442\u0443\u0441 \u0447\u0435\u0441\u0442\u043D\u043E \xAB\u043D\u0435 \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D\xBB (node \u0435\u0441\u0442\u044C, tgz \u0435\u0449\u0451
             \u043A\u0430\u0447\u0430\u0435\u0442\u0441\u044F), \u0438 \u043F\u0440\u0435\u0434\u0443\u043F\u0440\u0435\u0436\u0434\u0435\u043D\u0438\u0435 \u043E \u043F\u043E\u043B\u043E\u043C\u043A\u0435 \u0437\u0434\u0435\u0441\u044C \u043B\u043E\u0436\u044C. -->
        <div class="banner banner-warn">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="flex-shrink:0;margin-top:2px;"><path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3Z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
          <div>\u041D\u0435\u043F\u043E\u043B\u043D\u0430\u044F \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043A\u0430: \u043E\u0442\u0441\u0443\u0442\u0441\u0442\u0432\u0443\u044E\u0442 \u043A\u043E\u043C\u043F\u043E\u043D\u0435\u043D\u0442\u044B \u043F\u0430\u043A\u0435\u0442\u0430. \u041D\u0430\u0436\u043C\u0438\u0442\u0435 <b>\xAB\u0423\u0441\u0442\u0430\u043D\u043E\u0432\u0438\u0442\u044C\xBB</b>, \u0447\u0442\u043E\u0431\u044B \u0432\u043E\u0441\u0441\u0442\u0430\u043D\u043E\u0432\u0438\u0442\u044C \u0446\u0435\u043B\u043E\u0441\u0442\u043D\u043E\u0441\u0442\u044C.</div>
        </div>
      ` : ""}

      <!-- \u041F\u0440\u043E\u0433\u0440\u0435\u0441\u0441 \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043A\u0438 / \u0437\u0430\u0433\u0440\u0443\u0437\u043A\u0438 -->
      <div class="progress-container">
        <div class="progress-status"><span>\u041F\u043E\u0434\u0433\u043E\u0442\u043E\u0432\u043A\u0430...</span><span>0%</span></div>
        <div class="progress-track"><div class="progress-bar"></div></div>
      </div>

      <!-- \u041C\u0430\u043F\u043F\u0438\u043D\u0433 \u043A\u043E\u043C\u0431\u043E-\u043C\u043E\u0434\u0435\u043B\u0435\u0439 -->
      <div class="combos-wrapper ${this.combos.length ? "on" : ""}">
        <div class="combos-title">\u0410\u043A\u0442\u0438\u0432\u043D\u044B\u0435 \u0441\u0432\u044F\u0437\u043A\u0438 \u043C\u043E\u0434\u0435\u043B\u0435\u0439 (${this.combos.length})</div>
        ${this.combos.map((c) => `
          <div class="combo-item">
            <div class="combo-header">${esc(c.name)}</div>
            <div class="combo-chips">
              ${c.models.map((m) => `<span class="model-chip">${esc(m)}</span>`).join("")}
            </div>
          </div>
        `).join("")}
      </div>

      <!-- \u041F\u0443\u0442\u044C \u043A \u0431\u0438\u043D\u0430\u0440\u043D\u0438\u043A\u0443/\u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043A\u0435 -->
      <div class="path-box" title="${s?.path ? esc(s.path) : ""}">
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/></svg>
        <span>\u041F\u0443\u0442\u044C: ${s?.path ? esc(s.path) : "\u041D\u0435 \u043E\u043F\u0440\u0435\u0434\u0435\u043B\u0435\u043D"}</span>
      </div>
    `;
      this.root.querySelectorAll(".tabs-nav button").forEach((b) => {
        b.addEventListener("click", () => this.switchRouter(b.dataset.router));
      });
      this.root.querySelector(".install")?.addEventListener("click", () => {
        void this.onInstall().catch(() => {
        });
      });
      this.root.querySelector(".check-update")?.addEventListener("click", () => void this.onCheckUpdate());
      this.root.querySelector(".install-update")?.addEventListener("click", () => void this.onInstallUpdate());
      this.root.querySelector(".copy-url")?.addEventListener("click", () => void this.onCopyUrl());
      this.root.querySelector(".start")?.addEventListener("click", () => void this.onStart());
      this.root.querySelector(".stop")?.addEventListener("click", () => void this.onStop());
      this.root.querySelector(".refresh")?.addEventListener("click", () => void this.onShowCombos());
      this.root.querySelector(".open")?.addEventListener("click", () => void this.onOpen());
      this.root.querySelector(".setdir")?.addEventListener("click", () => void this.onSetDir());
      this.root.querySelector(".reinstall")?.addEventListener("click", () => void this.onReinstall());
      this.renderUpdateState(getUpdateState(this.router));
      this.applyProgress(this.router);
    }
    renderUpdateState(s) {
      const badge = this.root.querySelector("#updateBadge");
      const updateBtn = this.root.querySelector(".install-update");
      const display = s.hasUpdate ? "inline-flex" : "none";
      if (badge) badge.style.display = display;
      if (updateBtn) updateBtn.style.display = display;
    }
  };
  if (!customElements.get("cloud-routers-panel")) {
    customElements.define("cloud-routers-panel", CloudRoutersPanel);
  }
  void onLoginState((p) => {
    logPlugin(`no_login: ${p.router} \u2014 ${p.detail}`);
    toast(p.detail, "error");
  }).catch(() => {
  });

  // guest-js/index.ts
  var PROGRESS_EVENT = "cloud-routers-progress";
  var CHUNK_EVENT = "cloud-routers-chunk";
  var OPEN_DASHBOARD_EVENT = "cloud-routers-open-dashboard";
  var ROUTER_IDS = ["9router", "extremerouter", "omniroute", "gateway"];
  function getStatus(router) {
    return invoke("plugin:cloud-routers|get_status", { router });
  }
  function installOrUpdate(router, force = false) {
    return invoke("plugin:cloud-routers|install_or_update", { router, force });
  }
  function reinstall(router) {
    return invoke("plugin:cloud-routers|reinstall", { router });
  }
  function ensureStarted(router) {
    return invoke("plugin:cloud-routers|ensure_started", { router });
  }
  function stop(router) {
    return invoke("plugin:cloud-routers|stop", { router });
  }
  function setRouterDir(router, path) {
    return invoke("plugin:cloud-routers|set_router_dir", { router, path });
  }
  function getCombos(router) {
    return invoke("plugin:cloud-routers|get_combos", { router });
  }
  function checkRouterUpdate(router) {
    return invoke("plugin:cloud-routers|check_router_update", { router });
  }
  function openDashboard(router) {
    return invoke("plugin:cloud-routers|open_dashboard", { router });
  }
  function chatCompletion(router, opts) {
    return invoke("plugin:cloud-routers|chat_completion", {
      router,
      model: opts.model,
      messages: opts.messages,
      maxTokens: opts.maxTokens,
      temperature: opts.temperature,
      author: opts.author
    });
  }
  function onProgress(cb) {
    return listen(PROGRESS_EVENT, (e) => cb(e.payload));
  }
  function onChunk(cb) {
    return listen(CHUNK_EVENT, (e) => cb(e.payload));
  }
  function onOpenDashboard(cb) {
    return listen(OPEN_DASHBOARD_EVENT, (e) => cb(e.payload));
  }
  var STATUS_EVENT = "cloud-routers-status";
  function onStatusChange(cb) {
    return listen(STATUS_EVENT, (e) => cb(e.payload));
  }

  // guest-js/iife-entry.ts
  var g4 = window;
  if ("__TAURI__" in window) {
    const tauri = g4.__TAURI__;
    if (tauri && !tauri["cloud-routers"]) {
      Object.defineProperty(tauri, "cloud-routers", {
        configurable: true,
        value: {
          chatCompletion,
          ensureStarted,
          getCombos,
          getStatus,
          installOrUpdate,
          onChunk,
          onOpenDashboard,
          onProgress,
          openDashboard,
          setRouterDir,
          stop
        }
      });
    }
  }
})();
