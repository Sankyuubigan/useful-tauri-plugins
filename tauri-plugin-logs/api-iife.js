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
    const ev = g2.__TAURI__?.event;
    if (!ev?.listen) {
      throw new Error("window.__TAURI__.event.listen \u043D\u0435\u0434\u043E\u0441\u0442\u0443\u043F\u0435\u043D");
    }
    return ev.listen(event, handler);
  }

  // guest-js/shims/dialog.ts
  var g3 = window;
  async function save(opts) {
    const dlg = g3.__TAURI__?.dialog;
    if (!dlg?.save) return null;
    return dlg.save(opts);
  }

  // guest-js/web-components.ts
  var LogsPanel = class extends HTMLElement {
    constructor() {
      super(...arguments);
      this.unlisten = null;
      this.lines = [];
      this.nearBottom = true;
      this.maxLines = 4e3;
    }
    connectedCallback() {
      if (!this.shadowRoot) {
        this.root = this.attachShadow({ mode: "open" });
      } else {
        this.root = this.shadowRoot;
      }
      this.render();
    }
    disconnectedCallback() {
      this.unlisten?.();
      this.unlisten = null;
    }
    render() {
      this.root.innerHTML = `
      <style>
        :host { display: flex; flex-direction: column; height: 100%; min-height: 0;
               font-family: var(--font, system-ui, sans-serif); }
        .toolbar { display: flex; align-items: center; gap: 8px; padding: 6px 10px;
                   background: var(--logs-toolbar-bg, #2a2d2e);
                   border-bottom: 1px solid var(--border, #3a3d3e);
                   border-top-left-radius: 8px; border-top-right-radius: 8px; }
        .title { font-weight: 600; color: var(--text, #e0e0e0); font-size: 13px; white-space: nowrap; }
        .hint { color: var(--text-muted, #9a9a9a); font-size: 11px; overflow: hidden;
                text-overflow: ellipsis; white-space: nowrap; }
        .spacer { flex: 1; }
        button { font: inherit; font-size: 12px; padding: 4px 10px; cursor: pointer;
                 border: 1px solid var(--border, #3a3d3e); border-radius: 6px;
                 background: var(--logs-btn-bg, #3a3d3e); color: var(--text, #e0e0e0); }
        button:hover { filter: brightness(1.15); }
        textarea { flex: 1; resize: none; width: 100%; box-sizing: border-box; border: none;
                   padding: 8px; background: var(--logs-area-bg, #1e1e1e);
                   color: var(--logs-area-text, #d4d4d4);
                   font-family: var(--mono-font, Consolas, monospace); font-size: 12px;
                   line-height: 1.45; outline: none;
                   border-bottom-left-radius: 8px; border-bottom-right-radius: 8px; }
      </style>
      <div class="toolbar">
        <span class="title">\u041B\u043E\u0433\u0438 \u0441\u0438\u0441\u0442\u0435\u043C\u044B</span>
        <span class="hint" id="hint"></span>
        <div class="spacer"></div>
        <button id="copy">\u041A\u043E\u043F\u0438\u0440\u043E\u0432\u0430\u0442\u044C</button>
        <button id="save">\u0421\u043E\u0445\u0440\u0430\u043D\u0438\u0442\u044C</button>
        <button id="clear">\u041E\u0447\u0438\u0441\u0442\u0438\u0442\u044C</button>
      </div>
      <textarea id="area" readonly spellcheck="false" placeholder="\u0417\u0434\u0435\u0441\u044C \u043F\u043E\u044F\u0432\u0438\u0442\u0441\u044F \u043B\u043E\u0433 \u043F\u0440\u0438\u043B\u043E\u0436\u0435\u043D\u0438\u044F\u2026"></textarea>`;
      const area = this.root.getElementById("area");
      area.addEventListener("scroll", () => {
        this.nearBottom = area.scrollHeight - area.scrollTop - area.clientHeight < 40;
      });
      this.root.getElementById("copy").addEventListener("click", () => {
        void this.onCopy();
      });
      this.root.getElementById("save").addEventListener("click", () => {
        void this.onSave();
      });
      this.root.getElementById("clear").addEventListener("click", () => {
        this.onClear();
      });
      void getLastLogsPath().then((p) => {
        if (p) this.root.getElementById("hint").textContent = `\u0424\u0430\u0439\u043B: ${p}`;
      }).catch(() => {
      });
      void onLogMessage((line) => this.appendLine(line)).then((u) => {
        this.unlisten = u;
      }).catch((e) => {
        logFront(`[logs-panel] \u043D\u0435 \u0443\u0434\u0430\u043B\u043E\u0441\u044C \u043F\u043E\u0434\u043F\u0438\u0441\u0430\u0442\u044C\u0441\u044F \u043D\u0430 \u043B\u043E\u0433: ${String(e)}`);
      });
    }
    appendLine(line) {
      this.lines.push(line);
      const area = this.root.getElementById("area");
      if (!area) return;
      let trimmed = false;
      if (this.lines.length > this.maxLines) {
        const overflow = this.lines.length - this.maxLines;
        this.lines.splice(0, overflow);
        trimmed = true;
      }
      if (trimmed) {
        area.value = this.lines.join("\n");
      } else {
        area.value += `${line}
`;
      }
      if (this.nearBottom) {
        area.scrollTop = area.scrollHeight;
      }
    }
    onClear() {
      this.lines = [];
      this.nearBottom = true;
      const area = this.root.getElementById("area");
      if (area) {
        area.value = "";
        area.scrollTop = 0;
      }
    }
    async onCopy() {
      const text = this.lines.join("\n");
      try {
        await navigator.clipboard.writeText(text);
        logFront(`[logs-panel] \u043B\u043E\u0433\u0438 \u0441\u043A\u043E\u043F\u0438\u0440\u043E\u0432\u0430\u043D\u044B \u0432 \u0431\u0443\u0444\u0435\u0440 \u043E\u0431\u043C\u0435\u043D\u0430 (${this.lines.length} \u0441\u0442\u0440\u043E\u043A)`);
      } catch (e) {
        logFront(`[logs-panel] \u043D\u0435 \u0443\u0434\u0430\u043B\u043E\u0441\u044C \u0441\u043A\u043E\u043F\u0438\u0440\u043E\u0432\u0430\u0442\u044C \u043B\u043E\u0433\u0438: ${String(e)}`);
      }
    }
    async onSave() {
      const content = this.lines.join("\n");
      const saved = await saveLogsToFile(content).catch(() => null);
      if (saved) logFront(`[logs-panel] \u043B\u043E\u0433\u0438 \u0441\u043E\u0445\u0440\u0430\u043D\u0435\u043D\u044B \u0432 ${saved}`);
    }
  };
  if (!customElements.get("logs-panel")) {
    customElements.define("logs-panel", LogsPanel);
  }

  // guest-js/index.ts
  function onLogMessage(cb) {
    return listen("logs:message", (e) => cb(e.payload));
  }
  function getLastLogsPath() {
    return invoke("plugin:logs|get_last_logs_path");
  }
  function logFront(msg) {
    void invoke("plugin:logs|log_frontend_event", { level: "FE", msg }).catch(() => {
    });
  }
  async function logFrontendEvent(level, msg) {
    try {
      await invoke("plugin:logs|log_frontend_event", { level, msg });
    } catch {
    }
  }
  async function trackError(report) {
    try {
      await invoke("plugin:logs|track_error", {
        errorType: report.errorType,
        message: report.message,
        stack: report.stack ?? null,
        severity: report.severity ?? "error",
        kind: report.kind ?? "handled",
        breadcrumbs: [...breadcrumbs]
      });
    } catch {
    }
  }
  function defaultLogFilename() {
    const d = /* @__PURE__ */ new Date();
    const pad = (n) => String(n).padStart(2, "0");
    const ts = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}_${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}`;
    return `logs_${ts}.txt`;
  }
  async function saveLogsToFile(content) {
    const path = await save({
      title: "\u0421\u043E\u0445\u0440\u0430\u043D\u0438\u0442\u044C \u043B\u043E\u0433\u0438",
      defaultPath: defaultLogFilename(),
      filters: [{ name: "\u0422\u0435\u043A\u0441\u0442", extensions: ["log", "txt"] }]
    });
    if (!path) return null;
    try {
      await invoke("plugin:logs|save_logs_file", { path, content });
      return path;
    } catch {
      return null;
    }
  }
  var captureAttached = false;
  var BREADCRUMB_LIMIT = 100;
  var breadcrumbs = [];
  function stamp() {
    return (/* @__PURE__ */ new Date()).toISOString().slice(11, 19);
  }
  function pushBreadcrumb(kind, detail) {
    if (breadcrumbs.length >= BREADCRUMB_LIMIT) breadcrumbs.shift();
    breadcrumbs.push(`[${stamp()}] [${kind}] ${detail}`);
  }
  function takeBreadcrumbs() {
    return breadcrumbs.splice(0, breadcrumbs.length);
  }
  function truncate(s, n) {
    return s.length > n ? `${s.slice(0, n)}\u2026` : s;
  }
  function installInvokeInterceptor() {
    const g5 = window;
    const MARKER = "__logs_breadcrumb_wrapped";
    const patch = (proto, key) => {
      if (!proto) return;
      const orig = proto[key];
      if (typeof orig !== "function" || orig[MARKER]) return;
      const wrapped = async (...args) => {
        const cmd = args[0];
        const name = typeof cmd === "string" ? cmd : String(cmd);
        const skip = name.startsWith("plugin:logs|");
        const start = performance.now();
        if (!skip) {
          const payload = truncate(JSON.stringify(args[1] ?? {}), 300);
          pushBreadcrumb("IPC START", `${name} ${payload}`);
        }
        try {
          const result = await orig.apply(proto, args);
          if (!skip) {
            const ms = (performance.now() - start).toFixed(1);
            pushBreadcrumb("IPC OK", `${name} (${ms}ms)`);
          }
          return result;
        } catch (err) {
          const ms = (performance.now() - start).toFixed(1);
          const emsg = err instanceof Error ? err.message : String(err ?? "unknown");
          if (!skip) {
            pushBreadcrumb("IPC ERROR", `${name} (${ms}ms) ${emsg}`);
            logFront(`[IPC ERROR] ${name} (${ms}ms) ${emsg}`);
          }
          throw err;
        }
      };
      wrapped[MARKER] = true;
      proto[key] = wrapped;
    };
    try {
      patch(g5.__TAURI_INTERNALS__, "invoke");
      patch(g5.__TAURI__?.core, "invoke");
    } catch {
    }
  }
  function installClickCapture() {
    document.addEventListener(
      "click",
      (e) => {
        const target = e.target;
        if (!target) return;
        const id = target.id ? `#${target.id}` : "";
        const cls = typeof target.className === "string" && target.className ? `.${target.className.split(" ")[0]}` : "";
        const text = (target.textContent ?? "").trim().slice(0, 40);
        pushBreadcrumb("UI ACTION", `<${target.tagName.toLowerCase()}${id}${cls}> ${truncate(text, 40)}`);
      },
      true
    );
  }
  async function flushFrontendCrash(errorType, message, stack) {
    const crumbs = takeBreadcrumbs();
    logFront(`[window error] ${message}`);
    try {
      await invoke("plugin:logs|dump_frontend_error", {
        errorType,
        message,
        stack: stack ?? null,
        breadcrumbs: crumbs
      });
    } catch {
    }
  }
  function initFrontendErrorCapture() {
    if (captureAttached) return;
    captureAttached = true;
    installInvokeInterceptor();
    installClickCapture();
    window.addEventListener("error", (e) => {
      const msg = e.message ?? "unknown error";
      const stack = e.error instanceof Error ? e.error.stack : void 0;
      void flushFrontendCrash("Frontend Error", msg, stack);
    });
    window.addEventListener("unhandledrejection", (e) => {
      const reason = e.reason;
      let message = String(reason ?? "unknown rejection");
      let stack;
      if (reason instanceof Error) {
        message = reason.message;
        stack = reason.stack;
      }
      void flushFrontendCrash("Unhandled Promise Rejection", message, stack);
    });
  }

  // guest-js/iife-entry.ts
  var g4 = window;
  if ("__TAURI__" in window) {
    const tauri = g4.__TAURI__;
    if (tauri && !tauri.logs) {
      Object.defineProperty(tauri, "logs", {
        configurable: true,
        value: { getLastLogsPath, logFront, logFrontendEvent, onLogMessage, trackError }
      });
    }
    initFrontendErrorCapture();
  }
})();
