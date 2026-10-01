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
    return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
  }
  function toast(msg, kind = "success") {
    const el = document.createElement("div");
    el.textContent = msg;
    el.style.cssText = `position:fixed; right:16px; bottom:16px; z-index:5000; max-width:420px; padding:10px 14px;border-radius:8px; font:13px var(--font, system-ui, sans-serif); box-shadow:0 3px 14px rgba(0,0,0,.4);background:${kind === "success" ? "var(--primary, #4a90d9)" : "var(--danger, #b54242)"}; color:#fff;`;
    document.body.appendChild(el);
    setTimeout(() => el.remove(), 3500);
  }
  function logPlugin(msg) {
    void invoke("plugin:logs|log_frontend_event", { level: "FE", msg }).catch(() => {
    });
  }
  var ROUTER_LABELS = {
    "9router": "9Router",
    extremerouter: "ExtremeRouter",
    omniroute: "OmniRoute"
  };
  var CloudRoutersPanel = class extends HTMLElement {
    constructor() {
      super();
      this.router = "9router";
      this.states = /* @__PURE__ */ new Map();
      this.combosByRouter = /* @__PURE__ */ new Map();
      this.offProgress = null;
      this.visibilityObserver = null;
      this.refreshTimer = null;
      this.root = this.attachShadow({ mode: "open" });
    }
    // ── Состояние: единственное место чтения/записи, ключ — роутер ──────────────
    stateOf(router) {
      return this.states.get(router) ?? { kind: "loading" };
    }
    /** Текущий статус активного роутера; null пока он неизвестен или ошибка. */
    get status() {
      const s = this.stateOf(this.router);
      return s.kind === "ready" ? s.status : null;
    }
    get combos() {
      return this.combosByRouter.get(this.router) ?? [];
    }
    /**
     * Запись состояния — всегда с явным роутером. Иначе ответ на действие, начатое
     * на одной вкладке, лёг бы в слот той, на которую юзер успел переключиться.
     */
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
        if (p.router === this.router) this.onProgress(p.text, p.done, p.total);
      }).then((off) => {
        this.offProgress = off;
      });
      this.observeVisibility();
      this.refreshTimer = window.setInterval(() => void this.refresh(), 15e3);
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
      while (host && host !== document.body && !host.classList?.contains("view")) {
        host = host.parentElement;
      }
      if (!host || host === document.body) return;
      this.visibilityObserver = new MutationObserver(() => {
        if (host.classList.contains("active")) void this.refresh();
      });
      this.visibilityObserver.observe(host, { attributes: true, attributeFilter: ["class"] });
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
    onProgress(text, done, total) {
      const bar = this.root.querySelector(".progress-bar");
      const label = this.root.querySelector(".progress-status");
      const box = this.root.querySelector(".progress-container");
      if (box) box.classList.add("on");
      if (label) label.textContent = text;
      if (bar) bar.style.width = total > 0 ? `${Math.min(100, done / total * 100).toFixed(1)}%` : "100%";
      if (total > 0 && done >= total) {
        logPlugin(`[cloud-routers] \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043A\u0430 \u0437\u0430\u0432\u0435\u0440\u0448\u0435\u043D\u0430: ${text}`);
        void this.refresh();
      }
    }
    setBtnBusy(selector, v) {
      const btn = this.root.querySelector(selector);
      if (btn) {
        btn.disabled = v;
        btn.classList.toggle("busy", v);
      }
    }
    notifyCombosChanged(router = this.router) {
      window.dispatchEvent(new CustomEvent("cloud-routers:combos-changed", {
        detail: { router, combos: this.combosByRouter.get(router) ?? [] }
      }));
    }
    async onInstall() {
      const router = this.router;
      this.setBtnBusy(".install", true);
      try {
        this.setStatus(router, await installOrUpdate(router, true));
        this.setCombos(router, await getCombos(router).catch(() => []));
      } catch (e) {
        const msg = `\u041E\u0448\u0438\u0431\u043A\u0430 \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043A\u0438: ${String(e)}`;
        logPlugin(`[cloud-routers] ${msg}`);
        toast(msg, "error");
        const label = this.root.querySelector(".progress-status");
        const box = this.root.querySelector(".progress-container");
        if (box) box.classList.add("on");
        if (label) label.textContent = `\u041E\u0448\u0438\u0431\u043A\u0430: ${String(e)}`;
        return;
      } finally {
        this.setBtnBusy(".install", false);
      }
      this.notifyCombosChanged(router);
      this.render();
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
    async onSaveApiKey() {
      const router = this.router;
      const input = this.root.querySelector(".api-key-input");
      const ok = this.root.querySelector(".api-ok");
      const err = this.root.querySelector(".api-err");
      if (!input) return;
      ok && (ok.textContent = "");
      err && (err.textContent = "");
      this.setBtnBusy(".save-key", true);
      try {
        this.setStatus(router, await setApiKey(router, input.value.trim()));
        ok && (ok.textContent = "\u2713 \u0441\u043E\u0445\u0440\u0430\u043D\u0451\u043D");
      } catch (e) {
        const msg = `\u041E\u0448\u0438\u0431\u043A\u0430 \u0441\u043E\u0445\u0440\u0430\u043D\u0435\u043D\u0438\u044F API-\u043A\u043B\u044E\u0447\u0430: ${String(e)}`;
        logPlugin(`[cloud-routers] ${msg}`);
        toast(msg, "error");
        if (err) err.textContent = `\u041E\u0448\u0438\u0431\u043A\u0430: ${String(e)}`;
      } finally {
        this.setBtnBusy(".save-key", false);
      }
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
          label.textContent = next.hasUpdate ? `\u0414\u043E\u0441\u0442\u0443\u043F\u043D\u043E \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u0435: v${next.tag ?? ""}` : `\u0410\u043A\u0442\u0443\u0430\u043B\u0435\u043D${this.status?.version ? ` (v${this.status.version})` : ""}`;
        }
      } catch (e) {
        const msg = `\u041E\u0448\u0438\u0431\u043A\u0430 \u043F\u0440\u043E\u0432\u0435\u0440\u043A\u0438 \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u044F: ${String(e)}`;
        logPlugin(`[cloud-routers] ${msg}`);
        toast(msg, "error");
        if (box) box.classList.add("on");
        if (label) label.textContent = `\u041E\u0448\u0438\u0431\u043A\u0430 \u043F\u0440\u043E\u0432\u0435\u0440\u043A\u0438 \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u044F: ${String(e)}`;
      } finally {
        if (btn) btn.disabled = false;
      }
    }
    async onInstallUpdate() {
      const router = this.router;
      await this.onInstall();
      setUpdateState(router, { hasUpdate: false });
    }
    switchRouter(router) {
      if (router === this.router) return;
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
      const dotClass = installed && running ? "dot on" : installed ? "dot warn" : "dot";
      const message = known ? s.message : state.kind === "error" ? state.message : "\u041F\u0440\u043E\u0432\u0435\u0440\u043A\u0430 \u0441\u043E\u0441\u0442\u043E\u044F\u043D\u0438\u044F\u2026";
      const version = s?.version ? `v${s.version}` : "\u2014";
      const nodeVersion = s?.node_version ? `Node ${s.node_version}` : "Node \u2014";
      const actions = known ? `<button class="primary install" style="${installed ? "display:none;" : ""}">\u0423\u0441\u0442\u0430\u043D\u043E\u0432\u0438\u0442\u044C</button>
         <button class="check-update" style="${installed ? "" : "display:none;"}">\u041F\u0440\u043E\u0432\u0435\u0440\u0438\u0442\u044C \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u0435</button>
         <button class="primary install-update" style="display:none;">\u041E\u0431\u043D\u043E\u0432\u0438\u0442\u044C</button>
         <button class="open" ${installed ? "" : "disabled"}>\u041E\u0442\u043A\u0440\u044B\u0442\u044C Web UI</button>
         <button class="stop" ${running ? "" : "disabled"}>\u041E\u0441\u0442\u0430\u043D\u043E\u0432\u0438\u0442\u044C</button>
         <button class="refresh" ${installed ? "" : "disabled"}>\u27F3 \u041E\u0431\u043D\u043E\u0432\u0438\u0442\u044C \u043A\u043E\u043C\u0431\u043E</button>` : `<span class="muted">${state.kind === "error" ? "\u0421\u043E\u0441\u0442\u043E\u044F\u043D\u0438\u0435 \u043F\u0440\u043E\u0432\u0435\u0440\u0438\u0442\u044C \u043D\u0435 \u0443\u0434\u0430\u043B\u043E\u0441\u044C \u2014 \u043F\u043E\u0432\u0442\u043E\u0440\u0438\u0442\u0435 \u0447\u0435\u0440\u0435\u0437 \xAB\u041E\u0431\u043D\u043E\u0432\u0438\u0442\u044C \u043A\u043E\u043C\u0431\u043E\xBB" : '<span class="spinner"></span> \u041F\u0440\u043E\u0432\u0435\u0440\u043A\u0430 \u0441\u043E\u0441\u0442\u043E\u044F\u043D\u0438\u044F\u2026'}</span>`;
      this.root.innerHTML = `
      <style>${STYLE}</style>
      <div class="tabs">
        ${ROUTER_IDS.map(
        (id) => `<button class="${this.router === id ? "active" : ""}" data-router="${id}">${ROUTER_LABELS[id]}</button>`
      ).join("")}
      </div>
      <div class="status">
        <span class="${dotClass}"></span>
        <span>${message}</span>
        <span id="updateBadge" class="update-badge" style="display:none;"></span>
      </div>
      <div class="row muted">
        <span>${this.router}: ${version}</span>
        <span>\xB7</span>
        <span>${nodeVersion}</span>
        <span>\xB7</span>
        <span>\u043F\u043E\u0440\u0442 ${s?.port ?? "\u2014"}</span>
      </div>
      <div class="row">
        ${actions}
        <button class="setdir">\u0418\u0437\u043C\u0435\u043D\u0438\u0442\u044C \u043F\u0443\u0442\u044C</button>
      </div>
      <div class="row muted">
        <span>\u041A\u0430\u0442\u0430\u043B\u043E\u0433 \u0431\u0430\u0437\u044B \u0434\u0430\u043D\u043D\u044B\u0445: ${s?.data_dir ? esc(s.data_dir) : "\u2014"}</span>
      </div>
      <div class="row muted">
        <span>\u0421\u0442\u0430\u0442\u0443\u0441 \u0411\u0414: ${s?.db_present ? "\u043D\u0430\u0439\u0434\u0435\u043D\u0430" : "\u26A0 \u043D\u0435 \u043D\u0430\u0439\u0434\u0435\u043D\u0430"}</span>
      </div>
      ${!installed && (s?.node_present || s?.server_present) ? '<div class="muted warn-hint">\u0427\u0430\u0441\u0442\u0438\u0447\u043D\u0430\u044F \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043A\u0430: \u043D\u0430\u0439\u0434\u0435\u043D\u044B \u043D\u0435 \u0432\u0441\u0435 \u043A\u043E\u043C\u043F\u043E\u043D\u0435\u043D\u0442\u044B. \u041D\u0430\u0436\u043C\u0438\u0442\u0435 \xAB\u0423\u0441\u0442\u0430\u043D\u043E\u0432\u0438\u0442\u044C\xBB, \u0447\u0442\u043E\u0431\u044B \u043F\u043E\u0447\u0438\u043D\u0438\u0442\u044C.</div>' : ""}
      <div class="progress-container">
        <div class="progress-status"></div>
        <div class="progress-track"><div class="progress-bar"></div></div>
      </div>
      <div class="combos ${this.combos.length ? "on" : ""}">
        ${this.combos.map(
        (c) => `<div class="combo"><div class="name">${esc(c.name)}</div><div class="models">${esc(c.models.join(", "))}</div></div>`
      ).join("")}
      </div>
      <div class="row path">\u041F\u0443\u0442\u044C \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043A\u0438 \u043F\u0440\u043E\u0433\u0440\u0430\u043C\u043C\u044B: ${s?.path ? esc(s.path) : "\u2014"}</div>
    `;
      this.root.querySelectorAll(".tabs button").forEach((b) => {
        b.addEventListener("click", () => this.switchRouter(b.dataset.router));
      });
      this.root.querySelector(".install")?.addEventListener("click", () => void this.onInstall());
      this.root.querySelector(".check-update")?.addEventListener("click", () => void this.onCheckUpdate());
      this.root.querySelector(".install-update")?.addEventListener("click", () => void this.onInstallUpdate());
      this.root.querySelector(".stop")?.addEventListener("click", () => void this.onStop());
      this.root.querySelector(".refresh")?.addEventListener("click", () => void this.onShowCombos());
      this.root.querySelector(".open")?.addEventListener("click", () => void this.onOpen());
      this.root.querySelector(".setdir")?.addEventListener("click", () => void this.onSetDir());
      this.renderUpdateState(getUpdateState(this.router));
    }
    renderUpdateState(s) {
      const badge = this.root.querySelector("#updateBadge");
      const updateBtn = this.root.querySelector(".install-update");
      const display = s.hasUpdate ? "inline-block" : "none";
      if (badge) badge.style.display = display;
      if (updateBtn) updateBtn.style.display = display;
    }
  };
  if (!customElements.get("cloud-routers-panel")) {
    customElements.define("cloud-routers-panel", CloudRoutersPanel);
  }

  // guest-js/index.ts
  var PROGRESS_EVENT = "cloud-routers-progress";
  var CHUNK_EVENT = "cloud-routers-chunk";
  var ROUTER_IDS = ["9router", "extremerouter", "omniroute"];
  function getStatus(router) {
    return invoke("plugin:cloud-routers|get_status", { router });
  }
  function installOrUpdate(router, force = false) {
    return invoke("plugin:cloud-routers|install_or_update", { router, force });
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
  function setApiKey(router, key) {
    return invoke("plugin:cloud-routers|set_api_key", { router, key });
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
          onProgress,
          openDashboard,
          setRouterDir,
          stop
        }
      });
    }
  }
})();
