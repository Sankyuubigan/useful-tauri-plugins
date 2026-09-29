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
  var state = { hasUpdate: false };
  var subs = /* @__PURE__ */ new Set();
  function getUpdateState() {
    return state;
  }
  function onUpdateState(fn) {
    subs.add(fn);
    fn(state);
    return () => {
      subs.delete(fn);
    };
  }
  function setUpdateState(next) {
    state = next;
    subs.forEach((fn) => fn(state));
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
  var CloudRoutersPanel = class extends HTMLElement {
    constructor() {
      super();
      this.router = "9router";
      this.status = null;
      this.combos = [];
      this.offProgress = null;
      this.visibilityObserver = null;
      this.refreshTimer = null;
      this.root = this.attachShadow({ mode: "open" });
    }
    connectedCallback() {
      this.render();
      this.unsubUpdate = onUpdateState((s) => this.renderUpdateState(s));
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
    async refresh() {
      try {
        this.status = await getStatus(this.router);
      } catch (e) {
        this.status = null;
        logPlugin(`[cloud-routers] getStatus failed: ${String(e)}`);
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
    notifyCombosChanged() {
      window.dispatchEvent(new CustomEvent("cloud-routers:combos-changed", {
        detail: { router: this.router, combos: this.combos }
      }));
    }
    async onInstall() {
      this.setBtnBusy(".install", true);
      try {
        this.status = await installOrUpdate(this.router, true);
        this.combos = await getCombos(this.router).catch(() => []);
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
      this.notifyCombosChanged();
      this.render();
    }
    async onOpen() {
      this.setBtnBusy(".open", true);
      try {
        await openDashboard(this.router);
        this.status = await getStatus(this.router).catch(() => this.status);
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
      this.setBtnBusy(".stop", true);
      try {
        this.status = await stop(this.router);
        this.combos = [];
        this.notifyCombosChanged();
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
      this.setBtnBusy(".refresh", true);
      try {
        this.combos = await getCombos(this.router);
        this.status = await getStatus(this.router);
      } catch (e) {
        const msg = `\u041E\u0448\u0438\u0431\u043A\u0430 \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u044F \u043A\u043E\u043C\u0431\u043E: ${String(e)}`;
        logPlugin(`[cloud-routers] ${msg}`);
        toast(msg, "error");
      } finally {
        this.setBtnBusy(".refresh", false);
      }
      this.notifyCombosChanged();
      this.render();
    }
    async onSaveApiKey() {
      const input = this.root.querySelector(".api-key-input");
      const ok = this.root.querySelector(".api-ok");
      const err = this.root.querySelector(".api-err");
      if (!input) return;
      ok && (ok.textContent = "");
      err && (err.textContent = "");
      this.setBtnBusy(".save-key", true);
      try {
        this.status = await setApiKey(this.router, input.value.trim());
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
      this.setBtnBusy(".setdir", true);
      try {
        const sel = await open({ directory: true });
        if (!sel) return;
        const path = Array.isArray(sel) ? sel[0] : sel;
        if (!path) return;
        this.status = await setRouterDir(this.router, path);
        this.combos = [];
        this.notifyCombosChanged();
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
      const btn = this.root.querySelector(".check-update");
      const updateBtn = this.root.querySelector(".install-update");
      if (btn) btn.disabled = true;
      if (updateBtn) updateBtn.style.display = "none";
      const label = this.root.querySelector(".progress-status");
      const box = this.root.querySelector(".progress-container");
      try {
        const newTag = await checkRouterUpdate(this.router);
        if (newTag) {
          if (box) box.classList.add("on");
          if (label) label.textContent = `\u0414\u043E\u0441\u0442\u0443\u043F\u043D\u043E \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u0435: v${newTag}`;
          if (updateBtn) updateBtn.style.display = "inline-block";
        } else {
          if (box) box.classList.add("on");
          if (label) label.textContent = `\u0410\u043A\u0442\u0443\u0430\u043B\u0435\u043D${this.status?.version ? ` (v${this.status.version})` : ""}`;
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
      await this.onInstall();
      setUpdateState({ hasUpdate: false });
      const updateBtn = this.root.querySelector(".install-update");
      if (updateBtn) updateBtn.style.display = "none";
    }
    switchRouter(router) {
      this.router = router;
      this.status = null;
      this.combos = [];
      void this.refresh();
      this.render();
    }
    render() {
      const s = this.status;
      const installed = s?.installed ?? false;
      const running = s?.running ?? false;
      const dotClass = installed && running ? "dot on" : installed ? "dot warn" : "dot";
      const message = s?.message ?? "\u0417\u0430\u0433\u0440\u0443\u0437\u043A\u0430...";
      const version = s?.version ? `v${s.version}` : "\u2014";
      const nodeVersion = s?.node_version ? `Node ${s.node_version}` : "Node \u2014";
      this.root.innerHTML = `
      <style>${STYLE}</style>
      <div class="tabs">
        <button class="${this.router === "9router" ? "active" : ""}" data-router="9router">9Router</button>
        <button class="${this.router === "extremerouter" ? "active" : ""}" data-router="extremerouter">ExtremeRouter</button>
        <button class="${this.router === "omniroute" ? "active" : ""}" data-router="omniroute">OmniRoute</button>
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
        <button class="primary install" style="${installed ? "display:none;" : ""}">\u0423\u0441\u0442\u0430\u043D\u043E\u0432\u0438\u0442\u044C</button>
        <button class="check-update" style="${installed ? "" : "display:none;"}">\u041F\u0440\u043E\u0432\u0435\u0440\u0438\u0442\u044C \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u0435</button>
        <button class="primary install-update" style="display:none;">\u041E\u0431\u043D\u043E\u0432\u0438\u0442\u044C</button>
        <button class="open" ${installed ? "" : "disabled"}>\u041E\u0442\u043A\u0440\u044B\u0442\u044C Web UI</button>
        <button class="stop" ${running ? "" : "disabled"}>\u041E\u0441\u0442\u0430\u043D\u043E\u0432\u0438\u0442\u044C</button>
        <button class="refresh" ${installed ? "" : "disabled"}>\u27F3 \u041E\u0431\u043D\u043E\u0432\u0438\u0442\u044C \u043A\u043E\u043C\u0431\u043E</button>
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
      this.renderUpdateState(getUpdateState());
    }
    renderUpdateState(s) {
      const badge = this.root.querySelector("#updateBadge");
      const updateBtn = this.root.querySelector(".install-update");
      if (s.hasUpdate) {
        if (badge) badge.style.display = "inline-block";
        if (updateBtn) updateBtn.style.display = "inline-block";
      } else if (badge) {
        badge.style.display = "none";
      }
    }
  };
  if (!customElements.get("cloud-routers-panel")) {
    customElements.define("cloud-routers-panel", CloudRoutersPanel);
  }

  // guest-js/index.ts
  var PROGRESS_EVENT = "cloud-routers-progress";
  var CHUNK_EVENT = "cloud-routers-chunk";
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
