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

  // guest-js/shims/dialog.ts
  var g2 = window;
  async function open(opts) {
    const dialog = g2.__TAURI__?.dialog;
    if (!dialog?.open) throw new Error("window.__TAURI__.dialog.open \u043D\u0435\u0434\u043E\u0441\u0442\u0443\u043F\u0435\u043D");
    return dialog.open(opts);
  }

  // guest-js/shims/event.ts
  var g3 = window;
  async function listen(name, cb) {
    const event = g3.__TAURI__?.event ?? g3.__TAURI_INTERNALS__?.event;
    if (!event?.listen) throw new Error("window.__TAURI__.event.listen \u043D\u0435\u0434\u043E\u0441\u0442\u0443\u043F\u0435\u043D");
    return event.listen(name, (e) => cb(e));
  }

  // guest-js/web-components.ts
  var STYLE = `
  :host { display: block; color: var(--text, #eee); font-family: var(--font, system-ui, sans-serif); font-size: 13px; }
  .row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .row + .row { margin-top: 8px; }
  label { color: var(--text-muted, #999); min-width: 80px; }
  button { padding: 6px 12px; cursor: pointer; border-radius: 6px; border: 1px solid var(--border, #333);
           background: var(--bg-elevated, #2a2a2a); color: var(--text, #eee); font: inherit; font-size: 13px; }
  button.primary { background: var(--primary, #4a90d9); color: #fff; border-color: var(--primary, #4a90d9); }
  button.danger { background: var(--danger, #b54242); color: #fff; border-color: var(--danger, #b54242); }
  button:disabled { opacity: .5; cursor: default; }
  select, input[type=text] { padding: 6px; border-radius: 6px; border: 1px solid var(--border, #333);
           background: var(--bg-elevated, #1c1c1c); color: var(--text, #eee); font: inherit; font-size: 13px; }
  .progress-container { display: none; margin-top: 10px; }
  .progress-status { color: var(--text-muted, #999); font-size: 12px; margin-bottom: 5px; word-break: break-all; }
  .progress-track { height: 8px; border-radius: 6px; background: var(--border, #333); overflow: hidden; }
  .progress-bar { height: 100%; width: 0%; background: var(--primary, #4a90d9); transition: width .1s linear; }
  .hint { color: var(--text-muted, #999); font-size: 12px; white-space: pre-line; }
  .badge-ok { color: #4caf50; }
  .badge-err { color: #b54242; }
  .badge-warn { color: #ff9800; }
  .section-title { font-size: 15px; font-weight: 600; color: var(--text, #eee); margin: 0 0 10px 0; }
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
  var YtdlpPanel = class extends HTMLElement {
    constructor() {
      super(...arguments);
      this.busy = false;
    }
    connectedCallback() {
      if (this.root) return;
      this.root = this.attachShadow({ mode: "open" });
      this.root.innerHTML = `
      <style>${STYLE}</style>
      <div>
        <h3 class="section-title">\u0423\u043F\u0440\u0430\u0432\u043B\u0435\u043D\u0438\u0435 yt-dlp</h3>
        <div class="row"><label>\u0421\u0442\u0430\u0442\u0443\u0441:</label><span id="status" class="hint">\u041F\u0440\u043E\u0432\u0435\u0440\u043A\u0430\u2026</span></div>
        <div class="row"><label>\u0412\u0435\u0440\u0441\u0438\u044F:</label><span id="version" class="hint">\u2014</span></div>
        <div class="row"><label>\u041F\u0443\u0442\u044C:</label><span id="path" class="hint" style="word-break:break-all; flex:1;"></span></div>
        <div class="row"><label>FFmpeg:</label><span id="ffmpeg" class="hint">\u2014</span></div>
        <div class="row"><label>Deno:</label><span id="deno" class="hint">\u2014</span></div>
        <div class="row" style="margin-top:10px;">
          <button id="install" class="primary">\u0423\u0441\u0442\u0430\u043D\u043E\u0432\u0438\u0442\u044C yt-dlp</button>
          <button id="checkUpdate" style="display:none;">\u041F\u0440\u043E\u0432\u0435\u0440\u0438\u0442\u044C \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u0435</button>
          <button id="installUpdate" class="primary" style="display:none;">\u041E\u0431\u043D\u043E\u0432\u0438\u0442\u044C</button>
          <button id="installDeno">\u0423\u0441\u0442\u0430\u043D\u043E\u0432\u0438\u0442\u044C Deno</button>
          <button id="setDir">\u0418\u0437\u043C\u0435\u043D\u0438\u0442\u044C \u043F\u0443\u0442\u044C</button>
          <button id="setFfmpeg">\u041F\u0443\u0442\u044C \u043A FFmpeg</button>
        </div>
        <div id="progress" class="progress-container">
          <div class="progress-status" id="progressStatus">\u041F\u043E\u0434\u0433\u043E\u0442\u043E\u0432\u043A\u0430\u2026</div>
          <div class="progress-track"><div class="progress-bar" id="progressBar"></div></div>
        </div>
      </div>`;
      this.root.getElementById("install").addEventListener("click", () => this.onInstall());
      this.root.getElementById("checkUpdate").addEventListener("click", () => this.onCheckUpdate());
      this.root.getElementById("installUpdate").addEventListener("click", () => this.onInstallUpdate());
      this.root.getElementById("installDeno").addEventListener("click", () => this.onInstallDeno());
      this.root.getElementById("setDir").addEventListener("click", () => this.onSetDir());
      this.root.getElementById("setFfmpeg").addEventListener("click", () => this.onSetFfmpeg());
      void this.refresh();
    }
    disconnectedCallback() {
    }
    showProgress(on) {
      this.root.getElementById("progress").style.display = on ? "block" : "none";
    }
    setProgress(pct) {
      const bar = this.root.getElementById("progressBar");
      bar.style.width = `${pct}%`;
    }
    async refresh() {
      let st;
      try {
        st = await getYtdlpStatus();
      } catch (e) {
        this.root.getElementById("status").textContent = `\u041E\u0448\u0438\u0431\u043A\u0430: ${e}`;
        return;
      }
      this.root.getElementById("status").textContent = st.message;
      this.root.getElementById("version").textContent = st.version ?? "\u2014";
      this.root.getElementById("path").textContent = st.exe_path;
      const ffmpegEl = this.root.getElementById("ffmpeg");
      if (st.ffmpeg_installed) {
        ffmpegEl.innerHTML = `<span class="badge-ok">\u041D\u0430\u0439\u0434\u0435\u043D</span> ${esc(st.ffmpeg_path ?? "")}`;
      } else {
        ffmpegEl.innerHTML = '<span class="badge-warn">\u041D\u0435 \u043D\u0430\u0439\u0434\u0435\u043D (\u043D\u0443\u0436\u0435\u043D \u0434\u043B\u044F \u0441\u043A\u043B\u0435\u0439\u043A\u0438)</span>';
      }
      const denoEl = this.root.getElementById("deno");
      if (st.deno_installed) {
        denoEl.innerHTML = `<span class="badge-ok">\u0423\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D</span> ${esc(st.deno_path ?? "")}`;
      } else {
        denoEl.innerHTML = '<span class="badge-warn">\u041D\u0435 \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D (\u043D\u0443\u0436\u0435\u043D \u0434\u043B\u044F n-sig)</span>';
      }
      const installed = st.installed;
      this.root.getElementById("install").style.display = installed ? "none" : "inline-block";
      this.root.getElementById("checkUpdate").style.display = installed ? "inline-block" : "none";
      this.root.getElementById("installUpdate").style.display = "none";
    }
    async onInstall() {
      if (this.busy) return;
      this.busy = true;
      const btn = this.root.getElementById("install");
      btn.disabled = true;
      this.showProgress(true);
      this.setProgress(0);
      this.root.getElementById("progressStatus").textContent = "\u0421\u043A\u0430\u0447\u0438\u0432\u0430\u043D\u0438\u0435 yt-dlp...";
      try {
        await installYtdlp();
        toast("yt-dlp \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D!");
        await this.refresh();
      } catch (e) {
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430 \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043A\u0438: ${e}`, "error");
      } finally {
        btn.disabled = false;
        this.busy = false;
        this.showProgress(false);
      }
    }
    async onCheckUpdate() {
      if (this.busy) return;
      this.busy = true;
      const btn = this.root.getElementById("checkUpdate");
      btn.disabled = true;
      try {
        const newVer = await checkYtdlpUpdate();
        if (newVer) {
          this.root.getElementById("status").textContent = `\u0414\u043E\u0441\u0442\u0443\u043F\u043D\u043E \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u0435: ${newVer}`;
          this.root.getElementById("installUpdate").style.display = "inline-block";
        } else {
          this.root.getElementById("status").textContent = "yt-dlp \u0430\u043A\u0442\u0443\u0430\u043B\u0435\u043D";
        }
      } catch (e) {
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430 \u043F\u0440\u043E\u0432\u0435\u0440\u043A\u0438: ${e}`, "error");
      } finally {
        btn.disabled = false;
        this.busy = false;
      }
    }
    async onInstallUpdate() {
      if (this.busy) return;
      this.busy = true;
      const btn = this.root.getElementById("installUpdate");
      btn.disabled = true;
      this.showProgress(true);
      this.setProgress(0);
      this.root.getElementById("progressStatus").textContent = "\u041E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u0435 yt-dlp...";
      try {
        await installYtdlpUpdate();
        toast("yt-dlp \u043E\u0431\u043D\u043E\u0432\u043B\u0451\u043D!");
        await this.refresh();
      } catch (e) {
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430 \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u044F: ${e}`, "error");
      } finally {
        btn.disabled = false;
        this.busy = false;
        this.showProgress(false);
      }
    }
    async onInstallDeno() {
      if (this.busy) return;
      this.busy = true;
      const btn = this.root.getElementById("installDeno");
      btn.disabled = true;
      this.showProgress(true);
      this.setProgress(0);
      this.root.getElementById("progressStatus").textContent = "\u0421\u043A\u0430\u0447\u0438\u0432\u0430\u043D\u0438\u0435 Deno...";
      try {
        await installDeno();
        toast("Deno \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D!");
        await this.refresh();
      } catch (e) {
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430 \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043A\u0438 Deno: ${e}`, "error");
      } finally {
        btn.disabled = false;
        this.busy = false;
        this.showProgress(false);
      }
    }
    async onSetDir() {
      try {
        const sel = await open({ directory: true });
        if (!sel) return;
        const path = Array.isArray(sel) ? sel[0] : sel;
        if (!path) return;
        await setYtdlpDir(path);
        toast("\u041F\u0443\u0442\u044C \u043A yt-dlp \u0438\u0437\u043C\u0435\u043D\u0451\u043D");
        await this.refresh();
      } catch (e) {
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430: ${e}`, "error");
      }
    }
    async onSetFfmpeg() {
      try {
        const sel = await open({ directory: true });
        if (!sel) return;
        const path = Array.isArray(sel) ? sel[0] : sel;
        if (!path) return;
        await setFfmpegPath(path);
        toast("\u041F\u0443\u0442\u044C \u043A FFmpeg \u0438\u0437\u043C\u0435\u043D\u0451\u043D");
        await this.refresh();
      } catch (e) {
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430: ${e}`, "error");
      }
    }
  };
  var YtdlpDownloadPanel = class extends HTMLElement {
    constructor() {
      super(...arguments);
      this.busy = false;
    }
    connectedCallback() {
      if (this.root) return;
      this.root = this.attachShadow({ mode: "open" });
      this.root.innerHTML = `
      <style>${STYLE}</style>
      <div>
        <h3 class="section-title">\u0421\u043A\u0430\u0447\u0438\u0432\u0430\u043D\u0438\u0435 \u0432\u0438\u0434\u0435\u043E</h3>
        <div class="row">
          <label>URL \u0432\u0438\u0434\u0435\u043E:</label>
          <input type="text" id="url" placeholder="https://www.youtube.com/watch?v=..." style="flex:1; min-width:300px;">
        </div>
        <div class="row">
          <label>\u0424\u043E\u0440\u043C\u0430\u0442:</label>
          <select id="format">
            <option value="mp4">MP4 (\u0411\u0435\u0437 \u043A\u043E\u043D\u0432\u0435\u0440\u0442\u0430\u0446\u0438\u0438)</option>
            <option value="best">\u041B\u0443\u0447\u0448\u0438\u0439 (\u041B\u044E\u0431\u043E\u0439 \u0444\u043E\u0440\u043C\u0430\u0442)</option>
            <option value="mp3">\u0422\u043E\u043B\u044C\u043A\u043E \u0410\u0443\u0434\u0438\u043E (MP3)</option>
          </select>
          <label>\u041A\u0430\u0447\u0435\u0441\u0442\u0432\u043E:</label>
          <select id="quality">
            <option value="max">\u041C\u0430\u043A\u0441\u0438\u043C\u0430\u043B\u044C\u043D\u043E\u0435</option>
            <option value="1080">1080p</option>
            <option value="720">720p</option>
            <option value="480">480p</option>
            <option value="360">360p</option>
          </select>
        </div>
        <div class="row">
          <label>\u0410\u0432\u0442\u043E\u0440\u0438\u0437\u0430\u0446\u0438\u044F:</label>
          <select id="authMode">
            <option value="none">\u0411\u0435\u0437 \u0430\u0432\u0442\u043E\u0440\u0438\u0437\u0430\u0446\u0438\u0438</option>
            <option value="browser">\u0418\u0437 \u0431\u0440\u0430\u0443\u0437\u0435\u0440\u0430</option>
            <option value="file">Cookies.txt</option>
          </select>
          <select id="browserSelect" style="display:none;">
            <option value="chrome">Chrome</option>
            <option value="edge">Edge</option>
            <option value="firefox">Firefox</option>
            <option value="brave">Brave</option>
            <option value="opera">Opera</option>
            <option value="vivaldi">Vivaldi</option>
          </select>
          <input type="text" id="cookiesFile" placeholder="\u041F\u0443\u0442\u044C \u043A cookies.txt" style="display:none; flex:1;">
          <button id="browseCookies" style="display:none;">\u041E\u0431\u0437\u043E\u0440</button>
        </div>
        <div class="row">
          <label>\u0421\u043E\u0445\u0440\u0430\u043D\u0438\u0442\u044C \u0432:</label>
          <input type="text" id="outputDir" placeholder="\u041F\u0430\u043F\u043A\u0430 \u0434\u043B\u044F \u0441\u043E\u0445\u0440\u0430\u043D\u0435\u043D\u0438\u044F" style="flex:1;">
          <button id="browseDir">\u041E\u0431\u0437\u043E\u0440</button>
        </div>
        <div class="row" style="margin-top:10px;">
          <button id="download" class="primary" style="min-width:120px;">\u0421\u043A\u0430\u0447\u0430\u0442\u044C</button>
          <button id="cancel" class="danger" style="display:none;">\u041E\u0442\u043C\u0435\u043D\u0430</button>
        </div>
        <div id="progress" class="progress-container">
          <div class="progress-status" id="progressStatus">\u041E\u0436\u0438\u0434\u0430\u043D\u0438\u0435...</div>
          <div class="progress-track"><div class="progress-bar" id="progressBar"></div></div>
        </div>
      </div>`;
      this.root.getElementById("download").addEventListener("click", () => this.onDownload());
      this.root.getElementById("cancel").addEventListener("click", () => this.onCancel());
      this.root.getElementById("browseDir").addEventListener("click", () => this.onBrowseDir());
      this.root.getElementById("browseCookies").addEventListener("click", () => this.onBrowseCookies());
      this.root.getElementById("authMode").addEventListener("change", () => this.onAuthChange());
      const authMode = this.root.getElementById("authMode");
      authMode.addEventListener("change", () => this.onAuthChange());
      void this.setupListeners();
      void this.loadDefaults();
    }
    disconnectedCallback() {
      this.unlisten?.();
      this.unlistenProgress?.();
      this.unlistenStatus?.();
      this.unlistenFinished?.();
      this.unlistenError?.();
      this.unlistenCancelled?.();
    }
    async setupListeners() {
      this.unlistenProgress = await listen("ytdlp:download-progress", (e) => {
        const pct = e.payload;
        const bar = this.root.getElementById("progressBar");
        bar.style.width = `${pct}%`;
      });
      this.unlistenStatus = await listen("ytdlp:download-status", (e) => {
        this.root.getElementById("progressStatus").textContent = e.payload;
      });
      this.unlistenFinished = await listen("ytdlp:download-finished", () => {
        this.root.getElementById("progressStatus").textContent = "\u0413\u043E\u0442\u043E\u0432\u043E! \u0412\u0438\u0434\u0435\u043E \u0441\u043A\u0430\u0447\u0430\u043D\u043E.";
        this.setBusy(false);
        toast("\u0412\u0438\u0434\u0435\u043E \u0443\u0441\u043F\u0435\u0448\u043D\u043E \u0441\u043A\u0430\u0447\u0430\u043D\u043E!");
      });
      this.unlistenError = await listen("ytdlp:download-error", (e) => {
        const msg = e.payload;
        this.root.getElementById("progressStatus").textContent = `\u041E\u0448\u0438\u0431\u043A\u0430: ${msg}`;
        this.setBusy(false);
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430: ${msg}`, "error");
      });
      this.unlistenCancelled = await listen("ytdlp:download-cancelled", () => {
        this.root.getElementById("progressStatus").textContent = "\u041E\u0442\u043C\u0435\u043D\u0435\u043D\u043E";
        this.setBusy(false);
      });
    }
    async loadDefaults() {
      try {
        const cfg = await invoke("plugin:ytdlp|get_ytdlp_status", {});
        const outputDir = this.root.getElementById("outputDir");
        if (cfg.download_path) {
          outputDir.value = cfg.download_path;
        }
      } catch {
      }
    }
    onAuthChange() {
      const mode = this.root.getElementById("authMode").value;
      const browserSelect = this.root.getElementById("browserSelect");
      const cookiesFile = this.root.getElementById("cookiesFile");
      const browseCookies = this.root.getElementById("browseCookies");
      browserSelect.style.display = mode === "browser" ? "inline-block" : "none";
      cookiesFile.style.display = mode === "file" ? "inline-block" : "none";
      browseCookies.style.display = mode === "file" ? "inline-block" : "none";
    }
    setBusy(busy) {
      this.busy = busy;
      const dlBtn = this.root.getElementById("download");
      const cancelBtn = this.root.getElementById("cancel");
      dlBtn.disabled = busy;
      dlBtn.style.display = busy ? "none" : "inline-block";
      cancelBtn.style.display = busy ? "inline-block" : "none";
      this.root.getElementById("progress").style.display = busy ? "block" : "none";
    }
    async onBrowseDir() {
      try {
        const sel = await open({ directory: true });
        if (!sel) return;
        const path = Array.isArray(sel) ? sel[0] : sel;
        if (!path) return;
        this.root.getElementById("outputDir").value = path;
      } catch (e) {
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430: ${e}`, "error");
      }
    }
    async onBrowseCookies() {
      try {
        const sel = await open({ filters: [{ name: "Text", extensions: ["txt"] }] });
        if (!sel) return;
        const path = Array.isArray(sel) ? sel[0] : sel;
        if (!path) return;
        this.root.getElementById("cookiesFile").value = path;
      } catch (e) {
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430: ${e}`, "error");
      }
    }
    async onDownload() {
      if (this.busy) return;
      const url = this.root.getElementById("url").value.trim();
      if (!url) {
        toast("\u0412\u0432\u0435\u0434\u0438\u0442\u0435 URL \u0432\u0438\u0434\u0435\u043E", "error");
        return;
      }
      const format = this.root.getElementById("format").value;
      const quality = this.root.getElementById("quality").value;
      const outputDir = this.root.getElementById("outputDir").value.trim();
      const authMode = this.root.getElementById("authMode").value;
      const browser = this.root.getElementById("browserSelect").value;
      const cookiesFile = this.root.getElementById("cookiesFile").value.trim();
      if (!outputDir) {
        toast("\u0423\u043A\u0430\u0436\u0438\u0442\u0435 \u043F\u0430\u043F\u043A\u0443 \u0434\u043B\u044F \u0441\u043E\u0445\u0440\u0430\u043D\u0435\u043D\u0438\u044F", "error");
        return;
      }
      if (authMode === "file" && !cookiesFile) {
        toast("\u0423\u043A\u0430\u0436\u0438\u0442\u0435 \u043F\u0443\u0442\u044C \u043A cookies.txt", "error");
        return;
      }
      this.setBusy(true);
      this.root.getElementById("progressBar").style.width = "0%";
      this.root.getElementById("progressStatus").textContent = "\u041F\u043E\u0434\u0433\u043E\u0442\u043E\u0432\u043A\u0430...";
      try {
        await downloadVideo({
          url,
          format,
          quality,
          outputDir,
          authMode,
          browser: authMode === "browser" ? browser : null,
          cookiesFile: authMode === "file" ? cookiesFile : null
        });
      } catch (e) {
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430 \u0441\u043A\u0430\u0447\u0438\u0432\u0430\u043D\u0438\u044F: ${e}`, "error");
        this.setBusy(false);
      }
    }
    async onCancel() {
      try {
        await cancelDownload();
      } catch (e) {
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430 \u043E\u0442\u043C\u0435\u043D\u044B: ${e}`, "error");
      }
    }
  };
  if (!customElements.get("ytdlp-panel")) {
    customElements.define("ytdlp-panel", YtdlpPanel);
  }
  if (!customElements.get("ytdlp-download-panel")) {
    customElements.define("ytdlp-download-panel", YtdlpDownloadPanel);
  }

  // guest-js/index.ts
  function getYtdlpStatus() {
    return invoke("plugin:ytdlp|get_ytdlp_status");
  }
  function installYtdlp() {
    return invoke("plugin:ytdlp|install_ytdlp");
  }
  function checkYtdlpUpdate() {
    return invoke("plugin:ytdlp|check_ytdlp_update");
  }
  function installYtdlpUpdate() {
    return invoke("plugin:ytdlp|install_ytdlp_update");
  }
  function setYtdlpDir(path) {
    return invoke("plugin:ytdlp|set_ytdlp_dir", { path });
  }
  function setFfmpegPath(path) {
    return invoke("plugin:ytdlp|set_ffmpeg_path", { path });
  }
  function installDeno() {
    return invoke("plugin:ytdlp|install_deno");
  }
  function fetchVideoInfo(url) {
    return invoke("plugin:ytdlp|fetch_video_info", { url });
  }
  function downloadVideo(params) {
    return invoke("plugin:ytdlp|download_video", {
      url: params.url,
      format: params.format,
      quality: params.quality,
      outputDir: params.outputDir,
      authMode: params.authMode,
      browser: params.browser ?? null,
      cookiesFile: params.cookiesFile ?? null
    });
  }
  function cancelDownload() {
    return invoke("plugin:ytdlp|cancel_download");
  }

  // guest-js/iife-entry.ts
  var g4 = window;
  if ("__TAURI__" in window) {
    const tauri = g4.__TAURI__;
    if (tauri && !tauri["ytdlp"]) {
      Object.defineProperty(tauri, "ytdlp", {
        configurable: true,
        value: {
          cancelDownload,
          checkYtdlpUpdate,
          downloadVideo,
          fetchVideoInfo,
          getYtdlpStatus,
          installDeno,
          installYtdlp,
          installYtdlpUpdate,
          setFfmpegPath,
          setYtdlpDir
        }
      });
    }
  }
})();
