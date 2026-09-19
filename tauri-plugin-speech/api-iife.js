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
  async function open(opts) {
    const dlg = g3.__TAURI__?.dialog;
    if (!dlg?.open) return null;
    return dlg.open(opts);
  }

  // guest-js/web-components.ts
  var PANEL_STYLES = `
  :host { display: block; color: var(--text, #333); font-family: var(--font, system-ui, sans-serif); }
  * { box-sizing: border-box; }
  button { margin: 4px 4px 0 0; padding: 5px 10px; cursor: pointer; border-radius: 6px;
           border: 1px solid var(--border, #ccc); background: var(--session-hover, #eee);
           color: var(--text, #333); font: inherit; }
  button.primary { background: var(--primary, #89b4fa); color: #1e1e2e; font-weight: 600; border-color: var(--primary, #89b4fa); }
  button.primary:hover:not(:disabled) { background: var(--primary-hover, #74a0f0); }
  button:disabled { opacity: .5; cursor: default; }
  select, input, textarea { font: inherit; color: var(--text, #333); background: var(--bg-color, #fff);
           border: 1px solid var(--border, #ccc); border-radius: 6px; padding: 4px 6px; }
  input[readonly] { opacity: .85; }
  input[type="range"] { accent-color: var(--primary, #89b4fa); width: 100%; padding: 0; }
  .muted { color: var(--text-muted, #888); font-size: 12px; }
  .row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin-top: 6px; }
  .field { margin-top: 8px; }
  .field label { display: block; font-size: 13px; margin-bottom: 2px; }
  .field input[type="text"], .field select { width: 100%; }
  .progress { height: 6px; background: var(--session-hover, #eee); border-radius: 3px; margin-top: 6px; overflow: hidden; }
  .progress > div { height: 100%; background: var(--primary, #4a90d9); width: 0; transition: width .2s; }
  h3 { font-size: 15px; margin: 14px 0 8px; opacity: .85; }
  h4 { font-size: 14px; margin: 12px 0 6px; opacity: .8; }
  hr { border: none; border-top: 1px solid var(--border, #45475a); margin: 16px 0; }
  label { display: block; margin: 10px 0 6px; opacity: .8; }
  .checkbox-row { display: flex; align-items: center; gap: 8px; margin: 12px 0 4px; opacity: .9; }
  .checkbox-row input { flex: none; width: auto; min-width: auto; }
  .hint { opacity: .65; font-size: 13px; margin: 4px 0 10px; color: var(--text, #cdd6f4); }
  .hint.warn { color: #f9e2af; }
  code { background: var(--bg-color, #313244); padding: 1px 6px; border-radius: 4px; word-break: break-all; color: var(--text, #cdd6f4); }
  .vs-head { display: flex; justify-content: space-between; align-items: center; gap: 12px; }
  .vs-head h3 { margin: 14px 0 8px; }
  .voice-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 10px; margin-top: 6px; }
  .voice-card { display: flex; gap: 12px; align-items: center; background: var(--bg-color, #313244);
       border: 1px solid var(--border, #45475a); border-radius: 10px; padding: 10px 12px; }
  .vc-avatar { flex: 0 0 auto; width: 52px; height: 52px; border-radius: 50%; overflow: hidden;
       background: var(--session-hover, #45475a); display: flex; align-items: center; justify-content: center; }
  .vc-avatar img { width: 100%; height: 100%; object-fit: cover; }
  .vc-avatar-ph { font-size: 22px; font-weight: 700; color: var(--text, #cdd6f4); }
  .vc-body { flex: 1 1 auto; min-width: 0; }
  .vc-name { font-size: 14px; font-weight: 600; color: var(--text, #cdd6f4); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .vc-ref { font-size: 12px; opacity: .7; color: var(--text, #cdd6f4); margin-top: 2px; display: -webkit-box;
       -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
  .vc-date { font-size: 11px; opacity: .5; margin-top: 3px; }
  .vc-actions { flex: 0 0 auto; display: flex; gap: 4px; align-items: center; }
  .vc-actions button { padding: 4px 9px; font-size: 12px; margin: 0; }
  .vc-del { background: transparent; color: #f38ba8; opacity: .8; }
  .vc-del:hover { opacity: 1; background: var(--session-hover, #45475a); }
  .ve-overlay { position: fixed; inset: 0; background: rgba(0, 0, 0, 0.55); display: flex; align-items: center;
       justify-content: center; z-index: 50; padding: 16px; }
  .ve-modal { background: #181825; border: 1px solid var(--border, #45475a); border-radius: 12px; padding: 18px 20px;
       width: 100%; max-width: 440px; max-height: 90vh; overflow-y: auto; box-shadow: 0 20px 50px rgba(0, 0, 0, 0.5);
       color: var(--text, #cdd6f4); }
  .ve-modal h3 { margin: 0 0 12px; font-size: 17px; }
  .ve-modal input[type="text"], .ve-modal textarea { width: 100%; box-sizing: border-box; padding: 8px; border-radius: 6px;
       border: 1px solid var(--border, #45475a); background: var(--bg-color, #313244); color: var(--text, #cdd6f4); }
  .ve-modal textarea { min-height: 64px; resize: vertical; font-family: inherit; }
  .ve-modal input::placeholder, .ve-modal textarea::placeholder { color: var(--text-muted, #7f849c); }
  .status { opacity: .8; font-size: 13px; color: var(--text, #cdd6f4); }
  ul { list-style: none; padding: 0; margin: 8px 0 0; }
  li { display: flex; justify-content: space-between; align-items: center; gap: 8px;
       padding: 6px 10px; background: var(--bg-color, #313244); border-radius: 6px; margin-bottom: 4px; }
  .mname { flex: 1; min-width: 0; font-size: 13px; }
  .badges { display: flex; align-items: center; gap: 6px; flex-shrink: 0; }
  button.small { padding: 2px 10px; font-size: 11px; border-radius: 8px; margin: 0; }
  .badge { font-size: 11px; padding: 2px 7px; border-radius: 10px; background: var(--session-hover, #45475a);
           color: var(--text, #cdd6f4); white-space: nowrap; }
  .badge.ru { background: #313244; color: #f38ba8; border: 1px solid #f38ba8; font-weight: 600; }
  .badge.ok { background: #1e2a1e; color: #a6e3a1; display: inline-flex; align-items: center; gap: 5px; }
  .badge.ok::before { content: ''; width: 8px; height: 8px; border-radius: 50%; background: #a6e3a1; box-shadow: 0 0 6px #a6e3a1; }
  .badge.warn { background: #33260f; color: #f9c77a; }
`;
  var SpeechEnginePanel = class extends HTMLElement {
    constructor() {
      super(...arguments);
      this.busy = false;
      this.initialized = false;
      this.settings = {};
      this.defaults = { engine_dir: "", models_dir: "" };
      this.backend = "";
      this.backends = [];
      this.status = "";
      this.updateInfo = "";
      this.download = null;
      this.unlisteners = [];
    }
    connectedCallback() {
      this.root = this.attachShadow({ mode: "open" });
      this.render();
      this.init();
    }
    render() {
      this.root.innerHTML = `
      <style>${PANEL_STYLES}</style>
      <div>
        <h3>\u0414\u0432\u0438\u0436\u043E\u043A</h3>
        <label for="backend">\u0422\u0438\u043F \u0431\u044D\u043A\u0435\u043D\u0434\u0430:</label>
        <div class="row">
          <select id="backend"></select>
          <span id="backends_none" class="hint" hidden>\u043D\u0435 \u0443\u0434\u0430\u043B\u043E\u0441\u044C \u043F\u043E\u043B\u0443\u0447\u0438\u0442\u044C \u0441\u043F\u0438\u0441\u043E\u043A \u0431\u0438\u043D\u0430\u0440\u0435\u0439 (\u043D\u0435\u0442 \u0441\u0435\u0442\u0438?)</span>
        </div>
        <p class="hint">\u0421\u0442\u0430\u0442\u0443\u0441: <span id="engine_status">\u2014</span></p>
        <div class="row">
          <button id="install" class="primary">\u0441\u043A\u0430\u0447\u0430\u0442\u044C \u0434\u0432\u0438\u0436\u043E\u043A</button>
          <button id="refresh">\u043F\u0440\u043E\u0432\u0435\u0440\u0438\u0442\u044C \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u044F</button>
          <button id="engine_dir_browse">\u0438\u0437\u043C\u0435\u043D\u0438\u0442\u044C \u043F\u0443\u0442\u044C \u043A \u0434\u0432\u0438\u0436\u043A\u0443</button>
          <button id="unload">\u0432\u044B\u0433\u0440\u0443\u0437\u0438\u0442\u044C (VRAM)</button>
        </div>
        <p class="hint">\u041F\u0443\u0442\u044C \u043A \u0434\u0432\u0438\u0436\u043A\u0443: <code id="engine_dir_code"></code></p>
        <p class="hint" id="update_info" hidden></p>
        <div id="progress" class="progress" style="display:none"><div></div></div>
        <div id="status" class="muted" style="margin-top:8px"></div>

        <hr />

        <h3>\u041F\u0430\u043F\u043A\u0430 \u043C\u043E\u0434\u0435\u043B\u0435\u0439 TTS</h3>
        <div class="row">
          <input id="models_dir_input" type="text" readonly value="" style="flex:1; min-width:200px;" />
          <button id="models_dir_browse">\u0432\u044B\u0431\u0440\u0430\u0442\u044C \u043F\u0430\u043F\u043A\u0443</button>
        </div>
        <p class="hint">\u0412\u043D\u0443\u0442\u0440\u0438 \u0441\u043E\u0437\u0434\u0430\u0451\u0442\u0441\u044F \u043F\u043E\u0434\u043F\u0430\u043F\u043A\u0430 \u043D\u0430 \u043A\u0430\u0436\u0434\u044B\u0439 \u043F\u0440\u0435\u0441\u0435\u0442; \u0432\u0441\u0435 GGUF \u043A\u0430\u0447\u0430\u044E\u0442\u0441\u044F \u0442\u0443\u0434\u0430 \u0430\u0432\u0442\u043E\u043C\u0430\u0442\u0438\u0447\u0435\u0441\u043A\u0438.</p>
      </div>`;
      this.root.getElementById("engine_dir_browse").addEventListener("click", () => this.pickDir("engine_dir"));
      this.root.getElementById("models_dir_browse").addEventListener("click", () => this.pickDir("models_dir"));
      this.root.getElementById("backend").addEventListener("change", (e) => {
        this.backend = e.target.value;
        this.saveSettings();
      });
      this.root.getElementById("install").addEventListener("click", () => this.install());
      this.root.getElementById("refresh").addEventListener("click", () => this.checkUpdate());
      this.root.getElementById("unload").addEventListener("click", async () => {
        try {
          await ttsUnload();
          this.setStatus("\u0434\u0432\u0438\u0436\u043E\u043A \u0432\u044B\u0433\u0440\u0443\u0436\u0435\u043D");
        } catch (e) {
          this.setStatus("\u043E\u0448\u0438\u0431\u043A\u0430: " + e.message);
        }
      });
    }
    async init() {
      if (this.initialized) return;
      const [s, d, b] = await Promise.all([
        ttsGetSettings().catch(() => ({})),
        ttsDefaultDirs().catch(() => ({ engine_dir: "", models_dir: "" })),
        ttsEngineBackends().catch(() => [])
      ]);
      this.settings = s;
      this.defaults = d;
      this.backends = b;
      if (!this.settings.engine_dir) this.settings.engine_dir = d.engine_dir;
      if (!this.settings.models_dir) this.settings.models_dir = d.models_dir;
      if (!this.settings.engine_backend && b.length) this.settings.engine_backend = b[0].id;
      this.root.getElementById("engine_dir_code").textContent = this.settings.engine_dir;
      this.root.getElementById("models_dir_input").value = this.settings.models_dir;
      const sel = this.root.getElementById("backend");
      sel.innerHTML = b.map((x) => `<option value="${x.id}">${x.label}</option>`).join("");
      if (b.length === 0) {
        const none = document.createElement("option");
        none.value = "";
        none.textContent = "\u0441\u0435\u0442\u044C \u043D\u0435\u0434\u043E\u0441\u0442\u0443\u043F\u043D\u0430\u2026";
        sel.appendChild(none);
        const span = this.root.getElementById("backends_none");
        if (span) span.hidden = false;
      }
      sel.value = this.settings.engine_backend || b[0]?.id || "";
      this.backend = sel.value;
      this.unlisteners.push(await listen("tts-download", (ev) => {
        const p = ev.payload;
        this.download = { current: p.downloaded, total: p.total };
        this.updateProgress();
      }));
      this.initialized = true;
      this.checkUpdate();
    }
    async pickDir(field) {
      const picked = await open({ directory: true }).catch(() => null);
      if (picked && typeof picked === "string") {
        if (field === "engine_dir") {
          this.settings.engine_dir = picked;
          this.root.getElementById("engine_dir_code").textContent = picked;
        } else {
          this.settings.models_dir = picked;
          this.root.getElementById("models_dir_input").value = picked;
        }
        this.saveSettings();
      }
    }
    async saveSettings() {
      try {
        await ttsSaveSettings(this.settings);
      } catch {
      }
    }
    setStatus(s) {
      this.status = s;
      const el = this.root.getElementById("status");
      if (el) el.textContent = [s, this.updateInfo].filter(Boolean).join(" \u2022 ");
    }
    setEngineStatus(s) {
      const el = this.root.getElementById("engine_status");
      if (el) el.textContent = s;
    }
    updateProgress() {
      const box = this.root.getElementById("progress");
      const bar = box.firstElementChild;
      if (this.download && this.download.total > 0) {
        box.style.display = "block";
        bar.style.width = `${Math.min(100, this.download.current / this.download.total * 100)}%`;
        this.setStatus(`\u0441\u043A\u0430\u0447\u0438\u0432\u0430\u043D\u0438\u0435\u2026 ${Math.round(this.download.total / 1048576)} \u041C\u0411`);
      } else if (this.download) {
        box.style.display = "block";
        bar.style.width = "100%";
      } else {
        box.style.display = "none";
      }
    }
    async checkUpdate() {
      const res = await ttsCheckUpdate().catch(() => null);
      if (!res) return;
      const infoEl = this.root.getElementById("update_info");
      if (res.ok) {
        const need = (res.engines ?? []).filter((e) => e.update_available);
        this.updateInfo = need.length ? `\u043F\u043E\u0441\u043B\u0435\u0434\u043D\u044F\u044F \u0432\u0435\u0440\u0441\u0438\u044F ${res.latest}: \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u044F \u0434\u043B\u044F ${need.map((e) => e.label).join(", ")}` : `\u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D\u0430 \u043F\u043E\u0441\u043B\u0435\u0434\u043D\u044F\u044F \u0432\u0435\u0440\u0441\u0438\u044F (${res.latest})`;
        this.setEngineStatus(need.length ? "\u0435\u0441\u0442\u044C \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u044F" : `\u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D\u0430 \u043F\u043E\u0441\u043B\u0435\u0434\u043D\u044F\u044F \u0432\u0435\u0440\u0441\u0438\u044F (${res.latest})`);
      } else {
        this.updateInfo = "\u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u044F \u043D\u0435\u0434\u043E\u0441\u0442\u0443\u043F\u043D\u044B: " + (res.error ?? "?");
        this.setEngineStatus("\u043D\u0435 \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D");
      }
      if (infoEl) {
        infoEl.textContent = this.updateInfo;
        infoEl.hidden = false;
      }
      this.setStatus(this.status);
    }
    async install() {
      if (this.busy) return;
      if (!this.backend) {
        this.setStatus("\u0432\u044B\u0431\u0435\u0440\u0438\u0442\u0435 \u0431\u044D\u043A\u0435\u043D\u0434");
        return;
      }
      this.busy = true;
      this.setStatus("\u0441\u043A\u0430\u0447\u0438\u0432\u0430\u044E \u0434\u0432\u0438\u0436\u043E\u043A\u2026");
      this.download = { current: 0, total: 0 };
      this.updateProgress();
      try {
        await ttsDownloadEngine(this.backend, this.settings.engine_dir || this.defaults.engine_dir);
        this.setStatus("\u0434\u0432\u0438\u0436\u043E\u043A \u0441\u043A\u0430\u0447\u0430\u043D");
        await this.checkUpdate();
      } catch (e) {
        this.setStatus("\u043E\u0448\u0438\u0431\u043A\u0430: " + e.message);
      } finally {
        this.download = null;
        this.updateProgress();
        this.busy = false;
      }
    }
    disconnectedCallback() {
      for (const u of this.unlisteners) u();
      this.unlisteners = [];
    }
  };
  var SpeechModelsPanel = class extends HTMLElement {
    constructor() {
      super(...arguments);
      this.models = [];
      this.download = null;
      this.busyId = "";
      this.modelsDir = "";
      this.initialized = false;
      this.unlisteners = [];
    }
    connectedCallback() {
      this.root = this.attachShadow({ mode: "open" });
      this.render();
      this.init();
    }
    static get observedAttributes() {
      return ["models-dir"];
    }
    attributeChangedCallback() {
      if (!this.root || !this.initialized) return;
      const dir = this.getAttribute("models-dir");
      if (dir && dir !== this.modelsDir) {
        this.modelsDir = dir;
        void this.reload();
      }
    }
    render() {
      this.root.innerHTML = `
      <style>${PANEL_STYLES}</style>
      <div>
        <div class="row"><strong>\u0423\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D\u043D\u044B\u0435 \u043C\u043E\u0434\u0435\u043B\u0438 (GGUF)</strong></div>
        <div id="progress" class="progress" style="display:none"><div></div></div>
        <div id="models"></div>
        <div id="status" class="muted" style="margin-top:8px"></div>
        <div class="row" style="margin-top:8px">
          <button id="refresh" class="small">\u27F3 \u043E\u0431\u043D\u043E\u0432\u0438\u0442\u044C</button>
        </div>
      </div>`;
      this.root.getElementById("refresh").addEventListener("click", () => this.reload());
    }
    async init() {
      if (this.initialized) return;
      const attr = this.getAttribute("models-dir");
      if (!this.modelsDir) this.modelsDir = attr || "";
      if (!this.modelsDir) {
        const s = await ttsGetSettings().catch(() => ({}));
        this.modelsDir = s.models_dir || "";
        if (!this.modelsDir) {
          const d = await ttsDefaultDirs().catch(() => ({ models_dir: "" }));
          this.modelsDir = d.models_dir;
        }
      }
      this.initialized = true;
      void listen("tts-download", (ev) => {
        const p = ev.payload;
        if (p.kind !== "model") return;
        this.download = { current: p.downloaded, total: p.total };
        this.updateProgress();
      }).then((u) => this.unlisteners.push(u)).catch(() => {
      });
      void this.reload();
    }
    async reload() {
      this.setStatus("\u0437\u0430\u0433\u0440\u0443\u0436\u0430\u044E\u2026");
      const t = setTimeout(() => this.setStatus("\u0442\u0430\u0439\u043C\u0430\u0443\u0442 \u043E\u0436\u0438\u0434\u0430\u043D\u0438\u044F \u043E\u0442\u0432\u0435\u0442\u0430 (\u0441\u0435\u0442\u044C?)"), 1e4);
      try {
        this.models = await ttsListModels(this.modelsDir);
        if (this.models.length === 0) {
          this.setStatus("\u0441\u043F\u0438\u0441\u043E\u043A \u043F\u0443\u0441\u0442 (\u043D\u0435\u0442 \u043F\u0440\u0435\u0441\u0435\u0442\u043E\u0432 \u0438\u043B\u0438 \u043F\u0443\u0441\u0442\u043E\u0439 \u043A\u0430\u0442\u0430\u043B\u043E\u0433 \u043C\u043E\u0434\u0435\u043B\u0435\u0439)");
        } else {
          this.setStatus(`\u043D\u0430\u0439\u0434\u0435\u043D\u043E \u043C\u043E\u0434\u0435\u043B\u0435\u0439: ${this.models.length}`);
        }
      } catch (e) {
        this.setStatus("\u043E\u0448\u0438\u0431\u043A\u0430: " + (e.message || String(e)));
      } finally {
        clearTimeout(t);
      }
      this.renderList();
    }
    renderList() {
      const box = this.root.getElementById("models");
      if (this.models.length === 0) {
        box.textContent = "";
        return;
      }
      box.innerHTML = "<ul>" + this.models.map((m) => `
      <li>
        <span class="mname">${this.escapeHtml(m.label)}${m.voice_type === "clone" || m.voice_type === "clone_named" ? " \u{1F3AD}" : ""}</span>
        <span class="badges">
          <span class="badge">${m.size}</span>
          ${m.supports_russian ? '<span class="badge ru">RU</span>' : ""}
          ${m.installed ? '<span class="badge ok">\u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D\u043E</span>' : `<button data-id="${m.id}" class="small primary install">\u0441\u043A\u0430\u0447\u0430\u0442\u044C</button>`}
        </span>
      </li>`).join("") + "</ul>";
      for (const btn of box.querySelectorAll("button.install")) {
        btn.addEventListener("click", () => this.install(btn.dataset["id"] || ""));
      }
    }
    escapeHtml(s) {
      return String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
    }
    setStatus(s) {
      const el = this.root.getElementById("status");
      if (el) el.textContent = s;
    }
    updateProgress() {
      const box = this.root.getElementById("progress");
      const bar = box.firstElementChild;
      if (this.download && this.download.total > 0) {
        box.style.display = "block";
        bar.style.width = `${Math.min(100, this.download.current / this.download.total * 100)}%`;
      } else {
        box.style.display = "none";
      }
    }
    async install(id) {
      if (this.busyId) return;
      this.busyId = id;
      this.setStatus(`\u0441\u043A\u0430\u0447\u0438\u0432\u0430\u044E ${id}\u2026`);
      this.download = { current: 0, total: 0 };
      this.updateProgress();
      try {
        await ttsDownloadModel(id, this.modelsDir);
        this.models = await ttsListModels(this.modelsDir).catch(() => this.models);
        this.renderList();
        this.setStatus("\u043C\u043E\u0434\u0435\u043B\u044C \u0441\u043A\u0430\u0447\u0430\u043D\u0430");
      } catch (e) {
        this.setStatus("\u043E\u0448\u0438\u0431\u043A\u0430: " + e.message);
      } finally {
        this.download = null;
        this.updateProgress();
        this.busyId = "";
      }
    }
    disconnectedCallback() {
      for (const u of this.unlisteners) u();
      this.unlisteners = [];
    }
  };
  var SpeechVoiceStorage = class extends HTMLElement {
    constructor() {
      super(...arguments);
      this.voices = [];
      this.modelsDir = "";
      this.initialized = false;
      this.unlisteners = [];
    }
    connectedCallback() {
      this.root = this.attachShadow({ mode: "open" });
      this.render();
      this.init();
    }
    static get observedAttributes() {
      return ["models-dir"];
    }
    attributeChangedCallback() {
      if (!this.root || !this.initialized) return;
      const dir = this.getAttribute("models-dir");
      if (dir && dir !== this.modelsDir) {
        this.modelsDir = dir;
        void this.refresh();
      }
    }
    render() {
      this.root.innerHTML = `
      <style>${PANEL_STYLES}</style>
      <div>
        <div class="vs-head">
          <h3>\u0425\u0440\u0430\u043D\u0438\u043B\u0438\u0449\u0435 \u0433\u043E\u043B\u043E\u0441\u043E\u0432</h3>
          <button id="add" class="primary">\u0434\u043E\u0431\u0430\u0432\u0438\u0442\u044C \u0433\u043E\u043B\u043E\u0441</button>
        </div>
        <p class="hint">\u0417\u0434\u0435\u0441\u044C \u0445\u0440\u0430\u043D\u044F\u0442\u0441\u044F \u0432\u0430\u0448\u0438 \u0433\u043E\u043B\u043E\u0441\u0430 \u0434\u043B\u044F \u043A\u043B\u043E\u043D\u0438\u0440\u043E\u0432\u0430\u043D\u0438\u044F. \u0415\u0441\u043B\u0438 \u043D\u0430\u0436\u0430\u0442\u044C \xAB\u0434\u043E\u0431\u0430\u0432\u0438\u0442\u044C \u0433\u043E\u043B\u043E\u0441\xBB, \u043E\u0442\u043A\u0440\u043E\u0435\u0442\u0441\u044F \u0440\u0435\u0434\u0430\u043A\u0442\u043E\u0440 \u2014 \u0432\u044B\u0431\u0435\u0440\u0438\u0442\u0435 \u0440\u0435\u0444\u0435\u0440\u0435\u043D\u0441\u043D\u043E\u0435 \u0430\u0443\u0434\u0438\u043E (WAV/MP3/OGG/FLAC) \u0438 \u0443\u043A\u0430\u0436\u0438\u0442\u0435 \u0438\u043C\u044F.</p>
        <div id="voices"></div>
        <div class="row"><span id="status" class="status" style="margin-left:8px"></span></div>
        <div id="editor_host"></div>
      </div>`;
      this.root.getElementById("add").addEventListener("click", () => this.openEditor(null));
    }
    async init() {
      if (this.initialized) return;
      const attr = this.getAttribute("models-dir");
      if (!this.modelsDir) this.modelsDir = attr || "";
      if (!this.modelsDir) {
        const s = await ttsGetSettings().catch(() => ({}));
        this.modelsDir = s.models_dir || "";
        if (!this.modelsDir) {
          const d = await ttsDefaultDirs().catch(() => ({ models_dir: "" }));
          this.modelsDir = d.models_dir;
        }
      }
      this.initialized = true;
      await this.refresh();
    }
    async refresh() {
      this.voices = await ttsListVoices(this.modelsDir).catch(() => []);
      this.renderList();
    }
    setStatus(s) {
      const el = this.root.getElementById("status");
      if (el) el.textContent = s;
    }
    async renderList() {
      const box = this.root.getElementById("voices");
      if (this.voices.length === 0) {
        box.innerHTML = '<p class="hint warn">\u041F\u043E\u043A\u0430 \u043D\u0435\u0442 \u0441\u043E\u0445\u0440\u0430\u043D\u0451\u043D\u043D\u044B\u0445 \u0433\u043E\u043B\u043E\u0441\u043E\u0432 \u2014 \u043D\u0430\u0436\u043C\u0438\u0442\u0435 \xAB\u0434\u043E\u0431\u0430\u0432\u0438\u0442\u044C \u0433\u043E\u043B\u043E\u0441\xBB.</p>';
        return;
      }
      box.innerHTML = '<div class="voice-grid"></div>';
      const grid = box.firstElementChild;
      for (const v of this.voices) {
        const card = document.createElement("div");
        card.className = "voice-card";
        const av = document.createElement("div");
        av.className = "vc-avatar";
        if (v.has_avatar) {
          const b = await ttsVoiceAvatar(this.modelsDir, v.id).catch(() => null);
          if (b && b.length) {
            const img = document.createElement("img");
            img.alt = v.name;
            img.src = "data:image/jpeg;base64," + btoa(String.fromCharCode(...b));
            av.appendChild(img);
          } else {
            const ph = document.createElement("div");
            ph.className = "vc-avatar-ph";
            ph.textContent = v.name.slice(0, 1).toUpperCase();
            av.appendChild(ph);
          }
        } else {
          const ph = document.createElement("div");
          ph.className = "vc-avatar-ph";
          ph.textContent = v.name.slice(0, 1).toUpperCase();
          av.appendChild(ph);
        }
        card.appendChild(av);
        const body = document.createElement("div");
        body.className = "vc-body";
        const nm = document.createElement("div");
        nm.className = "vc-name";
        nm.textContent = v.name;
        const rf = document.createElement("div");
        rf.className = "vc-ref";
        rf.textContent = v.ref_text || "\u2014 \u043D\u0435\u0442 \u0440\u0435\u0444\u0435\u0440\u0435\u043D\u0441\u043D\u043E\u0433\u043E \u0442\u0435\u043A\u0441\u0442\u0430 \u2014";
        body.appendChild(nm);
        body.appendChild(rf);
        if (v.created_at) {
          const dt = document.createElement("div");
          dt.className = "vc-date";
          dt.textContent = this.fmtDate(v.created_at);
          body.appendChild(dt);
        }
        card.appendChild(body);
        const acts = document.createElement("div");
        acts.className = "vc-actions";
        const bPlay = document.createElement("button");
        bPlay.textContent = "\u25B6";
        bPlay.title = "\u041F\u0440\u043E\u0441\u043B\u0443\u0448\u0430\u0442\u044C";
        bPlay.addEventListener("click", () => this.playVoice(v.id));
        const bEdit = document.createElement("button");
        bEdit.textContent = "\u270E";
        bEdit.title = "\u0418\u0437\u043C\u0435\u043D\u0438\u0442\u044C";
        bEdit.addEventListener("click", () => this.openEditor(v));
        const bDel = document.createElement("button");
        bDel.className = "vc-del";
        bDel.textContent = "\u2715";
        bDel.title = "\u0423\u0434\u0430\u043B\u0438\u0442\u044C";
        bDel.addEventListener("click", () => this.deleteVoice(v.id));
        acts.append(bPlay, bEdit, bDel);
        card.appendChild(acts);
        grid.appendChild(card);
      }
    }
    fmtDate(s) {
      if (!s) return "";
      return s.replace("T", " ").replace("Z", "").slice(0, 16);
    }
    async playVoice(id) {
      try {
        const data = await ttsVoiceAudio(this.modelsDir, id);
        const url = URL.createObjectURL(new Blob([new Uint8Array(data)], { type: "audio/wav" }));
        const audio = new Audio(url);
        audio.onended = () => URL.revokeObjectURL(url);
        await audio.play();
      } catch (e) {
        this.setStatus("\u043D\u0435 \u0443\u0434\u0430\u043B\u043E\u0441\u044C \u043F\u0440\u043E\u0438\u0433\u0440\u0430\u0442\u044C: " + e.message);
      }
    }
    async deleteVoice(id) {
      if (!confirm("\u0423\u0434\u0430\u043B\u0438\u0442\u044C \u0433\u043E\u043B\u043E\u0441 \u0431\u0435\u0437\u0432\u043E\u0437\u0432\u0440\u0430\u0442\u043D\u043E?")) return;
      try {
        await ttsDeleteVoice(this.modelsDir, id);
        await this.refresh();
        this.setStatus("\u0433\u043E\u043B\u043E\u0441 \u0443\u0434\u0430\u043B\u0451\u043D");
      } catch (e) {
        this.setStatus("\u043E\u0448\u0438\u0431\u043A\u0430 \u0443\u0434\u0430\u043B\u0435\u043D\u0438\u044F: " + e.message);
      }
    }
    baseName(p) {
      return p.split("\\").pop()?.split("/").pop() || p;
    }
    openEditor(voice) {
      const host = this.root.getElementById("editor_host");
      const ac = new AbortController();
      const close = () => {
        ac.abort();
        host.innerHTML = "";
      };
      host.innerHTML = `
      <div class="ve-overlay" id="ve_overlay" role="presentation">
        <div class="ve-modal" role="dialog" aria-modal="true">
          <h3>${voice ? "\u0418\u0437\u043C\u0435\u043D\u0438\u0442\u044C \u0433\u043E\u043B\u043E\u0441" : "\u041D\u043E\u0432\u044B\u0439 \u0433\u043E\u043B\u043E\u0441"}</h3>

          <label for="ve_name">\u0418\u043C\u044F \u0433\u043E\u043B\u043E\u0441\u0430:</label>
          <input id="ve_name" type="text" placeholder="\u043D\u0430\u043F\u0440. Morgan Freeman" value="${voice ? voice.name : ""}" />

          <label for="ve_audio">\u0420\u0435\u0444\u0435\u0440\u0435\u043D\u0441\u043D\u043E\u0435 \u0430\u0443\u0434\u0438\u043E${voice ? " (\u043E\u043F\u0446. \u2014 \u0447\u0442\u043E\u0431\u044B \u0437\u0430\u043C\u0435\u043D\u0438\u0442\u044C)" : ""}:</label>
          <div class="row">
            <button id="ve_pick_audio">\u0432\u044B\u0431\u0440\u0430\u0442\u044C \u0430\u0443\u0434\u0438\u043E</button>
            <span class="status" id="ve_audio_status">${voice ? "\u0431\u0435\u0437 \u0438\u0437\u043C\u0435\u043D\u0435\u043D\u0438\u0439" : "\u043D\u0435 \u0432\u044B\u0431\u0440\u0430\u043D\u043E"}</span>
          </div>

          <div id="ve_denoise_block" style="${voice ? "display:none" : ""}">
            <label class="checkbox-row">
              <input id="ve_denoise" type="checkbox" checked />
              \u0448\u0443\u043C\u043E\u043F\u043E\u0434\u0430\u0432\u043B\u0435\u043D\u0438\u0435 (RNNoise)
            </label>
            <label for="ve_denoise_strength">\u0421\u0438\u043B\u0430 \u0448\u0443\u043C\u043E\u043F\u043E\u0434\u0430\u0432\u043B\u0435\u043D\u0438\u044F: <span id="ve_denoise_label">90%</span></label>
            <input id="ve_denoise_strength" type="range" min="0" max="1" step="0.05" value="0.9" />
          </div>

          <label for="ve_text">\u0420\u0435\u0444\u0435\u0440\u0435\u043D\u0441\u043D\u044B\u0439 \u0442\u0435\u043A\u0441\u0442 (\u043E\u043F\u0446., \u0443\u043B\u0443\u0447\u0448\u0430\u0435\u0442 \u043A\u0430\u0447\u0435\u0441\u0442\u0432\u043E):</label>
          <textarea id="ve_text" placeholder="\u0447\u0442\u043E \u0433\u043E\u0432\u043E\u0440\u0438\u0442\u0441\u044F \u0432 \u0430\u0443\u0434\u0438\u043E">${voice?.ref_text ?? ""}</textarea>

          ${voice ? '<div class="row"><button id="ve_play">\u25B6 \u043F\u0440\u043E\u0441\u043B\u0443\u0448\u0430\u0442\u044C \u0440\u0435\u0444\u0435\u0440\u0435\u043D\u0441</button></div>' : ""}

          <label for="ve_avatar">\u0410\u0432\u0430\u0442\u0430\u0440 (\u043E\u043F\u0446.):</label>
          <div class="row">
            <button id="ve_pick_avatar">\u0432\u044B\u0431\u0440\u0430\u0442\u044C \u043A\u0430\u0440\u0442\u0438\u043D\u043A\u0443</button>
            <span class="status" id="ve_avatar_status">${voice?.has_avatar ? "\u0431\u0435\u0437 \u0438\u0437\u043C\u0435\u043D\u0435\u043D\u0438\u0439" : "\u043D\u0435 \u0432\u044B\u0431\u0440\u0430\u043D\u0430"}</span>
            ${voice?.has_avatar ? '<button class="small" id="ve_remove_avatar">\u0441\u0431\u0440\u043E\u0441\u0438\u0442\u044C \u0430\u0432\u0430\u0442\u0430\u0440</button>' : ""}
          </div>

          <div class="row">
            <button id="ve_save" class="primary">\u0441\u043E\u0445\u0440\u0430\u043D\u0438\u0442\u044C</button>
            <button id="ve_cancel">\u043E\u0442\u043C\u0435\u043D\u0430</button>
            <span class="status" id="ve_status"></span>
          </div>
        </div>
      </div>
      <audio id="ve_audio"></audio>`;
      const $ = (id) => host.querySelector("#" + id);
      let audioPath = "";
      let avatarPath = "";
      let removeAvatar = false;
      let busy = false;
      this.root.addEventListener("keydown", (e) => {
        if (e.key === "Escape") close();
      }, { signal: ac.signal });
      $("ve_overlay").addEventListener("click", (e) => {
        if (e.target === e.currentTarget) close();
      }, { signal: ac.signal });
      $("ve_cancel").addEventListener("click", () => close(), { signal: ac.signal });
      const denoiseChk = $("ve_denoise");
      const denoiseStr = $("ve_denoise_strength");
      denoiseChk?.addEventListener("change", (e) => {
        const block = this.root.getElementById("ve_denoise_block");
        if (block) block.style.display = e.target.checked ? "" : "none";
      }, { signal: ac.signal });
      denoiseStr?.addEventListener("input", (e) => {
        const lbl = this.root.getElementById("ve_denoise_label");
        if (lbl) lbl.textContent = Math.round(Number(e.target.value) * 100) + "%";
      }, { signal: ac.signal });
      $("ve_pick_audio").addEventListener("click", async () => {
        const p = await open({ filters: [{ name: "Audio", extensions: ["wav", "ogg", "mp3", "flac"] }] }).catch(() => null);
        if (p && typeof p === "string") {
          audioPath = p;
          $("ve_audio_status").textContent = this.baseName(p);
          const block = this.root.getElementById("ve_denoise_block");
          if (block) block.style.display = "";
        }
      }, { signal: ac.signal });
      $("ve_pick_avatar").addEventListener("click", async () => {
        const p = await open({ filters: [{ name: "Image", extensions: ["jpg", "jpeg", "png", "webp"] }] }).catch(() => null);
        if (p && typeof p === "string") {
          avatarPath = p;
          removeAvatar = false;
          $("ve_avatar_status").textContent = this.baseName(p);
        }
      }, { signal: ac.signal });
      const rmBtn = $("ve_remove_avatar");
      rmBtn?.addEventListener("click", () => {
        removeAvatar = !removeAvatar;
        avatarPath = "";
        $("ve_avatar_status").textContent = removeAvatar ? "\u0431\u0443\u0434\u0435\u0442 \u0443\u0434\u0430\u043B\u0451\u043D" : "\u0431\u0435\u0437 \u0438\u0437\u043C\u0435\u043D\u0435\u043D\u0438\u0439";
      }, { signal: ac.signal });
      const playBtn = $("ve_play");
      playBtn?.addEventListener("click", async () => {
        if (!voice) return;
        try {
          const b = await ttsVoiceTrimmedAudio(this.modelsDir, voice.id, "");
          const data = new Uint8Array(b);
          const url = URL.createObjectURL(new Blob([data], { type: "audio/wav" }));
          const audioEl = $("ve_audio");
          audioEl.src = url;
          audioEl.onended = () => URL.revokeObjectURL(url);
          await audioEl.play();
        } catch (e) {
          $("ve_status").textContent = "\u043E\u0448\u0438\u0431\u043A\u0430: " + e.message;
        }
      }, { signal: ac.signal });
      $("ve_save").addEventListener("click", async () => {
        if (busy) return;
        const name = $("ve_name").value.trim();
        if (!name) {
          $("ve_status").textContent = "\u0432\u0432\u0435\u0434\u0438\u0442\u0435 \u0438\u043C\u044F \u0433\u043E\u043B\u043E\u0441\u0430";
          return;
        }
        if (!voice && !audioPath) {
          $("ve_status").textContent = "\u0432\u044B\u0431\u0435\u0440\u0438\u0442\u0435 \u0440\u0435\u0444\u0435\u0440\u0435\u043D\u0441\u043D\u043E\u0435 \u0430\u0443\u0434\u0438\u043E";
          return;
        }
        busy = true;
        $("ve_status").textContent = "\u0441\u043E\u0445\u0440\u0430\u043D\u044F\u044E\u2026";
        const refText = $("ve_text").value;
        const denoise = denoiseChk ? denoiseChk.checked : true;
        const denoiseStrength = denoiseStr ? Number(denoiseStr.value) : 0.9;
        try {
          if (voice) {
            await ttsUpdateVoice({
              modelsDir: this.modelsDir,
              id: voice.id,
              name,
              refText,
              avatar: removeAvatar ? "__REMOVE__" : avatarPath,
              srcAudio: audioPath,
              denoise,
              denoiseStrength
            });
          } else {
            await ttsAddVoice({
              modelsDir: this.modelsDir,
              name,
              srcAudio: audioPath,
              refText,
              avatar: avatarPath,
              denoise,
              denoiseStrength
            });
          }
          close();
          await this.refresh();
          this.setStatus("\u0433\u043E\u043B\u043E\u0441 \u0441\u043E\u0445\u0440\u0430\u043D\u0451\u043D");
        } catch (e) {
          $("ve_status").textContent = "\u043E\u0448\u0438\u0431\u043A\u0430: " + e.message;
        } finally {
          busy = false;
        }
      }, { signal: ac.signal });
    }
    disconnectedCallback() {
      for (const u of this.unlisteners) u();
      this.unlisteners = [];
    }
  };
  if (!customElements.get("speech-engine-panel")) {
    customElements.define("speech-engine-panel", SpeechEnginePanel);
  }
  if (!customElements.get("speech-models-panel")) {
    customElements.define("speech-models-panel", SpeechModelsPanel);
  }
  if (!customElements.get("speech-voice-storage")) {
    customElements.define("speech-voice-storage", SpeechVoiceStorage);
  }

  // guest-js/index.ts
  async function ttsSpeak(preset, voice, instruct, speed, text, language) {
    return invoke("plugin:speech|tts_speak", {
      preset,
      voice,
      instruct,
      speed,
      text,
      language
    });
  }
  async function ttsPresets2() {
    return invoke("plugin:speech|tts_presets");
  }
  async function ttsCapabilities() {
    return invoke("plugin:speech|tts_capabilities");
  }
  async function ttsUnload() {
    return invoke("plugin:speech|tts_unload");
  }
  async function ttsSaveWav(path, data) {
    return invoke("plugin:speech|tts_save_wav", { path, data });
  }
  async function ttsDownloadEngine(backendId, dest) {
    return invoke("plugin:speech|tts_download_engine", {
      backendId,
      dest
    });
  }
  async function ttsDownloadModel(preset, dest) {
    return invoke(
      "plugin:speech|tts_download_model",
      { preset, dest }
    );
  }
  async function ttsEngineBackends() {
    return invoke("plugin:speech|tts_engine_backends");
  }
  async function ttsListModels(modelsDir) {
    return invoke("plugin:speech|tts_list_models", { modelsDir });
  }
  async function ttsListVoices(modelsDir) {
    return invoke("plugin:speech|tts_list_voices", { modelsDir });
  }
  async function ttsAddVoice(args) {
    return invoke("plugin:speech|tts_add_voice", {
      modelsDir: args.modelsDir,
      name: args.name,
      srcAudio: args.srcAudio,
      refText: args.refText,
      avatar: args.avatar,
      denoise: args.denoise,
      denoiseStrength: args.denoiseStrength
    });
  }
  async function ttsDeleteVoice(modelsDir, id) {
    return invoke("plugin:speech|tts_delete_voice", { modelsDir, id });
  }
  async function ttsUpdateVoice(args) {
    return invoke("plugin:speech|tts_update_voice", {
      modelsDir: args.modelsDir,
      id: args.id,
      name: args.name,
      refText: args.refText,
      avatar: args.avatar,
      srcAudio: args.srcAudio,
      denoise: args.denoise,
      denoiseStrength: args.denoiseStrength
    });
  }
  async function ttsVoiceAvatar(modelsDir, id) {
    return invoke("plugin:speech|tts_voice_avatar", {
      modelsDir,
      id
    });
  }
  async function ttsVoiceAudio(modelsDir, id) {
    return invoke("plugin:speech|tts_voice_audio", { modelsDir, id });
  }
  async function ttsVoiceTrimmedAudio(modelsDir, id, backend) {
    return invoke("plugin:speech|tts_voice_trimmed_audio", {
      modelsDir,
      id,
      backend
    });
  }
  async function ttsCheckUpdate() {
    return invoke("plugin:speech|tts_check_update");
  }
  async function ttsDefaultDirs() {
    return invoke("plugin:speech|tts_default_dirs");
  }
  async function ttsGetSettings() {
    return invoke("plugin:speech|tts_get_settings");
  }
  async function ttsSaveSettings(settings) {
    return invoke("plugin:speech|tts_save_settings", { settings });
  }
  async function sttGetSettings() {
    return invoke("plugin:speech|stt_get_settings");
  }
  async function sttSaveSettings(settings) {
    return invoke("plugin:speech|stt_save_settings", { settings });
  }
  async function sttStart() {
    return invoke("plugin:speech|stt_start");
  }
  async function sttStop() {
    return invoke("plugin:speech|stt_stop");
  }
  async function sttGetStatus() {
    return invoke("plugin:speech|stt_get_status");
  }
  async function sttInjectText(text) {
    return invoke("plugin:speech|stt_inject_text", { text });
  }

  // guest-js/iife-entry.ts
  var g4 = window;
  if ("__TAURI__" in window) {
    const tauri = g4.__TAURI__;
    if (tauri && !tauri.speech) {
      Object.defineProperty(tauri, "speech", {
        configurable: true,
        value: {
          sttGetSettings,
          sttGetStatus,
          sttInjectText,
          sttSaveSettings,
          sttStart,
          sttStop,
          ttsAddVoice,
          ttsCapabilities,
          ttsCheckUpdate,
          ttsDefaultDirs,
          ttsDeleteVoice,
          ttsDownloadEngine,
          ttsDownloadModel,
          ttsEngineBackends,
          ttsGetSettings,
          ttsListModels,
          ttsListVoices,
          ttsPresets: ttsPresets2,
          ttsSaveSettings,
          ttsSaveWav,
          ttsSpeak,
          ttsUnload,
          ttsUpdateVoice,
          ttsVoiceAudio,
          ttsVoiceAvatar,
          ttsVoiceTrimmedAudio
        }
      });
    }
  }
})();
