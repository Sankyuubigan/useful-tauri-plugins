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
  async function listen(event, handler) {
    const tauriEvent = g2.__TAURI__?.event;
    if (!tauriEvent?.listen) {
      throw new Error("window.__TAURI__.event.listen \u043D\u0435\u0434\u043E\u0441\u0442\u0443\u043F\u0435\u043D");
    }
    const unlisten = await tauriEvent.listen(event, handler);
    return typeof unlisten === "function" ? unlisten : () => {
    };
  }

  // guest-js/web-components.ts
  var STATUS_LABEL = {
    running: "\u0421\u043A\u0430\u0447\u0438\u0432\u0430\u043D\u0438\u0435\u2026",
    done: "\u0413\u043E\u0442\u043E\u0432\u043E",
    error: "\u041E\u0448\u0438\u0431\u043A\u0430",
    cancelled: "\u041E\u0442\u043C\u0435\u043D\u0435\u043D\u043E"
  };
  function formatBytes(n) {
    if (!Number.isFinite(n) || n <= 0) return "0 B";
    const units = ["B", "KB", "MB", "GB"];
    let i = 0;
    let v = n;
    while (v >= 1024 && i < units.length - 1) {
      v /= 1024;
      i++;
    }
    return `${v >= 10 || i === 0 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
  }
  function formatSpeed(bps) {
    if (!Number.isFinite(bps) || bps <= 0) return "\u2014";
    return `${formatBytes(bps)}/s`;
  }
  function formatEta(sec) {
    if (!Number.isFinite(sec) || sec < 0) return "\u2014";
    if (sec < 60) return `${Math.max(1, Math.round(sec))} \u0441`;
    const m = Math.floor(sec / 60);
    const s = Math.round(sec % 60);
    if (m < 60) return `${m} \u043C\u0438\u043D ${s} \u0441`;
    const h = Math.floor(m / 60);
    return `${h} \u0447 ${m % 60} \u043C\u0438\u043D`;
  }
  function pct(p) {
    if (!p.total || p.total <= 0) return 0;
    return Math.min(100, Math.max(0, p.downloaded / p.total * 100));
  }
  function createRowsController(render) {
    const rows = /* @__PURE__ */ new Map();
    let unlisten = null;
    let hideTimer = null;
    const purgeTerminal = () => {
      const now = Date.now();
      for (const [id, row] of rows) {
        if (row.p.status !== "running" && now - row.terminalAt > 4e3) {
          rows.delete(id);
        }
      }
      if (rows.size === 0 && hideTimer !== null) {
        window.clearTimeout(hideTimer);
        hideTimer = null;
      }
      render(rows);
    };
    const ensureListen = () => {
      if (unlisten) return;
      void onProgress((p) => {
        const prev = rows.get(p.task_id);
        if (p.status === "running") {
          rows.set(p.task_id, { p, terminalAt: 0 });
          if (hideTimer !== null) {
            window.clearTimeout(hideTimer);
            hideTimer = null;
          }
        } else {
          rows.set(p.task_id, { p, terminalAt: Date.now() });
          if (hideTimer === null) {
            hideTimer = window.setTimeout(() => {
              hideTimer = null;
              purgeTerminal();
            }, 4200);
          }
        }
        void prev;
        render(rows);
      }).then((fn) => {
        unlisten = fn;
      });
    };
    const destroy = () => {
      if (hideTimer !== null) window.clearTimeout(hideTimer);
      if (unlisten) {
        unlisten();
        unlisten = null;
      }
      rows.clear();
      render(rows);
    };
    return { ensureListen, destroy, rows };
  }
  function escapeHtml(s) {
    return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
  }
  function rowsHtml(rows, compact) {
    if (rows.size === 0) return "";
    let html = "";
    for (const { p } of rows.values()) {
      const percent = pct(p);
      const known = p.total > 0;
      const statusText = STATUS_LABEL[p.status] ?? p.status;
      const meta = known ? `${formatBytes(p.downloaded)} / ${formatBytes(p.total)} \xB7 ${formatSpeed(p.speed_bps)} \xB7 \u043E\u0441\u0442\u0430\u043B\u043E\u0441\u044C ${formatEta(p.eta_s)}` : `${formatBytes(p.downloaded)} \xB7 ${formatSpeed(p.speed_bps)}`;
      html += `<div class="dl-row${p.status !== "running" ? ` dl-${p.status}` : ""}" data-task="${escapeHtml(p.task_id)}">
      <div class="dl-head">
        <span class="dl-label" title="${escapeHtml(p.label)}">${escapeHtml(p.label)}</span>
        <span class="dl-meta">
          <span class="dl-level">${escapeHtml(p.level_name || statusText)}</span>
          ${known ? `<span class="dl-pct">${percent.toFixed(0)}%</span>` : ""}
          <span class="dl-status">${escapeHtml(p.status === "running" ? statusText : p.message || statusText)}</span>
        </span>
      </div>
      <div class="dl-bar"><div class="dl-fill" style="width:${known ? percent.toFixed(1) : 0}%"></div></div>
      ${compact ? "" : `<div class="dl-sub">${escapeHtml(meta)}</div>`}
      ${p.status === "running" ? `<button type="button" class="dl-cancel" data-cancel="${escapeHtml(p.task_id)}" title="\u041E\u0442\u043C\u0435\u043D\u0438\u0442\u044C">\xD7</button>` : ""}
    </div>`;
    }
    return html;
  }
  var HOST_STYLE = `
:host { display: block; color: inherit; font: inherit; }
.dl-list { display: flex; flex-direction: column; gap: 8px; }
.dl-row {
  position: relative;
  background: color-mix(in srgb, Canvas 92%, CanvasText 8%);
  border: 1px solid color-mix(in srgb, CanvasText 18%, transparent);
  border-radius: 8px;
  padding: 8px 34px 8px 10px;
}
.dl-head { display: flex; justify-content: space-between; gap: 8px; align-items: baseline; }
.dl-label { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.dl-meta { display: flex; gap: 8px; font-size: 0.85em; opacity: 0.75; flex-shrink: 0; }
.dl-bar {
  margin-top: 6px;
  height: 6px;
  border-radius: 4px;
  overflow: hidden;
  background: color-mix(in srgb, CanvasText 12%, transparent);
}
.dl-fill {
  height: 100%;
  background: #3b82f6;
  transition: width 150ms linear;
}
.dl-done .dl-fill { background: #22c55e; }
.dl-error .dl-fill { background: #ef4444; }
.dl-cancelled .dl-fill { background: #a3a3a3; }
.dl-sub { margin-top: 4px; font-size: 0.8em; opacity: 0.65; }
.dl-cancel {
  position: absolute;
  top: 6px;
  right: 6px;
  width: 22px;
  height: 22px;
  border: none;
  border-radius: 50%;
  cursor: pointer;
  background: transparent;
  color: inherit;
  font-size: 16px;
  line-height: 1;
  opacity: 0.55;
}
.dl-cancel:hover { opacity: 1; background: color-mix(in srgb, CanvasText 10%, transparent); }
`;
  function attachCancelClicks(host, onId) {
    host.addEventListener("click", (e) => {
      const btn = e.target?.closest("[data-cancel]");
      const id = btn?.dataset?.cancel;
      if (id) onId(id);
    });
  }
  var DownloaderWidget = class extends HTMLElement {
    constructor() {
      super(...arguments);
      this.ctrl = null;
      this.shadow = null;
    }
    connectedCallback() {
      if (!this.shadow) this.shadow = this.attachShadow({ mode: "open" });
      if (this.ctrl) return;
      const style = document.createElement("style");
      style.textContent = HOST_STYLE + `
:host { position: fixed; right: 16px; bottom: 16px; z-index: 9999; max-width: min(420px, calc(100vw - 32px)); }
.dl-list { min-width: 280px; }
`;
      this.shadow.appendChild(style);
      const list = document.createElement("div");
      list.className = "dl-list";
      this.shadow.appendChild(list);
      attachCancelClicks(this.shadow, (id) => void cancelDownload(id));
      this.ctrl = createRowsController((rows) => {
        list.innerHTML = rowsHtml(rows, true);
        this.style.display = rows.size === 0 ? "none" : "";
      });
      this.style.display = "none";
      this.ctrl.ensureListen();
    }
    disconnectedCallback() {
      this.ctrl?.destroy();
      this.ctrl = null;
      if (this.shadow) this.shadow.innerHTML = "";
    }
  };
  var DownloaderProgress = class extends HTMLElement {
    constructor() {
      super(...arguments);
      this.ctrl = null;
      this.shadow = null;
      /** Через атрибут kind фильтруются категории ("engine,model"). */
      this.filterKinds = [];
    }
    static get observedAttributes() {
      return ["kind", "label"];
    }
    connectedCallback() {
      if (!this.shadow) this.shadow = this.attachShadow({ mode: "open" });
      if (this.ctrl) return;
      const style = document.createElement("style");
      style.textContent = HOST_STYLE;
      this.shadow.appendChild(style);
      const list = document.createElement("div");
      list.className = "dl-list";
      this.shadow.appendChild(list);
      attachCancelClicks(this.shadow, (id) => void cancelDownload(id));
      this.ctrl = createRowsController((rows) => {
        const filtered = this.filterRows(rows);
        list.innerHTML = filtered.size === 0 ? '<div class="dl-empty" style="opacity:.55;font-size:.9em">\u041D\u0435\u0442 \u0430\u043A\u0442\u0438\u0432\u043D\u044B\u0445 \u0437\u0430\u0433\u0440\u0443\u0437\u043E\u043A</div>' : rowsHtml(filtered, false);
      });
      this.ctrl.ensureListen();
      this.ctrl && this.render();
    }
    attributeChangedCallback() {
      this.syncFilter();
      this.render();
    }
    disconnectedCallback() {
      this.ctrl?.destroy();
      this.ctrl = null;
      if (this.shadow) this.shadow.innerHTML = "";
    }
    syncFilter() {
      const kind = this.getAttribute("kind");
      this.filterKinds = kind ? kind.split(",").map((s) => s.trim()).filter(Boolean) : [];
    }
    filterRows(rows) {
      const label = this.getAttribute("label")?.trim();
      if (!this.filterKinds.length && !label) return rows;
      const out = /* @__PURE__ */ new Map();
      for (const [id, row] of rows) {
        const kindOk = !this.filterKinds.length || this.filterKinds.includes(row.p.kind);
        const labelOk = !label || row.p.label === label;
        if (kindOk && labelOk) out.set(id, row);
      }
      return out;
    }
    render() {
      if (!this.ctrl) return;
      const list = this.shadow?.querySelector(".dl-list");
      if (list) list.innerHTML = rowsHtml(this.filterRows(this.ctrl.rows), false);
    }
  };
  if (typeof customElements !== "undefined" && !customElements.get("downloader-widget")) {
    customElements.define("downloader-widget", DownloaderWidget);
  }
  if (typeof customElements !== "undefined" && !customElements.get("downloader-progress")) {
    customElements.define("downloader-progress", DownloaderProgress);
  }

  // guest-js/index.ts
  function downloadFile(url, dest, opts) {
    return invoke("plugin:downloader|download_file", { url, dest, opts: opts ?? null });
  }
  function downloadBytes(url, opts) {
    return invoke("plugin:downloader|download_bytes", { url, opts: opts ?? null });
  }
  function cancelDownload(taskId) {
    return invoke("plugin:downloader|cancel_download", { taskId });
  }
  function listActive() {
    return invoke("plugin:downloader|list_active");
  }
  function onProgress(cb) {
    return listen("downloader:progress", (e) => cb(e.payload));
  }

  // guest-js/iife-entry.ts
  var g3 = window;
  if ("__TAURI__" in window) {
    const tauri = g3.__TAURI__;
    if (tauri && !tauri.downloader) {
      Object.defineProperty(tauri, "downloader", {
        configurable: true,
        value: { downloadFile, downloadBytes, cancelDownload, listActive, onProgress }
      });
    }
  }
})();
