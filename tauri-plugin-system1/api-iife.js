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
    const unlisten = await event.listen(name, (e) => cb(e));
    return unlisten;
  }

  // guest-js/web-components.ts
  function toast(message, kind = "success") {
    const element = document.createElement("div");
    element.textContent = message;
    element.style.cssText = `position:fixed; right:16px; bottom:16px; z-index:5000; max-width:420px; padding:10px 14px;border-radius:8px; font:13px var(--font, system-ui, sans-serif); box-shadow:0 3px 14px rgba(0,0,0,.4);background:${kind === "success" ? "var(--primary, #4a90d9)" : "var(--danger, #b54242)"}; color:#fff;`;
    document.body.appendChild(element);
    setTimeout(() => element.remove(), 3500);
  }
  function logPlugin(message) {
    void invoke("plugin:logs|log_frontend_event", { level: "FE", msg: message }).catch(() => {
    });
  }
  var STYLE = `
  :host {
    display: block;
    font: 13px var(--font, system-ui, sans-serif);
    color: var(--text, #e6e6e6);
  }
  .card {
    border: 1px solid var(--border, #2c3240);
    border-radius: 10px;
    padding: 14px 16px;
    background: var(--bg-2, #161a22);
  }
  h3 { margin: 0 0 10px; font-size: 14px; font-weight: 600; }
  .lead { margin: 0 0 12px; color: var(--text-dim, #9aa3b2); line-height: 1.45; }
  table { width: 100%; border-collapse: collapse; }
  th {
    text-align: left; font-weight: 600; font-size: 12px; color: var(--text-dim, #9aa3b2);
    padding: 4px 8px 4px 0; border-bottom: 1px solid var(--border, #2c3240);
  }
  td { padding: 5px 8px 5px 0; vertical-align: top; border-bottom: 1px solid var(--border, #2c3240); }
  td.size { text-align: right; white-space: nowrap; color: var(--text-dim, #9aa3b2); }
  .group {
    font-size: 11px; text-transform: uppercase; letter-spacing: .04em;
    color: var(--text-dim, #9aa3b2); padding-top: 10px;
  }
  .group:first-of-type { padding-top: 0; }
  .ok { color: var(--success, #4caf7d); white-space: nowrap; }
  .bad { color: var(--danger, #b54242); white-space: nowrap; }
  .muted { color: var(--text-dim, #9aa3b2); }
  .row { display: flex; justify-content: space-between; gap: 12px; padding: 3px 0; }
  .label { color: var(--text-dim, #9aa3b2); }
  .value { text-align: right; word-break: break-all; }
  .actions { display: flex; gap: 8px; margin-top: 12px; }
  button {
    font: inherit;
    padding: 6px 12px;
    border-radius: 6px;
    border: 1px solid var(--border, #2c3240);
    background: var(--bg-3, #1e232d);
    color: inherit;
    cursor: pointer;
  }
  button:hover:not(:disabled) { border-color: var(--primary, #4a90d9); }
  button:disabled { opacity: .5; cursor: default; }
  progress { width: 100%; margin-top: 10px; height: 8px; }
  code { font-family: ui-monospace, Consolas, monospace; font-size: 12px; }
`;
  var GROUP_LABEL = {
    runtime: "\u0414\u0432\u0438\u0436\u043E\u043A",
    model: "\u041C\u043E\u0434\u0435\u043B\u044C",
    tokenizer: "\u0422\u043E\u043A\u0435\u043D\u0438\u0437\u0430\u0442\u043E\u0440"
  };
  function humanSize(bytes) {
    if (bytes <= 0) return "\u2014";
    if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} \u041C\u0411`;
    if (bytes >= 1024) return `${(bytes / 1024).toFixed(0)} \u041A\u0411`;
    return `${bytes} \u0411`;
  }
  function esc(value) {
    return value.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
  }
  var System1Panel = class extends HTMLElement {
    constructor() {
      super(...arguments);
      this.status = null;
      this.busy = false;
    }
    connectedCallback() {
      this.attachShadow({ mode: "open" });
      this.shadowRoot.appendChild(this.buildStyle());
      this.shadowRoot.appendChild(this.buildBody());
      void this.refresh();
      void onDownloadProgress((progress) => this.onProgress(progress));
    }
    disconnectedCallback() {
      this.shadowRoot?.replaceChildren();
    }
    buildStyle() {
      const element = document.createElement("style");
      element.textContent = STYLE;
      return element;
    }
    buildBody() {
      const card = document.createElement("div");
      card.className = "card";
      card.innerHTML = `
      <h3>\u041C\u043E\u0434\u0435\u043B\u044C \u0431\u044B\u0441\u0442\u0440\u044B\u0445 \u0440\u0435\u0448\u0435\u043D\u0438\u0439 (System-1 / Laya)</h3>
      <p class="lead">
        \u0420\u0430\u0431\u043E\u0442\u0430\u0435\u0442 \u0431\u0435\u0437 CUDA \u0438 DirectML \u2014 \u0442\u043E\u043B\u044C\u043A\u043E CPU. \u0424\u0430\u0439\u043B\u044B \u043D\u0435 \u0432\u0445\u043E\u0434\u044F\u0442 \u0432 \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u0449\u0438\u043A,
        \u0430 \u0441\u043A\u0430\u0447\u0438\u0432\u0430\u044E\u0442\u0441\u044F \u0441 \u0441\u0435\u0440\u0432\u0435\u0440\u0430 \u0438 \u043F\u0440\u043E\u0432\u0435\u0440\u044F\u044E\u0442\u0441\u044F \u043F\u043E \u043A\u043E\u043D\u0442\u0440\u043E\u043B\u044C\u043D\u043E\u0439 \u0441\u0443\u043C\u043C\u0435.
        \u0412\u0441\u0451 \u0445\u0440\u0430\u043D\u0438\u0442\u0441\u044F \u0432 \u0434\u0430\u043D\u043D\u044B\u0445 \u043F\u0440\u0438\u043B\u043E\u0436\u0435\u043D\u0438\u044F \u0438 \u043D\u0435 \u0437\u0430\u043D\u0438\u043C\u0430\u0435\u0442 \u043C\u0435\u0441\u0442\u043E \u0440\u044F\u0434\u043E\u043C \u0441 \u043F\u0440\u043E\u0433\u0440\u0430\u043C\u043C\u043E\u0439.
      </p>
      <div id="files"><div class="muted">\u041F\u0440\u043E\u0432\u0435\u0440\u043A\u0430 \u0444\u0430\u0439\u043B\u043E\u0432\u2026</div></div>
      <progress id="progress" max="100" value="0" hidden></progress>
      <div id="summary"></div>
      <div class="actions">
        <button id="refresh">\u041F\u0440\u043E\u0432\u0435\u0440\u0438\u0442\u044C</button>
        <button id="download">\u0421\u043A\u0430\u0447\u0430\u0442\u044C \u0432\u0441\u0451</button>
        <button id="remove">\u0423\u0434\u0430\u043B\u0438\u0442\u044C \u043C\u043E\u0434\u0435\u043B\u044C</button>
      </div>
    `;
      card.querySelector("#refresh").addEventListener("click", () => void this.refresh());
      card.querySelector("#download").addEventListener("click", () => void this.download());
      card.querySelector("#remove").addEventListener("click", () => void this.deleteModel());
      return card;
    }
    onProgress(progress) {
      const bar = this.shadowRoot?.querySelector("#progress");
      if (!bar || !progress.total) return;
      bar.hidden = false;
      bar.value = (progress.downloaded ?? 0) / progress.total * 100;
    }
    async refresh() {
      try {
        this.status = await getStatus();
        this.render();
      } catch (error) {
        logPlugin(`[system1] \u0441\u0442\u0430\u0442\u0443\u0441 \u043D\u0435 \u043F\u043E\u043B\u0443\u0447\u0435\u043D: ${String(error)}`);
        toast(`\u041D\u0435 \u0443\u0434\u0430\u043B\u043E\u0441\u044C \u043F\u043E\u043B\u0443\u0447\u0438\u0442\u044C \u0441\u0442\u0430\u0442\u0443\u0441 System-1: ${String(error)}`, "error");
      }
    }
    async download() {
      if (this.busy) return;
      this.busy = true;
      this.setButtons();
      const missing = this.status?.files.filter((file) => !file.present) ?? [];
      const what = missing.length ? ` (${missing.length} \u0438\u0437 ${this.status?.files.length ?? 0})` : "";
      try {
        await downloadAll();
        toast("\u0424\u0430\u0439\u043B\u044B System-1 \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D\u044B");
        logPlugin(`[system1] \u043A\u043E\u043C\u043F\u043B\u0435\u043A\u0442 \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D${what}`);
        await this.refresh();
      } catch (error) {
        logPlugin(`[system1] \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043A\u0430 \u043D\u0435 \u0443\u0434\u0430\u043B\u0430\u0441\u044C: ${String(error)}`);
        toast(`\u041D\u0435 \u0443\u0434\u0430\u043B\u043E\u0441\u044C \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u0438\u0442\u044C \u0444\u0430\u0439\u043B\u044B System-1: ${String(error)}`, "error");
      } finally {
        this.busy = false;
        this.setButtons();
      }
    }
    async deleteModel() {
      if (this.busy) return;
      this.busy = true;
      this.setButtons();
      try {
        await removeModel();
        toast("\u041C\u043E\u0434\u0435\u043B\u044C System-1 \u0443\u0434\u0430\u043B\u0435\u043D\u0430");
        logPlugin("[system1] \u043C\u043E\u0434\u0435\u043B\u044C \u0443\u0434\u0430\u043B\u0435\u043D\u0430");
        await this.refresh();
      } catch (error) {
        logPlugin(`[system1] \u0443\u0434\u0430\u043B\u0435\u043D\u0438\u0435 \u043D\u0435 \u0443\u0434\u0430\u043B\u043E\u0441\u044C: ${String(error)}`);
        toast(`\u041D\u0435 \u0443\u0434\u0430\u043B\u043E\u0441\u044C \u0443\u0434\u0430\u043B\u0438\u0442\u044C \u043C\u043E\u0434\u0435\u043B\u044C: ${String(error)}`, "error");
      } finally {
        this.busy = false;
        this.setButtons();
      }
    }
    setButtons() {
      for (const id of ["#refresh", "#download", "#remove"]) {
        const button = this.shadowRoot?.querySelector(id);
        if (button) button.disabled = this.busy;
      }
    }
    /** Одна строка таблицы файлов. */
    renderFile(file) {
      const state = file.present ? '<span class="ok">\u0433\u043E\u0442\u043E\u0432</span>' : '<span class="bad">\u043D\u0435\u0442</span>';
      const detail = file.present ? esc(file.action) : `<span class="muted">${esc(file.action)}</span>`;
      return `
      <tr>
        <td><code>${esc(file.name)}</code><div class="muted">${detail}</div></td>
        <td class="size">${humanSize(file.sizeBytes)}</td>
        <td class="size">${state}</td>
      </tr>
    `;
    }
    render() {
      const filesTarget = this.shadowRoot?.querySelector("#files");
      const summaryTarget = this.shadowRoot?.querySelector("#summary");
      const status = this.status;
      if (!filesTarget || !summaryTarget || !status) return;
      let previousKind = "";
      const body = status.files.map((file) => {
        const header = file.kind !== previousKind ? `<tr><td class="group" colspan="3">${esc(GROUP_LABEL[file.kind] ?? file.kind)}</td></tr>` : "";
        previousKind = file.kind;
        return header + this.renderFile(file);
      }).join("");
      filesTarget.innerHTML = `
      <table>
        <thead>
          <tr><th>\u0424\u0430\u0439\u043B</th><th class="size">\u0420\u0430\u0437\u043C\u0435\u0440</th><th class="size">\u0421\u043E\u0441\u0442\u043E\u044F\u043D\u0438\u0435</th></tr>
        </thead>
        <tbody>${body}</tbody>
      </table>
    `;
      const allPresent = status.files.every((file) => file.present);
      const missing = status.files.filter((file) => !file.present);
      const verdict = allPresent ? '<span class="ok">\u0432\u0441\u0435 \u0444\u0430\u0439\u043B\u044B \u043D\u0430 \u043C\u0435\u0441\u0442\u0435, \u043C\u043E\u0436\u043D\u043E \u0440\u0430\u0431\u043E\u0442\u0430\u0442\u044C</span>' : `<span class="bad">\u043D\u0435 \u0445\u0432\u0430\u0442\u0430\u0435\u0442: ${esc(missing.map((file) => file.name).join(", "))}</span>`;
      summaryTarget.innerHTML = `
      <div class="row" style="margin-top:10px">
        <span class="label">\u0413\u043E\u0442\u043E\u0432\u043D\u043E\u0441\u0442\u044C</span><span class="value">${verdict}</span>
      </div>
      <div class="row">
        <span class="label">\u0417\u0430\u0433\u0440\u0443\u0436\u0435\u043D\u0430 \u0432 \u043F\u0430\u043C\u044F\u0442\u044C</span>
        <span class="value">${status.loaded ? "\u0434\u0430" : "\u043D\u0435\u0442"}</span>
      </div>
      <div class="row">
        <span class="label">\u0423\u0441\u0442\u0440\u043E\u0439\u0441\u0442\u0432\u043E</span>
        <span class="value">${status.device === "cpu" ? "CPU" : esc(status.device)}</span>
      </div>
      <div class="row">
        <span class="label">\u0414\u0432\u0438\u0436\u043E\u043A</span>
        <span class="value"><code>${esc(status.runtime.version)}</code></span>
      </div>
      <div class="row">
        <span class="label">\u041F\u0430\u043F\u043A\u0430 \u0434\u0430\u043D\u043D\u044B\u0445</span>
        <span class="value"><code>${esc(status.modelsDir)}</code></span>
      </div>
    `;
    }
  };
  if (!customElements.get("system1-panel")) {
    customElements.define("system1-panel", System1Panel);
  }

  // guest-js/index.ts
  function getStatus() {
    return invoke("plugin:system1|get_status");
  }
  function downloadAll() {
    return invoke("plugin:system1|download_all");
  }
  function downloadModel(modelId) {
    return invoke("plugin:system1|download_model", { modelId });
  }
  function removeModel(modelId) {
    return invoke("plugin:system1|remove_model", { modelId });
  }
  function decide(request) {
    return invoke("plugin:system1|decide", { request });
  }
  function onDownloadProgress(cb) {
    return listen("downloader:progress", (event) => cb(event.payload));
  }

  // guest-js/iife-entry.ts
  var g3 = window;
  if ("__TAURI__" in window) {
    const tauri = g3.__TAURI__;
    if (tauri && !tauri["system1"]) {
      Object.defineProperty(tauri, "system1", {
        configurable: true,
        value: {
          decide,
          downloadModel,
          getStatus,
          onDownloadProgress,
          removeModel
        }
      });
    }
  }
})();
