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

  // guest-js/panels/styles.ts
  var PANEL_STYLES = `
  :host { display: block; color: var(--text, #333); font-family: var(--font, system-ui, sans-serif); }
  /* \u0410\u0442\u0440\u0438\u0431\u0443\u0442 hidden \u043E\u0431\u044F\u0437\u0430\u043D \u0440\u0430\u0431\u043E\u0442\u0430\u0442\u044C \u041B\u042E\u0411\u041E\u0413\u041E \u043A\u043B\u0430\u0441\u0441\u0430 \u0441 display.
     \u041F\u0440\u0430\u0432\u0438\u043B\u043E UA-\u0441\u0442\u0438\u043B\u0435\u0439 [hidden] { display: none } \u043F\u0440\u043E\u0438\u0433\u0440\u044B\u0432\u0430\u0435\u0442 \u0430\u0432\u0442\u043E\u0440\u0441\u043A\u043E\u043C\u0443
     display \u0438\u0437 \u044D\u0442\u043E\u0433\u043E \u0444\u0430\u0439\u043B\u0430 \u043F\u043E \u043F\u0440\u043E\u0438\u0441\u0445\u043E\u0436\u0434\u0435\u043D\u0438\u044E \u043A\u0430\u0441\u043A\u0430\u0434\u0430, \u043F\u043E\u044D\u0442\u043E\u043C\u0443 .badge.ok
     { display: inline-flex } \u0434\u0435\u043B\u0430\u043B \u0431\u0435\u0439\u0434\u0436 \xAB\u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u0435 \u0434\u043E\u0441\u0442\u0443\u043F\u043D\u043E\xBB \u0432\u0435\u0447\u043D\u043E
     \u0432\u0438\u0434\u0438\u043C\u044B\u043C, \u0430 .dlg-overlay { display: flex } \u2014 \u043C\u043E\u0434\u0430\u043B\u043A\u0443. \u041E\u0434\u043D\u043E \u043F\u0440\u0430\u0432\u0438\u043B\u043E
     \u0437\u0430\u043A\u0440\u044B\u0432\u0430\u0435\u0442 \u0432\u0435\u0441\u044C \u043A\u043B\u0430\u0441\u0441 \u043E\u0448\u0438\u0431\u043E\u043A, \u0430 \u043D\u0435 \u043A\u0430\u0436\u0434\u044B\u0439 \u0441\u043B\u0443\u0447\u0430\u0439 \u043F\u043E \u043E\u0442\u0434\u0435\u043B\u044C\u043D\u043E\u0441\u0442\u0438. */
  [hidden] { display: none !important; }
  * { box-sizing: border-box; }
  button { margin: 4px 4px 0 0; padding: 5px 10px; cursor: pointer; border-radius: 6px;
           border: 1px solid var(--border, #ccc); background: var(--session-hover, #eee);
           color: var(--text, #333); font: inherit; }
  button.primary { background: var(--primary, #89b4fa); color: #1e1e2e; font-weight: 600; border-color: var(--primary, #89b4fa); }
  button.primary:hover:not(:disabled) { background: var(--primary-hover, #74a0f0); }
  button.danger { color: #f38ba8; border-color: #f38ba8; background: transparent; }
  button.danger:hover:not(:disabled) { background: var(--session-hover, #45475a); }
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

  /* \u041C\u043E\u0434\u0430\u043B\u043A\u0430 \u043F\u043E\u0434\u0442\u0432\u0435\u0440\u0436\u0434\u0435\u043D\u0438\u044F \u0443\u0434\u0430\u043B\u0435\u043D\u0438\u044F \u0434\u0432\u0438\u0436\u043A\u0430 (\u043F\u043E \u043E\u0431\u0440\u0430\u0437\u0446\u0443 overlay \u0432 llama-\u043F\u043B\u0430\u0448\u043A\u0435). */
  .dlg-overlay { position: fixed; inset: 0; background: rgba(0, 0, 0, .55); display: flex;
                 align-items: center; justify-content: center; z-index: 2000; padding: 16px; }
  .dlg-box { background: var(--bg-elevated, #1c1c1c); border: 1px solid var(--border, #45475a);
             border-radius: 12px; padding: 20px; max-width: 520px; width: 100%; color: var(--text, #cdd6f4); }
  .dlg-box h3 { margin: 0 0 12px; font-size: 17px; }
  .dlg-box p { margin: 8px 0; color: var(--text-muted, #aaa); }
  .dlg-box .strong { color: var(--text, #cdd6f4); word-break: break-all; }
  .dlg-buttons { display: flex; gap: 10px; justify-content: flex-end; margin-top: 16px; }
`;

  // guest-js/panels/speech-engine-panel.ts
  var SpeechEnginePanel = class extends HTMLElement {
    constructor() {
      super(...arguments);
      this.initialized = false;
      this.busy = false;
      this.settings = {};
      this.defaults = { engine_dir: "", models_dir: "" };
      this.backends = [];
      /** Установленные бэкенды с диска: id → версия из `version.txt` (может быть null). */
      this.localBackends = /* @__PURE__ */ new Map();
      this.state = {
        engineDir: "",
        modelsDir: "",
        backend: "",
        installed: false,
        installedVersion: null,
        updateAvailable: false,
        latestVersion: null
      };
      this.download = null;
      this.unlisteners = [];
    }
    connectedCallback() {
      this.root = this.attachShadow({ mode: "open" });
      this.render();
      void this.init();
    }
    render() {
      this.root.innerHTML = `
      <style>${PANEL_STYLES}</style>
      <div>
        <h3>\u0414\u0432\u0438\u0436\u043E\u043A</h3>
        <label for="backend">\u0422\u0438\u043F \u0431\u044D\u043A\u0435\u043D\u0434\u0430:</label>
        <div class="row">
          <select id="backend"></select>
          <span id="backends_none" class="hint warn" hidden>\u043D\u0435 \u0443\u0434\u0430\u043B\u043E\u0441\u044C \u043F\u043E\u043B\u0443\u0447\u0438\u0442\u044C \u0441\u043F\u0438\u0441\u043E\u043A \u0431\u0438\u043D\u0430\u0440\u0435\u0439 (\u043D\u0435\u0442 \u0441\u0435\u0442\u0438?)</span>
        </div>
        <p class="hint">
          \u0421\u0442\u0430\u0442\u0443\u0441: <span id="engine_status">\u2014</span><span id="update_badge" class="badge ok" hidden>\u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u0435 \u0434\u043E\u0441\u0442\u0443\u043F\u043D\u043E</span>
        </p>
        <div class="row">
          <button id="install" class="primary">\u0423\u0441\u0442\u0430\u043D\u043E\u0432\u0438\u0442\u044C</button>
          <button id="checkUpdate">\u041F\u0440\u043E\u0432\u0435\u0440\u0438\u0442\u044C \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u0435</button>
          <button id="installUpdate" class="primary" hidden>\u041E\u0431\u043D\u043E\u0432\u0438\u0442\u044C</button>
          <button id="remove" class="danger" hidden>\u0423\u0434\u0430\u043B\u0438\u0442\u044C</button>
          <button id="engine_dir_browse">\u0418\u0437\u043C\u0435\u043D\u0438\u0442\u044C \u043F\u0443\u0442\u044C</button>
          <button id="unload">\u0412\u044B\u0433\u0440\u0443\u0437\u0438\u0442\u044C (VRAM)</button>
        </div>
        <p class="hint">\u041F\u0443\u0442\u044C \u043A \u0434\u0432\u0438\u0436\u043A\u0443: <code id="engine_dir_code"></code></p>
        <div id="progress" class="progress" style="display:none"><div></div></div>
        <div id="status" class="muted" style="margin-top:8px"></div>

        <div id="delete_dialog" class="dlg-overlay" hidden>
          <div class="dlg-box">
            <h3>\u0423\u0434\u0430\u043B\u0438\u0442\u044C \u0434\u0432\u0438\u0436\u043E\u043A CrispASR?</h3>
            <p>\u0411\u0443\u0434\u0435\u0442 \u0443\u0434\u0430\u043B\u0435\u043D\u0430 \u043F\u0430\u043F\u043A\u0430: <span class="strong" id="delete_path"></span></p>
            <p>\u041C\u043E\u0434\u0435\u043B\u0438 (GGUF) \u043B\u0435\u0436\u0430\u0442 \u0432 \u043E\u0442\u0434\u0435\u043B\u044C\u043D\u043E\u0439 \u043F\u0430\u043F\u043A\u0435 \u0438 \u043E\u0441\u0442\u0430\u043D\u0443\u0442\u0441\u044F \u043D\u0430 \u043C\u0435\u0441\u0442\u0435.</p>
            <div class="dlg-buttons">
              <button id="delete_cancel">\u041E\u0442\u043C\u0435\u043D\u0430</button>
              <button id="delete_ok" class="danger">\u0423\u0434\u0430\u043B\u0438\u0442\u044C</button>
            </div>
          </div>
        </div>

        <hr />

        <h3>\u041F\u0430\u043F\u043A\u0430 \u043C\u043E\u0434\u0435\u043B\u0435\u0439 TTS</h3>
        <div class="row">
          <input id="models_dir_input" type="text" readonly value="" style="flex:1; min-width:200px;" />
          <button id="models_dir_browse">\u0412\u044B\u0431\u0440\u0430\u0442\u044C \u043F\u0430\u043F\u043A\u0443</button>
        </div>
        <p class="hint">\u0412\u043D\u0443\u0442\u0440\u0438 \u0441\u043E\u0437\u0434\u0430\u0451\u0442\u0441\u044F \u043F\u043E\u0434\u043F\u0430\u043F\u043A\u0430 \u043D\u0430 \u043A\u0430\u0436\u0434\u044B\u0439 \u043F\u0440\u0435\u0441\u0435\u0442; \u0432\u0441\u0435 GGUF \u043A\u0430\u0447\u0430\u044E\u0442\u0441\u044F \u0442\u0443\u0434\u0430 \u0430\u0432\u0442\u043E\u043C\u0430\u0442\u0438\u0447\u0435\u0441\u043A\u0438.</p>
      </div>`;
      const on = (id, fn) => this.root.getElementById(id).addEventListener("click", fn);
      on("engine_dir_browse", () => void this.pickDir("engine_dir"));
      on("models_dir_browse", () => void this.pickDir("models_dir"));
      on("install", () => void this.install());
      on("checkUpdate", () => void this.checkUpdate());
      on("installUpdate", () => void this.install());
      on("remove", () => this.openDeleteDialog());
      on("delete_cancel", () => this.closeDeleteDialog());
      on("delete_ok", () => void this.deleteEngine());
      on("unload", () => void this.unload());
      this.root.getElementById("backend").addEventListener("change", (e) => {
        this.state.backend = e.target.value;
        void this.saveSettings();
        this.applySelectedBackendFromLocal();
        this.renderEngineState();
      });
    }
    async init() {
      if (this.initialized) return;
      this.initialized = true;
      const un = await listen(
        "downloader:progress",
        (ev) => {
          const p = ev.payload;
          if (p.kind !== "engine") return;
          this.download = { current: p.downloaded, total: p.total, speed: p.speed_bps };
          this.renderProgress();
        }
      ).catch(() => null);
      if (un) this.unlisteners.push(un);
      const [s, d] = await Promise.all([
        ttsGetSettings().catch(() => ({})),
        ttsDefaultDirs().catch(() => ({ engine_dir: "", models_dir: "" }))
      ]);
      this.settings = s;
      this.defaults = d;
      this.state.engineDir = s.engine_dir || d.engine_dir;
      this.state.modelsDir = s.models_dir || d.models_dir;
      this.state.backend = s.engine_backend || "";
      const modelsInput = this.root.getElementById("models_dir_input");
      modelsInput.value = this.state.modelsDir;
      await this.refreshLocalStatus();
      this.renderEngineState();
      void this.loadBackends();
    }
    /** Перечитывает локальный статус движка с диска (без сети). */
    async refreshLocalStatus() {
      let st;
      try {
        st = await ttsGetEngineStatus();
      } catch (e) {
        this.setStatus("\u043E\u0448\u0438\u0431\u043A\u0430 \u0447\u0442\u0435\u043D\u0438\u044F \u0441\u0442\u0430\u0442\u0443\u0441\u0430 \u0434\u0432\u0438\u0436\u043A\u0430: " + e.message);
        return;
      }
      this.localBackends = new Map(
        (st.installed_backends || []).map((b) => [b.id, b.installed_version])
      );
      this.state.engineDir = st.engine_dir || this.state.engineDir;
      this.state.modelsDir = st.models_dir || this.state.modelsDir;
      if (!this.state.backend) this.state.backend = st.selected_backend || "";
      this.state.installed = st.installed;
      this.state.installedVersion = st.installed ? st.installed_version : null;
    }
    /** Пересчитывает `installed`/`installedVersion` под текущий выбор бэкенда. */
    /**
     * Сбрасывает сетевое состояние апдейта.
     *
     * Результат проверки обновлений относится к КОНКРЕТНОМУ бэкенду в КОНКРЕТНОЙ
     * папке движка. Сменился бэкенд или папка — про новую цель мы ничего не
     * знаем, поэтому гасить надо (иначе бейдж «обновление доступно» остаётся
     * висеть от прежнего бэкенда, а кнопки «Обновить» нет — ровно тот баг,
     * что и чинили: апдейт одного бэкенда выдавался за все).
     */
    clearRemoteUpdateState() {
      this.state.updateAvailable = false;
      this.state.latestVersion = null;
    }
    /** Пересчитывает `installed`/`installedVersion` под текущий выбор бэкенда. */
    applySelectedBackendFromLocal() {
      this.clearRemoteUpdateState();
      if (!this.state.backend) {
        this.state.installed = false;
        this.state.installedVersion = null;
        return;
      }
      this.state.installed = this.localBackends.has(this.state.backend);
      this.state.installedVersion = this.localBackends.get(this.state.backend) ?? null;
    }
    /** Список бэкендов из GitHub. Недоступность сети — не повод ломать локальный UI. */
    async loadBackends() {
      try {
        this.backends = await ttsEngineBackends();
      } catch (e) {
        const span = this.root.getElementById("backends_none");
        if (span) span.hidden = false;
        this.setStatus("\u0441\u043F\u0438\u0441\u043E\u043A \u0431\u044D\u043A\u0435\u043D\u0434\u043E\u0432 \u043D\u0435\u0434\u043E\u0441\u0442\u0443\u043F\u0435\u043D: " + e.message);
        this.renderBackendSelect();
        return;
      }
      this.renderBackendSelect();
    }
    /** Селект: подписи с пометкой «— установлен» и «(рекомендуется)». */
    renderBackendSelect() {
      const sel = this.root.getElementById("backend");
      const prev = this.state.backend;
      sel.innerHTML = "";
      this.backends.forEach((b, i) => {
        const o = document.createElement("option");
        o.value = b.id;
        const marks = [
          this.localBackends.has(b.id) ? "\u2014 \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D" : "",
          i === 0 ? "(\u0440\u0435\u043A\u043E\u043C\u0435\u043D\u0434\u0443\u0435\u0442\u0441\u044F)" : ""
        ].filter(Boolean);
        o.textContent = marks.length ? `${b.label} ${marks.join(" ")}` : b.label;
        sel.appendChild(o);
      });
      for (const id of this.localBackends.keys()) {
        if (this.backends.some((b) => b.id === id)) continue;
        const o = document.createElement("option");
        o.value = id;
        o.textContent = `${id} \u2014 \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D (\u043D\u0435\u0442 \u0432 \u0441\u0432\u0435\u0436\u0435\u043C \u0440\u0435\u043B\u0438\u0437\u0435)`;
        sel.appendChild(o);
      }
      if (sel.options.length === 0) {
        const none = document.createElement("option");
        none.value = "";
        none.textContent = "\u2014 \u0441\u043F\u0438\u0441\u043E\u043A \u043D\u0435\u0434\u043E\u0441\u0442\u0443\u043F\u0435\u043D (\u043D\u0435\u0442 \u0441\u0435\u0442\u0438) \u2014";
        sel.appendChild(none);
      }
      if (prev && Array.from(sel.options).some((o) => o.value === prev)) sel.value = prev;
      this.state.backend = sel.value;
      this.applySelectedBackendFromLocal();
      this.renderEngineState();
    }
    /**
     * Единственное место, где статус, бейдж и кнопки выводятся из состояния.
     * Установка/обновление/удаление меняют состояние → перерисовка идёт отсюда.
     */
    renderEngineState() {
      const s = this.state;
      const btn = (id) => this.root.getElementById(id);
      const status = this.root.getElementById("engine_status");
      if (status) status.textContent = this.statusText();
      const badge = this.root.getElementById("update_badge");
      if (badge) badge.hidden = !s.updateAvailable;
      btn("install").hidden = s.installed;
      btn("checkUpdate").hidden = !s.installed;
      btn("installUpdate").hidden = !(s.installed && s.updateAvailable);
      btn("remove").hidden = !s.installed;
      const dir = this.root.getElementById("engine_dir_code");
      if (dir) dir.textContent = s.engineDir || "\u2014";
    }
    /** Текст статуса. Никогда не врёт: неизвестное отдаётся как неизвестное. */
    statusText() {
      const s = this.state;
      if (!s.backend) return "\u0431\u044D\u043A\u0435\u043D\u0434 \u043D\u0435 \u0432\u044B\u0431\u0440\u0430\u043D";
      if (!s.installed) return `\xAB${s.backend}\xBB \u043D\u0435 \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D`;
      const parts = [`\xAB${s.backend}\xBB \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D`];
      if (s.installedVersion) {
        parts.push(`\u0432\u0435\u0440\u0441\u0438\u044F ${s.installedVersion}`);
      } else {
        parts.push("\u0432\u0435\u0440\u0441\u0438\u044F \u043D\u0435\u0438\u0437\u0432\u0435\u0441\u0442\u043D\u0430");
      }
      if (s.updateAvailable && s.latestVersion) parts.push(`\u0435\u0441\u0442\u044C \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u0435 ${s.latestVersion}`);
      return parts.join(" \xB7 ");
    }
    /** Проверка обновлений по сети. Сетевая ошибка НЕ трогает локальный статус. */
    async checkUpdate() {
      const btn = this.root.getElementById("checkUpdate");
      const label = btn.textContent;
      btn.disabled = true;
      btn.textContent = "\u041F\u0440\u043E\u0432\u0435\u0440\u043A\u0430\u2026";
      try {
        const res = await ttsCheckUpdate();
        if (!res || !res.ok) {
          const err = res?.error || "\u043D\u0435\u0442 \u043E\u0442\u0432\u0435\u0442\u0430 \u043E\u0442 \u0441\u0435\u0440\u0432\u0435\u0440\u0430";
          this.setStatus("\u043F\u0440\u043E\u0432\u0435\u0440\u043A\u0430 \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u0439 \u043D\u0435\u0434\u043E\u0441\u0442\u0443\u043F\u043D\u0430: " + err);
          return;
        }
        this.state.latestVersion = res.latest ?? null;
        const own = (res.engines ?? []).find((e) => e.id === this.state.backend);
        if (this.state.installed) {
          const known = own?.installed_version ?? this.state.installedVersion;
          this.state.installedVersion = known;
          this.state.updateAvailable = own?.update_available === true;
        } else {
          this.state.updateAvailable = false;
        }
        this.renderEngineState();
        this.setStatus(
          !this.state.installed ? `\xAB${this.state.backend}\xBB \u043D\u0435 \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D \u2014 \u043C\u043E\u0436\u043D\u043E \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u0438\u0442\u044C` : this.state.updateAvailable ? "\u043D\u0430\u0439\u0434\u0435\u043D\u043E \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u0435 \u0434\u0432\u0438\u0436\u043A\u0430" : this.state.installedVersion ? "\u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D\u043D\u0430\u044F \u0432\u0435\u0440\u0441\u0438\u044F \u0430\u043A\u0442\u0443\u0430\u043B\u044C\u043D\u0430" : "\u0432\u0435\u0440\u0441\u0438\u044F \u0434\u0432\u0438\u0436\u043A\u0430 \u043D\u0435\u0438\u0437\u0432\u0435\u0441\u0442\u043D\u0430 \u2014 \u043E\u0431\u043D\u043E\u0432\u0438\u0442\u0435 \u0434\u0432\u0438\u0436\u043E\u043A \u0432\u0440\u0443\u0447\u043D\u0443\u044E"
        );
      } catch (e) {
        this.setStatus("\u043F\u0440\u043E\u0432\u0435\u0440\u043A\u0430 \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u0439 \u043D\u0435\u0434\u043E\u0441\u0442\u0443\u043F\u043D\u0430: " + e.message);
      } finally {
        btn.disabled = false;
        btn.textContent = label || "\u041F\u0440\u043E\u0432\u0435\u0440\u0438\u0442\u044C \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u0435";
      }
    }
    /** Установка ИЛИ обновление выбранного бэкенда — одна команда, как в llama-плашке. */
    async install() {
      if (this.busy) return;
      if (!this.state.backend) {
        this.setStatus("\u0432\u044B\u0431\u0435\u0440\u0438\u0442\u0435 \u0431\u044D\u043A\u0435\u043D\u0434");
        return;
      }
      this.busy = true;
      const wasInstalled = this.state.installed;
      this.setStatus(wasInstalled ? "\u043E\u0431\u043D\u043E\u0432\u043B\u044F\u044E \u0434\u0432\u0438\u0436\u043E\u043A\u2026" : "\u0443\u0441\u0442\u0430\u043D\u0430\u0432\u043B\u0438\u0432\u0430\u044E \u0434\u0432\u0438\u0436\u043E\u043A\u2026");
      this.download = { current: 0, total: 0, speed: 0 };
      this.renderProgress();
      this.setButtonsDisabled(true);
      try {
        await ttsDownloadEngine(this.state.backend, this.state.engineDir || this.defaults.engine_dir);
        this.state.updateAvailable = false;
        await this.refreshLocalStatus();
        this.renderBackendSelect();
        this.setStatus(wasInstalled ? "\u0434\u0432\u0438\u0436\u043E\u043A \u043E\u0431\u043D\u043E\u0432\u043B\u0451\u043D" : "\u0434\u0432\u0438\u0436\u043E\u043A \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D");
      } catch (e) {
        this.setStatus("\u043E\u0448\u0438\u0431\u043A\u0430: " + e.message);
      } finally {
        this.download = null;
        this.renderProgress();
        this.setButtonsDisabled(false);
        this.busy = false;
      }
    }
    openDeleteDialog() {
      const path = this.root.getElementById("delete_path");
      if (path) path.textContent = `${this.state.engineDir}\\${this.state.backend}`;
      const dlg = this.root.getElementById("delete_dialog");
      dlg.hidden = false;
    }
    closeDeleteDialog() {
      this.root.getElementById("delete_dialog").hidden = true;
    }
    async deleteEngine() {
      if (this.busy) return;
      this.closeDeleteDialog();
      this.busy = true;
      this.setStatus("\u0443\u0434\u0430\u043B\u044F\u044E \u0434\u0432\u0438\u0436\u043E\u043A\u2026");
      this.setButtonsDisabled(true);
      try {
        const res = await ttsDeleteEngine(
          this.state.backend,
          this.state.engineDir || this.defaults.engine_dir
        );
        const freed = res.freed_bytes ? ` (${(res.freed_bytes / 1048576).toFixed(1)} \u041C\u0411)` : "";
        this.setStatus(`\u0434\u0432\u0438\u0436\u043E\u043A \u0443\u0434\u0430\u043B\u0451\u043D${freed}`);
        this.state.updateAvailable = false;
        this.state.latestVersion = null;
        await this.refreshLocalStatus();
        this.renderBackendSelect();
      } catch (e) {
        this.setStatus("\u043E\u0448\u0438\u0431\u043A\u0430 \u0443\u0434\u0430\u043B\u0435\u043D\u0438\u044F: " + e.message);
      } finally {
        this.setButtonsDisabled(false);
        this.busy = false;
      }
    }
    async unload() {
      try {
        await ttsUnload();
        this.setStatus("\u0434\u0432\u0438\u0436\u043E\u043A \u0432\u044B\u0433\u0440\u0443\u0436\u0435\u043D (VRAM \u043E\u0441\u0432\u043E\u0431\u043E\u0436\u0434\u0451\u043D)");
      } catch (e) {
        this.setStatus("\u043E\u0448\u0438\u0431\u043A\u0430 \u0432\u044B\u0433\u0440\u0443\u0437\u043A\u0438: " + e.message);
      }
    }
    /** Блокирует кнопки на время скачивания/удаления — иначе можно кликнуть дважды. */
    setButtonsDisabled(disabled) {
      for (const id of ["install", "checkUpdate", "installUpdate", "remove", "unload"]) {
        const b = this.root.getElementById(id);
        b.disabled = disabled;
      }
    }
    async pickDir(field) {
      const picked = await open({ directory: true }).catch(() => null);
      if (!picked || typeof picked !== "string") return;
      if (field === "engine_dir") {
        this.settings.engine_dir = picked;
        this.state.engineDir = picked;
      } else {
        this.settings.models_dir = picked;
        this.state.modelsDir = picked;
        this.root.getElementById("models_dir_input").value = picked;
      }
      await this.saveSettings();
      if (field === "engine_dir") {
        this.clearRemoteUpdateState();
        await this.refreshLocalStatus();
        this.renderEngineState();
      }
    }
    async saveSettings() {
      this.settings.engine_dir = this.state.engineDir;
      this.settings.models_dir = this.state.modelsDir;
      this.settings.engine_backend = this.state.backend;
      try {
        await ttsSaveSettings(this.settings);
      } catch (e) {
        this.setStatus("\u043D\u0435 \u0443\u0434\u0430\u043B\u043E\u0441\u044C \u0441\u043E\u0445\u0440\u0430\u043D\u0438\u0442\u044C \u043D\u0430\u0441\u0442\u0440\u043E\u0439\u043A\u0438: " + e.message);
      }
    }
    setStatus(s) {
      const el = this.root.getElementById("status");
      if (el) el.textContent = s;
    }
    renderProgress() {
      const box = this.root.getElementById("progress");
      const bar = box.firstElementChild;
      const d = this.download;
      if (!d) {
        box.style.display = "none";
        return;
      }
      box.style.display = "block";
      bar.style.width = d.total > 0 ? `${Math.min(100, d.current / d.total * 100)}%` : "100%";
      const mb = (b) => (b / 1048576).toFixed(1);
      const speed = d.speed > 1024 ? ` \xB7 ${mb(d.speed)} \u041C\u0411/\u0441` : "";
      const size = d.total > 0 ? `${mb(d.current)} / ${mb(d.total)} \u041C\u0411` : `${mb(d.current)} \u041C\u0411`;
      this.setStatus(`\u0441\u043A\u0430\u0447\u0438\u0432\u0430\u043D\u0438\u0435\u2026 ${size}${speed}`);
    }
    disconnectedCallback() {
      for (const u of this.unlisteners) u();
      this.unlisteners = [];
    }
  };
  if (!customElements.get("speech-engine-panel")) {
    customElements.define("speech-engine-panel", SpeechEnginePanel);
  }

  // guest-js/web-components.ts
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
      void listen(
        "downloader:progress",
        (ev) => {
          const p = ev.payload;
          if (p.kind !== "model") return;
          this.download = { current: p.downloaded, total: p.total };
          this.updateProgress();
        }
      ).then((u) => this.unlisteners.push(u)).catch(() => {
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
  async function ttsPresets() {
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
  async function ttsGetEngineStatus() {
    return invoke("plugin:speech|tts_get_engine_status");
  }
  async function ttsDeleteEngine(backendId, dest) {
    return invoke("plugin:speech|tts_delete_engine", {
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
          ttsDeleteEngine,
          ttsDeleteVoice,
          ttsDownloadEngine,
          ttsDownloadModel,
          ttsEngineBackends,
          ttsGetEngineStatus,
          ttsGetSettings,
          ttsListModels,
          ttsListVoices,
          ttsPresets,
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
