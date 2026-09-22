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
  var NineRouterPanel = class extends HTMLElement {
    constructor() {
      super();
      this.status = null;
      this.combos = [];
      this.busy = false;
      this.offProgress = null;
      this.visibilityObserver = null;
      this.refreshTimer = null;
      this.root = this.attachShadow({ mode: "open" });
    }
    connectedCallback() {
      this.render();
      void this.refresh();
      onProgress((p) => this.onProgress(p.text, p.done, p.total)).then((off) => {
        this.offProgress = off;
      });
      this.observeVisibility();
      this.refreshTimer = window.setInterval(() => void this.refresh(), 15e3);
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
        this.status = await getStatus();
      } catch (e) {
        this.status = null;
        console.error("[9router] getStatus failed", e);
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
      if (total > 0 && done >= total) void this.refresh();
    }
    setBusy(v) {
      this.busy = v;
      this.root.querySelectorAll("button").forEach((b) => b.disabled = v);
    }
    notifyCombosChanged() {
      window.dispatchEvent(new CustomEvent("9router:combos-changed", { detail: { combos: this.combos } }));
    }
    async onInstall() {
      this.setBusy(true);
      try {
        this.status = await installOrUpdate(true);
        this.combos = await getCombos().catch(() => []);
      } catch (e) {
        console.error("[9router] install failed", e);
        const label = this.root.querySelector(".progress-status");
        const box = this.root.querySelector(".progress-container");
        if (box) box.classList.add("on");
        if (label) label.textContent = `\u041E\u0448\u0438\u0431\u043A\u0430: ${String(e)}`;
        return;
      } finally {
        this.setBusy(false);
      }
      this.notifyCombosChanged();
      this.render();
    }
    async onOpen() {
      try {
        await openDashboard();
        this.status = await getStatus().catch(() => this.status);
      } catch (e) {
        console.error("[9router] openDashboard failed", e);
      }
      this.render();
    }
    async onShowCombos() {
      try {
        this.combos = await getCombos();
        this.status = await getStatus();
      } catch (e) {
        console.error("[9router] getCombos failed", e);
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
      try {
        this.status = await setApiKey(input.value.trim());
        ok && (ok.textContent = "\u2713 \u0441\u043E\u0445\u0440\u0430\u043D\u0451\u043D");
      } catch (e) {
        console.error("[9router] setApiKey failed", e);
        if (err) err.textContent = `\u041E\u0448\u0438\u0431\u043A\u0430: ${String(e)}`;
      }
      this.render();
    }
    async onSetDir() {
      try {
        const sel = await open({ directory: true });
        if (!sel) return;
        const path = Array.isArray(sel) ? sel[0] : sel;
        if (!path) return;
        this.status = await setRouterDir(path);
        this.combos = [];
        this.notifyCombosChanged();
        this.render();
      } catch (e) {
        console.error("[9router] set dir failed", e);
        const label = this.root.querySelector(".progress-status");
        const box = this.root.querySelector(".progress-container");
        if (box) box.classList.add("on");
        if (label) label.textContent = `\u041E\u0448\u0438\u0431\u043A\u0430: ${String(e)}`;
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
        const newTag = await checkRouterUpdate();
        if (newTag) {
          if (box) box.classList.add("on");
          if (label) label.textContent = `\u0414\u043E\u0441\u0442\u0443\u043F\u043D\u043E \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u0435 9Router: v${newTag}`;
          if (updateBtn) updateBtn.style.display = "inline-block";
        } else {
          if (box) box.classList.add("on");
          if (label) label.textContent = `9Router \u0430\u043A\u0442\u0443\u0430\u043B\u0435\u043D${this.status?.version ? ` (v${this.status.version})` : ""}`;
        }
      } catch (e) {
        if (box) box.classList.add("on");
        if (label) label.textContent = `\u041E\u0448\u0438\u0431\u043A\u0430 \u043F\u0440\u043E\u0432\u0435\u0440\u043A\u0438 \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u044F: ${String(e)}`;
      } finally {
        if (btn) btn.disabled = false;
      }
    }
    async onInstallUpdate() {
      await this.onInstall();
      const updateBtn = this.root.querySelector(".install-update");
      if (updateBtn) updateBtn.style.display = "none";
    }
    render() {
      const s = this.status;
      const installed = s?.installed ?? false;
      const running = s?.running ?? false;
      const dotClass = running ? "dot on" : installed ? "dot warn" : "dot";
      const message = s?.message ?? "\u0417\u0430\u0433\u0440\u0443\u0437\u043A\u0430...";
      const version = s?.version ? `v${s.version}` : "\u2014";
      const nodeVersion = s?.node_version ? `Node ${s.node_version}` : "Node \u2014";
      this.root.innerHTML = `
      <style>${STYLE}</style>
      <div class="status">
        <span class="${dotClass}"></span>
        <span>${message}</span>
      </div>
      <div class="row muted">
        <span>9Router: ${version}</span>
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
        <button class="refresh" ${installed ? "" : "disabled"}>\u27F3 \u041E\u0431\u043D\u043E\u0432\u0438\u0442\u044C \u043A\u043E\u043C\u0431\u043E</button>
        <button class="setdir">\u0418\u0437\u043C\u0435\u043D\u0438\u0442\u044C \u043F\u0443\u0442\u044C</button>
      </div>
      ${!installed && (s?.node_present || s?.server_present) ? '<div class="muted warn-hint">\u0427\u0430\u0441\u0442\u0438\u0447\u043D\u0430\u044F \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043A\u0430: \u043D\u0430\u0439\u0434\u0435\u043D\u044B \u043D\u0435 \u0432\u0441\u0435 \u043A\u043E\u043C\u043F\u043E\u043D\u0435\u043D\u0442\u044B 9Router. \u041D\u0430\u0436\u043C\u0438\u0442\u0435 \xAB\u0423\u0441\u0442\u0430\u043D\u043E\u0432\u0438\u0442\u044C\xBB, \u0447\u0442\u043E\u0431\u044B \u043F\u043E\u0447\u0438\u043D\u0438\u0442\u044C.</div>' : ""}
      <div class="progress-container">
        <div class="progress-status"></div>
        <div class="progress-track"><div class="progress-bar"></div></div>
      </div>
      <div class="combos ${this.combos.length ? "on" : ""}">
        ${this.combos.map(
        (c) => `<div class="combo"><div class="name">${esc(c.name)}</div><div class="models">${esc(c.models.join(", "))}</div></div>`
      ).join("")}
      </div>
      <div class="row path">${s?.path ? esc(s.path) : ""}</div>
    `;
      this.root.querySelector(".install")?.addEventListener("click", () => void this.onInstall());
      this.root.querySelector(".check-update")?.addEventListener("click", () => void this.onCheckUpdate());
      this.root.querySelector(".install-update")?.addEventListener("click", () => void this.onInstallUpdate());
      this.root.querySelector(".refresh")?.addEventListener("click", () => void this.onShowCombos());
      this.root.querySelector(".open")?.addEventListener("click", () => void this.onOpen());
      this.root.querySelector(".setdir")?.addEventListener("click", () => void this.onSetDir());
    }
  };
  function esc(s) {
    return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
  }
  if (!customElements.get("nine-router-panel")) {
    customElements.define("nine-router-panel", NineRouterPanel);
  }

  // guest-js/index.ts
  var PROGRESS_EVENT = "9router-progress";
  var CHUNK_EVENT = "9router-chunk";
  function getStatus() {
    return invoke("plugin:9router|get_status");
  }
  function installOrUpdate(force = false) {
    return invoke("plugin:9router|install_or_update", { force });
  }
  function ensureStarted() {
    return invoke("plugin:9router|ensure_started");
  }
  function stop() {
    return invoke("plugin:9router|stop");
  }
  function setRouterDir(path) {
    return invoke("plugin:9router|set_router_dir", { path });
  }
  function getCombos() {
    return invoke("plugin:9router|get_combos");
  }
  function setApiKey(key) {
    return invoke("plugin:9router|set_api_key", { key });
  }
  function checkRouterUpdate() {
    return invoke("plugin:9router|check_router_update");
  }
  function openDashboard() {
    return invoke("plugin:9router|open_dashboard");
  }
  function chatCompletion(opts) {
    return invoke("plugin:9router|chat_completion", {
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
    if (tauri && !tauri["9router"]) {
      Object.defineProperty(tauri, "9router", {
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
