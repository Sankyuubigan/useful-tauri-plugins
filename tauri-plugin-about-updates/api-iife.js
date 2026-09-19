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

  // guest-js/shims/updater.ts
  var g2 = window;
  async function check(options) {
    const updater = g2.__TAURI__?.updater;
    if (!updater?.check) {
      throw new Error("window.__TAURI__.updater.check \u043D\u0435\u0434\u043E\u0441\u0442\u0443\u043F\u0435\u043D");
    }
    return updater.check(options);
  }

  // guest-js/shims/shell.ts
  var g3 = window;
  async function open(path, openWith) {
    const shell = g3.__TAURI__?.shell;
    if (!shell?.open) {
      throw new Error("window.__TAURI__.shell.open \u043D\u0435\u0434\u043E\u0441\u0442\u0443\u043F\u0435\u043D");
    }
    return shell.open(path, openWith);
  }

  // guest-js/web-components.ts
  var AboutUpdatesPanel = class extends HTMLElement {
    constructor() {
      super(...arguments);
      this.repo = "";
      this.busy = false;
      this.historyLoaded = false;
      this.historyOpen = false;
    }
    static get observedAttributes() {
      return ["repo"];
    }
    connectedCallback() {
      this.repo = this.getAttribute("repo") ?? "";
      this.root = this.attachShadow({ mode: "open" });
      this.render();
    }
    attributeChangedCallback() {
      this.repo = this.getAttribute("repo") ?? "";
      if (this.root) this.render();
    }
    setStatus(s) {
      const el = this.root.getElementById("status");
      if (el) el.textContent = s;
    }
    async render() {
      if (!this.root) return;
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
        <div><strong>\u0412\u0435\u0440\u0441\u0438\u044F:</strong> <span id="ver">\u2026</span></div>
        <div class="row">
          <button id="check" class="primary">\u041F\u0440\u043E\u0432\u0435\u0440\u0438\u0442\u044C \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u044F</button>
          <button id="rollback">\u0418\u0441\u0442\u043E\u0440\u0438\u044F / \u043E\u0442\u043A\u0430\u0442</button>
        </div>
        <div id="status" class="muted" style="margin-top:8px"></div>
        <div id="history" style="margin-top:8px" hidden></div>
      </div>`;
      const ver = await getAppVersion().catch(() => "?");
      this.root.getElementById("ver").textContent = ver;
      const supportUrl = await getSupportUrl().catch(() => null);
      if (supportUrl) {
        const sup = document.createElement("button");
        sup.textContent = "\u041F\u043E\u0434\u0434\u0435\u0440\u0436\u0430\u0442\u044C \u0430\u0432\u0442\u043E\u0440\u0430";
        sup.addEventListener("click", () => {
          open(supportUrl).catch(() => window.open(supportUrl, "_blank"));
        });
        this.root.getElementById("rollback").parentElement.appendChild(sup);
      }
      this.root.getElementById("check").addEventListener("click", () => this.onCheck());
      this.root.getElementById("rollback").addEventListener("click", () => this.onRollback());
    }
    async onCheck() {
      if (this.busy) return;
      this.busy = true;
      this.setStatus("\u041F\u0440\u043E\u0432\u0435\u0440\u043A\u0430\u2026");
      try {
        const update = await checkForUpdate();
        if (update) {
          this.setStatus(`\u0414\u043E\u0441\u0442\u0443\u043F\u043D\u043E \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u0435 ${update.version}. \u0423\u0441\u0442\u0430\u043D\u043E\u0432\u043A\u0430\u2026`);
          await downloadAndInstallUpdate(update);
          this.setStatus("\u041E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u0435 \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D\u043E, \u043F\u0435\u0440\u0435\u0437\u0430\u043F\u0443\u0441\u043A\u2026");
        } else {
          this.setStatus("\u0423 \u0432\u0430\u0441 \u043F\u043E\u0441\u043B\u0435\u0434\u043D\u044F\u044F \u0432\u0435\u0440\u0441\u0438\u044F.");
        }
      } catch (e) {
        this.setStatus("\u041E\u0448\u0438\u0431\u043A\u0430: " + e.message);
      } finally {
        this.busy = false;
      }
    }
    async onRollback() {
      const box = this.root.getElementById("history");
      this.historyOpen = !this.historyOpen;
      box.hidden = !this.historyOpen;
      if (!this.historyOpen || this.historyLoaded) return;
      this.historyLoaded = true;
      this.busy = true;
      this.setStatus("\u0417\u0430\u0433\u0440\u0443\u0437\u043A\u0430 \u0438\u0441\u0442\u043E\u0440\u0438\u0438 \u0440\u0435\u043B\u0438\u0437\u043E\u0432\u2026");
      try {
        const history = await getReleaseHistory();
        box.innerHTML = "";
        const title = document.createElement("div");
        title.className = "muted";
        title.textContent = "\u0414\u043E\u0441\u0442\u0443\u043F\u043D\u044B\u0435 \u0440\u0435\u043B\u0438\u0437\u044B:";
        box.appendChild(title);
        const row = document.createElement("div");
        row.className = "row";
        const select = document.createElement("select");
        const placeholder = document.createElement("option");
        placeholder.value = "";
        placeholder.textContent = "\u2014 \u0432\u044B\u0431\u0435\u0440\u0438\u0442\u0435 \u0432\u0435\u0440\u0441\u0438\u044E \u2014";
        placeholder.disabled = true;
        placeholder.selected = true;
        select.appendChild(placeholder);
        for (const r of history) {
          if (r.isCurrent) continue;
          const opt = document.createElement("option");
          opt.value = r.version;
          opt.textContent = r.pubDate ? `v${r.version} \u2014 ${new Date(r.pubDate).toLocaleDateString()}` : `v${r.version}`;
          opt.dataset.url = r.downloadUrl;
          select.appendChild(opt);
        }
        row.appendChild(select);
        const rollbackBtn = document.createElement("button");
        rollbackBtn.className = "primary";
        rollbackBtn.textContent = "\u041E\u0442\u043A\u0430\u0442\u0438\u0442\u044C";
        rollbackBtn.disabled = true;
        rollbackBtn.addEventListener("click", async () => {
          const selected = select.selectedOptions[0];
          const url = selected?.dataset.url;
          if (!url || this.busy) return;
          this.busy = true;
          this.setStatus(`\u041E\u0442\u043A\u0430\u0442 \u043D\u0430 ${selected.value}\u2026`);
          try {
            await installRelease(url);
          } catch (e) {
            this.setStatus("\u041E\u0448\u0438\u0431\u043A\u0430 \u043E\u0442\u043A\u0430\u0442\u0430: " + e.message);
          } finally {
            this.busy = false;
          }
        });
        select.addEventListener("change", () => {
          rollbackBtn.disabled = select.selectedOptions[0]?.dataset.url ? false : true;
        });
        row.appendChild(rollbackBtn);
        box.appendChild(row);
        if (select.options.length <= 1) {
          const none = document.createElement("div");
          none.className = "muted";
          none.style.marginTop = "8px";
          none.textContent = "\u0420\u0435\u0435\u0441\u0442\u0440 \u0431\u043E\u043B\u0435\u0435 \u0441\u0442\u0430\u0440\u044B\u0445 \u0432\u0435\u0440\u0441\u0438\u0439 \u043F\u0443\u0441\u0442.";
          box.appendChild(none);
        }
        this.setStatus("");
      } catch (e) {
        this.setStatus("\u041E\u0448\u0438\u0431\u043A\u0430: " + e.message);
      } finally {
        this.busy = false;
      }
    }
  };
  if (!customElements.get("about-updates-panel")) {
    customElements.define("about-updates-panel", AboutUpdatesPanel);
  }

  // guest-js/index.ts
  async function getReleaseHistory() {
    return invoke("plugin:about-updates|get_release_history");
  }
  async function installRelease(downloadUrl) {
    return invoke("plugin:about-updates|install_release", { downloadUrl });
  }
  async function getAppVersion() {
    return invoke("plugin:about-updates|get_app_version");
  }
  async function getSupportUrl() {
    return invoke("plugin:about-updates|get_support_url");
  }
  async function checkForUpdate() {
    return check();
  }
  async function downloadAndInstallUpdate(update) {
    await update.downloadAndInstall();
  }

  // guest-js/iife-entry.ts
  var g4 = window;
  if ("__TAURI__" in window) {
    const tauri = g4.__TAURI__;
    if (tauri && !tauri["about-updates"]) {
      Object.defineProperty(tauri, "about-updates", {
        configurable: true,
        value: {
          checkForUpdate,
          downloadAndInstallUpdate,
          getAppVersion,
          getReleaseHistory,
          getSupportUrl,
          installRelease
        }
      });
    }
  }
})();
