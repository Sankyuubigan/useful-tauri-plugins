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
  label { color: var(--text-muted, #999); min-width: 60px; }
  button { padding: 6px 10px; cursor: pointer; border-radius: 6px; border: 1px solid var(--border, #333);
           background: var(--session-hover, #eee); color: var(--text, #eee); font: inherit; font-size: 13px; }
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
  .files { margin: 6px 0 0 0; padding-left: 18px; }
  .files li { margin: 2px 0; }
  .ok { color: #6fbf73; }
  .missing { color: var(--text-muted, #999); }
  .overlay { position: fixed; inset: 0; background: rgba(0,0,0,.5); display: none; align-items: center;
             justify-content: center; z-index: 2000; }
  .overlay.open { display: flex; }
  .box { background: var(--bg-elevated, #1c1c1c); border: 1px solid var(--border, #333); border-radius: 12px;
         padding: 20px; max-width: 520px; width: 90%; }
  .box h3 { margin: 0 0 12px; font-size: 16px; }
  .box p { margin: 8px 0; color: var(--text-muted, #aaa); }
  .overlay-buttons { display: flex; gap: 10px; justify-content: flex-end; margin-top: 16px; }
`;
  function esc(s) {
    return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
  }
  function formatBytes(n) {
    if (!isFinite(n) || n <= 0) return "0 B";
    const units = ["B", "KB", "MB", "GB"];
    let i = 0;
    while (n >= 1024 && i < units.length - 1) {
      n /= 1024;
      i++;
    }
    return `${n.toFixed(1)} ${units[i]}`;
  }
  function toast(msg, kind = "success") {
    const el = document.createElement("div");
    el.textContent = msg;
    el.style.cssText = `position:fixed; right:16px; bottom:16px; z-index:5000; max-width:420px; padding:10px 14px;border-radius:8px; font-size:13px; color:#fff; background:${kind === "success" ? "rgba(60,140,70,.95)" : "rgba(180,60,60,.95)"};`;
    document.body.appendChild(el);
    setTimeout(() => el.remove(), 4200);
  }
  var ImageEnginePanel = class extends HTMLElement {
    constructor() {
      super(...arguments);
      this.busy = false;
      this.applied = "auto";
    }
    connectedCallback() {
      if (this.root) return;
      this.root = this.attachShadow({ mode: "open" });
      this.root.innerHTML = `
      <style>${STYLE}</style>
      <div>
        <div class="row"><label>\u0421\u0442\u0430\u0442\u0443\u0441:</label><span id="status" class="hint">\u041F\u0440\u043E\u0432\u0435\u0440\u043A\u0430\u2026</span></div>
        <div class="row"><label>GPU:</label><span id="gpu" class="hint"></span></div>
        <div class="row"><label>\u041F\u0443\u0442\u044C:</label><span id="path" class="hint" style="word-break:break-all; flex:1;"></span></div>
        <div class="row" style="margin-top:8px;">
          <label>\u0411\u0435\u043A\u0435\u043D\u0434:</label>
          <select id="variant" style="flex:1; min-width:200px;"><option value="">\u2026</option></select>
          <button id="apply" class="primary" style="display:none;">\u041F\u0440\u0438\u043C\u0435\u043D\u0438\u0442\u044C</button>
        </div>
        <div id="variantHint" class="hint"></div>
        <div class="row" style="margin-top:10px;">
          <button id="install" class="primary">\u0423\u0441\u0442\u0430\u043D\u043E\u0432\u0438\u0442\u044C</button>
          <button id="checkUpdate" class="secondary" style="display:none;">\u041F\u0440\u043E\u0432\u0435\u0440\u0438\u0442\u044C \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u0435</button>
          <button id="installUpdate" class="primary" style="display:none;">\u041E\u0431\u043D\u043E\u0432\u0438\u0442\u044C</button>
          <button id="remove" class="danger" style="display:none;">\u0423\u0434\u0430\u043B\u0438\u0442\u044C</button>
          <button id="setDir" class="secondary" style="display:none;">\u0418\u0437\u043C\u0435\u043D\u0438\u0442\u044C \u043F\u0443\u0442\u044C</button>
        </div>
        <div id="progress" class="progress-container">
          <div class="progress-status" id="progressStatus">0 MB / 0 MB</div>
          <div class="progress-track"><div class="progress-bar" id="progressBar"></div></div>
        </div>
        <div id="overlay" class="overlay">
          <div class="box">
            <h3>\u0423\u0434\u0430\u043B\u0438\u0442\u044C \u0434\u0432\u0438\u0436\u043E\u043A \u0438\u0437\u043E\u0431\u0440\u0430\u0436\u0435\u043D\u0438\u0439?</h3>
            <p>GPU-\u0443\u0441\u043A\u043E\u0440\u0435\u043D\u0438\u0435 \u0433\u0435\u043D\u0435\u0440\u0430\u0446\u0438\u0438 \u043E\u0442\u043A\u043B\u044E\u0447\u0438\u0442\u0441\u044F.</p>
            <div class="overlay-buttons">
              <button id="delCancel" class="secondary">\u041E\u0442\u043C\u0435\u043D\u0430</button>
              <button id="delOk" class="danger">\u0423\u0434\u0430\u043B\u0438\u0442\u044C</button>
            </div>
          </div>
        </div>
      </div>`;
      this.root.getElementById("variant").addEventListener("change", () => this.onVariantChange());
      this.root.getElementById("apply").addEventListener("click", () => this.onApplyVariant());
      this.root.getElementById("install").addEventListener("click", () => this.onInstall());
      this.root.getElementById("checkUpdate").addEventListener("click", () => this.onCheckUpdate());
      this.root.getElementById("installUpdate").addEventListener("click", () => this.onInstallUpdate());
      this.root.getElementById("remove").addEventListener("click", () => {
        this.root.getElementById("overlay").classList.add("open");
      });
      this.root.getElementById("delCancel").addEventListener("click", () => {
        this.root.getElementById("overlay").classList.remove("open");
      });
      this.root.getElementById("delOk").addEventListener("click", () => {
        this.root.getElementById("overlay").classList.remove("open");
        void this.onRemove();
      });
      this.root.getElementById("setDir").addEventListener("click", () => this.onSetDir());
      void this.refresh();
      void listen("downloader:progress", (e) => {
        const p = e.payload;
        if (p.kind && !["engine", "image-model"].includes(p.kind)) return;
        this.setProgress(p.downloaded, p.total);
        if (p.status === "done" || p.status === "error") {
          setTimeout(() => this.showProgress(false), 400);
        }
      }).then((u) => {
        this.unlisten = u;
      }).catch(() => {
      });
    }
    disconnectedCallback() {
      this.unlisten?.();
    }
    setStatus(s) {
      const el = this.root.getElementById("status");
      if (el) el.textContent = s;
    }
    setProgress(downloaded, total) {
      const pct = total > 0 ? downloaded / total * 100 : 0;
      const bar = this.root.getElementById("progressBar");
      bar.style.width = `${pct}%`;
      const st = this.root.getElementById("progressStatus");
      st.textContent = `${formatBytes(downloaded)} / ${total > 0 ? formatBytes(total) : "?"}${total > 0 ? ` (${pct.toFixed(0)}%)` : ""}`;
    }
    showProgress(on) {
      this.root.getElementById("progress").style.display = on ? "block" : "none";
    }
    hintText(st, value) {
      const lines = [];
      if (value === "auto") {
        const resolved = (st.available_variants || []).find((v) => v.id === st.resolved_variant);
        lines.push(`\u0410\u0432\u0442\u043E-\u043F\u043E\u0434\u0431\u043E\u0440 \u0434\u043B\u044F \u044D\u0442\u043E\u0439 \u043C\u0430\u0448\u0438\u043D\u044B: ${resolved ? resolved.label : st.resolved_variant || "\u2014"}.`);
      } else {
        const v = (st.available_variants || []).find((x) => x.id === value);
        if (v && v.note) lines.push(v.note);
      }
      const installed = st.installed_variants || [];
      lines.push(installed.length > 0 ? `\u0423\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D\u044B: ${installed.join(", ")}.` : "\u041D\u0438 \u043E\u0434\u0438\u043D \u0431\u0435\u043A\u0435\u043D\u0434 \u0435\u0449\u0451 \u043D\u0435 \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D.");
      return lines.join("\n");
    }
    async refresh() {
      let st;
      try {
        st = await getImageEngineStatus();
      } catch (e) {
        this.setStatus(`\u041E\u0448\u0438\u0431\u043A\u0430: ${e}`);
        return;
      }
      this.setStatus(st.message);
      this.root.getElementById("gpu").textContent = st.has_nvidia ? st.gpu_name || "NVIDIA" : "\u041D\u0435 \u043E\u0431\u043D\u0430\u0440\u0443\u0436\u0435\u043D\u0430 (CPU-\u0440\u0435\u0436\u0438\u043C)";
      this.root.getElementById("path").textContent = st.path || "\u2014";
      const sel = this.root.getElementById("variant");
      sel.innerHTML = "";
      const auto = document.createElement("option");
      auto.value = "auto";
      auto.textContent = "\u0410\u0432\u0442\u043E (\u0440\u0435\u043A\u043E\u043C\u0435\u043D\u0434\u0443\u0435\u0442\u0441\u044F)";
      sel.appendChild(auto);
      for (const v of st.available_variants || []) {
        const o = document.createElement("option");
        o.value = v.id;
        o.textContent = v.installed ? `${v.label} \u2014 \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D` : v.recommended ? `${v.label} (\u0440\u0435\u043A\u043E\u043C\u0435\u043D\u0434\u0443\u0435\u0442\u0441\u044F)` : v.label;
        sel.appendChild(o);
      }
      this.applied = st.selected_variant || "auto";
      sel.value = this.applied;
      this.root.getElementById("variantHint").textContent = this.hintText(st, sel.value);
      this.root.getElementById("apply").style.display = sel.value === this.applied ? "none" : "inline-block";
      this.applyButtonStates(st);
    }
    applyButtonStates(st) {
      const installed = st.installed;
      this.root.getElementById("install").style.display = installed ? "none" : "inline-block";
      this.root.getElementById("checkUpdate").style.display = installed ? "inline-block" : "none";
      this.root.getElementById("remove").style.display = installed ? "inline-block" : "none";
      this.root.getElementById("setDir").style.display = "inline-block";
    }
    async onVariantChange() {
      const sel = this.root.getElementById("variant");
      const value = sel.value;
      this.root.getElementById("apply").style.display = value === this.applied ? "none" : "inline-block";
      try {
        const s = await getImageEngineStatus();
        this.root.getElementById("variantHint").textContent = this.hintText(s, value);
      } catch {
      }
    }
    async onApplyVariant() {
      const btn = this.root.getElementById("apply");
      const variant = this.root.getElementById("variant").value;
      btn.disabled = true;
      try {
        let installedVariants = [];
        try {
          installedVariants = (await getImageEngineStatus()).installed_variants || [];
        } catch {
        }
        if (variant !== "auto" && !installedVariants.includes(variant)) {
          this.showProgress(true);
          this.setProgress(0, 0);
        }
        await setImageEngineVariant(variant);
        toast(variant === "auto" ? "\u0411\u0435\u043A\u0435\u043D\u0434: \u0430\u0432\u0442\u043E-\u043F\u043E\u0434\u0431\u043E\u0440." : "\u0411\u0435\u043A\u0435\u043D\u0434 \u043F\u0440\u0438\u043C\u0435\u043D\u0451\u043D.");
        await this.refresh();
      } catch (e) {
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430 \u0441\u043C\u0435\u043D\u044B \u0431\u0435\u043A\u0435\u043D\u0434\u0430: ${e}`, "error");
      } finally {
        btn.disabled = false;
        this.showProgress(false);
        await this.refresh();
      }
    }
    async onInstall() {
      const btn = this.root.getElementById("install");
      btn.disabled = true;
      this.showProgress(true);
      this.root.getElementById("progressStatus").textContent = "\u041F\u043E\u0434\u0433\u043E\u0442\u043E\u0432\u043A\u0430\u2026";
      try {
        await installImageEngine();
        await this.refresh();
        toast("\u0414\u0432\u0438\u0436\u043E\u043A \u0438\u0437\u043E\u0431\u0440\u0430\u0436\u0435\u043D\u0438\u0439 \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D!");
      } catch (e) {
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430 \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043A\u0438 \u0434\u0432\u0438\u0436\u043A\u0430: ${e}`, "error");
      } finally {
        btn.disabled = false;
        this.showProgress(false);
      }
    }
    async onCheckUpdate() {
      const btn = this.root.getElementById("checkUpdate");
      btn.disabled = true;
      const prevLabel = btn.textContent;
      btn.textContent = "\u041F\u0440\u043E\u0432\u0435\u0440\u043A\u0430\u2026";
      this.root.getElementById("installUpdate").style.display = "none";
      try {
        const newTag = await checkImageEngineUpdate();
        if (newTag) {
          this.setStatus(`\u0414\u043E\u0441\u0442\u0443\u043F\u043D\u043E \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u0435 \u0434\u0432\u0438\u0436\u043A\u0430: ${newTag}`);
          this.root.getElementById("installUpdate").style.display = "inline-block";
        } else {
          this.setStatus("\u0414\u0432\u0438\u0436\u043E\u043A \u0438\u0437\u043E\u0431\u0440\u0430\u0436\u0435\u043D\u0438\u0439 \u0430\u043A\u0442\u0443\u0430\u043B\u0435\u043D");
        }
      } catch (e) {
        this.setStatus("");
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430 \u043F\u0440\u043E\u0432\u0435\u0440\u043A\u0438 \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u044F \u0434\u0432\u0438\u0436\u043A\u0430: ${e}`, "error");
      } finally {
        btn.disabled = false;
        btn.textContent = prevLabel || "\u041F\u0440\u043E\u0432\u0435\u0440\u0438\u0442\u044C \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u0435";
      }
    }
    async onInstallUpdate() {
      const btn = this.root.getElementById("installUpdate");
      btn.disabled = true;
      this.showProgress(true);
      this.root.getElementById("progressStatus").textContent = "\u041E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u0435\u2026";
      try {
        await installImageEngineUpdate();
        this.root.getElementById("installUpdate").style.display = "none";
        await this.refresh();
        toast("\u0414\u0432\u0438\u0436\u043E\u043A \u0438\u0437\u043E\u0431\u0440\u0430\u0436\u0435\u043D\u0438\u0439 \u043E\u0431\u043D\u043E\u0432\u043B\u0451\u043D.", "success");
      } catch (e) {
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430 \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u044F \u0434\u0432\u0438\u0436\u043A\u0430: ${e}`, "error");
      } finally {
        btn.disabled = false;
        this.showProgress(false);
      }
    }
    async onRemove() {
      if (this.busy) return;
      this.busy = true;
      try {
        await removeImageEngine();
        await this.refresh();
        toast("\u0414\u0432\u0438\u0436\u043E\u043A \u0438\u0437\u043E\u0431\u0440\u0430\u0436\u0435\u043D\u0438\u0439 \u0443\u0434\u0430\u043B\u0451\u043D.");
      } catch (e) {
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430 \u0443\u0434\u0430\u043B\u0435\u043D\u0438\u044F \u0434\u0432\u0438\u0436\u043A\u0430: ${e}`, "error");
        void this.refresh();
      } finally {
        this.busy = false;
      }
    }
    async onSetDir() {
      try {
        const sel = await open({ directory: true });
        if (!sel) return;
        const path = Array.isArray(sel) ? sel[0] : sel;
        if (!path) return;
        await setImageEngineDir(path);
        await this.refresh();
        toast("\u041F\u0443\u0442\u044C \u0434\u0432\u0438\u0436\u043A\u0430 \u0438\u0437\u043C\u0435\u043D\u0451\u043D.");
      } catch (e) {
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430 \u0438\u0437\u043C\u0435\u043D\u0435\u043D\u0438\u044F \u043F\u0443\u0442\u0438: ${e}`, "error");
      }
    }
  };
  var ImageBundlePanel = class extends HTMLElement {
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
        <div class="row"><label>\u041D\u0430\u0431\u043E\u0440:</label><span id="label" class="hint">\u2026</span></div>
        <div id="note" class="hint"></div>
        <ul class="files" id="files"></ul>
        <div class="row" style="margin-top:8px;"><label>\u0414\u0438\u0441\u043A:</label><span id="disk" class="hint"></span></div>
        <div class="row"><label>\u041F\u0430\u043C\u044F\u0442\u044C:</label><span id="mem" class="hint"></span></div>
        <div class="row"><label>\u041F\u0430\u043F\u043A\u0430:</label><span id="dir" class="hint" style="word-break:break-all; flex:1;"></span></div>
        <div class="row" style="margin-top:10px;">
          <button id="download" class="primary">\u0421\u043A\u0430\u0447\u0430\u0442\u044C \u043D\u0430\u0431\u043E\u0440</button>
          <button id="remove" class="danger" style="display:none;">\u0423\u0434\u0430\u043B\u0438\u0442\u044C \u0444\u0430\u0439\u043B\u044B</button>
        </div>
        <div id="progress" class="progress-container">
          <div class="progress-status" id="progressStatus">0 MB / 0 MB</div>
          <div class="progress-track"><div class="progress-bar" id="progressBar"></div></div>
        </div>
      </div>`;
      this.root.getElementById("download").addEventListener("click", () => this.onDownload());
      this.root.getElementById("remove").addEventListener("click", () => this.onRemove());
      void this.refresh();
      void listen("downloader:progress", (e) => {
        const p = e.payload;
        if (p.kind && p.kind !== "image-model") return;
        this.setProgress(p.downloaded, p.total);
        if (p.status === "done" || p.status === "error") {
          setTimeout(() => {
            this.showProgress(false);
            void this.refresh();
          }, 400);
        }
      }).then((u) => {
        this.unlisten = u;
      }).catch(() => {
      });
    }
    disconnectedCallback() {
      this.unlisten?.();
    }
    setProgress(downloaded, total) {
      const pct = total > 0 ? downloaded / total * 100 : 0;
      const bar = this.root.getElementById("progressBar");
      bar.style.width = `${pct}%`;
      const st = this.root.getElementById("progressStatus");
      st.textContent = `${formatBytes(downloaded)} / ${total > 0 ? formatBytes(total) : "?"}${total > 0 ? ` (${pct.toFixed(0)}%)` : ""}`;
    }
    showProgress(on) {
      this.root.getElementById("progress").style.display = on ? "block" : "none";
    }
    async refresh() {
      let info;
      try {
        info = await getImageBundleInfo();
      } catch (e) {
        this.root.getElementById("label").textContent = `\u041E\u0448\u0438\u0431\u043A\u0430: ${e}`;
        return;
      }
      this.root.getElementById("label").textContent = info.label;
      this.root.getElementById("note").textContent = info.note || "";
      const ul = this.root.getElementById("files");
      ul.innerHTML = "";
      let anyExists = false;
      for (const f of info.files) {
        if (f.exists) anyExists = true;
        const li = document.createElement("li");
        li.className = f.exists ? "ok" : "missing";
        const size = f.size_bytes ? ` \u2014 ${formatBytes(f.size_bytes)}` : "";
        li.textContent = `${f.exists ? "\u2713" : "\u25CB"} ${esc(f.filename)} (${f.role})${size}`;
        ul.appendChild(li);
      }
      const totalGb = (info.total_bytes / 1024 / 1024 / 1024).toFixed(1);
      this.root.getElementById("disk").textContent = `~${totalGb} \u0413\u0411 \u043D\u0430 \u0434\u0438\u0441\u043A\u0435 \xB7 \u0441\u0432\u043E\u0431\u043E\u0434\u043D\u043E ${info.free_space_gb} \u0413\u0411`;
      this.root.getElementById("mem").textContent = `GPU: ${info.vram_fast_gb ?? "?"} \u0413\u0411 (\u0431\u044B\u0441\u0442\u0440\u043E) / ${info.vram_min_gb ?? "?"} \u0413\u0411 (\u043C\u0438\u043D.) \xB7 RAM \u2265${info.ram_min_gb ?? "?"} \u0413\u0411`;
      this.root.getElementById("dir").textContent = info.save_dir;
      const dlBtn = this.root.getElementById("download");
      dlBtn.style.display = info.fully_downloaded ? "none" : "inline-block";
      dlBtn.textContent = anyExists ? "\u0414\u043E\u043A\u0430\u0447\u0430\u0442\u044C \u0444\u0430\u0439\u043B\u044B" : "\u0421\u043A\u0430\u0447\u0430\u0442\u044C \u043D\u0430\u0431\u043E\u0440";
      this.root.getElementById("remove").style.display = info.fully_downloaded ? "inline-block" : "none";
      try {
        const mem = await estimateImageMemory();
        const gb = (mb) => `${(mb / 1024).toFixed(1)} \u0413\u0411`;
        this.root.getElementById("mem").textContent = `GPU: ${info.vram_fast_gb ?? "?"} \u0413\u0411 (\u0431\u044B\u0441\u0442\u0440\u043E) / ${info.vram_min_gb ?? "?"} \u0413\u0411 (\u043C\u0438\u043D.) \xB7 \u0441\u0435\u0439\u0447\u0430\u0441 \u0441\u0432\u043E\u0431\u043E\u0434\u043D\u043E ${gb(mem.estimate.vram_free_mb)} VRAM / ${gb(mem.estimate.ram_free_mb)} RAM \u2014 ${mem.message}`;
      } catch {
      }
    }
    async onDownload() {
      if (this.busy) return;
      this.busy = true;
      const btn = this.root.getElementById("download");
      btn.disabled = true;
      this.showProgress(true);
      this.setProgress(0, 0);
      try {
        const cur = await getImageBundleInfo();
        let target = cur.save_dir;
        const anyExists = cur.files.some((f) => f.exists);
        if (!anyExists || !target) {
          const sel = await open({ directory: true, title: "\u041A\u0443\u0434\u0430 \u0441\u043A\u0430\u0447\u0430\u0442\u044C \u043D\u0430\u0431\u043E\u0440?" });
          if (!sel) return;
          const base = Array.isArray(sel) ? sel[0] : sel;
          if (!base) return;
          const sep = base.includes("\\") ? "\\" : "/";
          const tail = base.split(/[\\/]/).pop() ?? "";
          target = tail === cur.bundle_name ? base : base.endsWith(sep) ? `${base}${cur.bundle_name}` : `${base}${sep}${cur.bundle_name}`;
        }
        await downloadImageBundle(target);
        notifyBundleChanged();
        toast("\u041D\u0430\u0431\u043E\u0440 \u0438\u0437\u043E\u0431\u0440\u0430\u0436\u0435\u043D\u0438\u0439 \u0441\u043A\u0430\u0447\u0430\u043D!");
      } catch (e) {
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430 \u0441\u043A\u0430\u0447\u0438\u0432\u0430\u043D\u0438\u044F \u043D\u0430\u0431\u043E\u0440\u0430: ${e}`, "error");
      } finally {
        this.busy = false;
        btn.disabled = false;
        this.showProgress(false);
        await this.refresh();
      }
    }
    async onRemove() {
      if (this.busy) return;
      this.busy = true;
      try {
        await removeImageBundle();
        notifyBundleChanged();
        toast("\u0424\u0430\u0439\u043B\u044B \u043D\u0430\u0431\u043E\u0440\u0430 \u0443\u0434\u0430\u043B\u0435\u043D\u044B.");
      } catch (e) {
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430 \u0443\u0434\u0430\u043B\u0435\u043D\u0438\u044F: ${e}`, "error");
      } finally {
        this.busy = false;
        await this.refresh();
      }
    }
  };
  var ImageModelsPanel = class extends HTMLElement {
    constructor() {
      super(...arguments);
      this.busy = false;
      this.onChangedBound = () => {
        void this.refresh();
      };
    }
    connectedCallback() {
      if (this.root) return;
      this.root = this.attachShadow({ mode: "open" });
      this.root.innerHTML = `
      <style>${STYLE}</style>
      <div>
        <div class="row"><label>\u041F\u0430\u043F\u043A\u0430:</label><span id="dir" class="hint" style="word-break:break-all; flex:1;">\u2026</span></div>
        <div id="status" class="hint"></div>
        <ul class="files" id="files"></ul>
        <div class="row" style="margin-top:10px;">
          <button id="add" class="secondary">+ \u0414\u043E\u0431\u0430\u0432\u0438\u0442\u044C \u043D\u0430\u0431\u043E\u0440</button>
          <button id="resume" class="primary" style="display:none;">\u0414\u043E\u043A\u0430\u0447\u0430\u0442\u044C \u0444\u0430\u0439\u043B\u044B</button>
        </div>
        <div id="progress" class="progress-container">
          <div class="progress-status" id="progressStatus">0 MB / 0 MB</div>
          <div class="progress-track"><div class="progress-bar" id="progressBar"></div></div>
        </div>
      </div>`;
      this.root.getElementById("add").addEventListener("click", () => this.onAdd());
      this.root.getElementById("resume").addEventListener("click", () => this.onResume());
      document.addEventListener("image:bundle-changed", this.onChangedBound);
      void this.refresh();
      void listen("downloader:progress", (e) => {
        const p = e.payload;
        if (p.kind && p.kind !== "image-model") return;
        this.setProgress(p.downloaded, p.total);
        if (p.status === "done" || p.status === "error") {
          setTimeout(() => {
            this.showProgress(false);
            void this.refresh();
          }, 400);
        }
      }).then((u) => {
        this.unlisten = u;
      }).catch(() => {
      });
    }
    disconnectedCallback() {
      document.removeEventListener("image:bundle-changed", this.onChangedBound);
      this.unlisten?.();
    }
    setProgress(downloaded, total) {
      const pct = total > 0 ? downloaded / total * 100 : 0;
      const bar = this.root.getElementById("progressBar");
      if (bar) bar.style.width = `${pct}%`;
      const st = this.root.getElementById("progressStatus");
      if (st) st.textContent = `${formatBytes(downloaded)} / ${total > 0 ? formatBytes(total) : "?"}${total > 0 ? ` (${pct.toFixed(0)}%)` : ""}`;
    }
    showProgress(on) {
      const prg = this.root.getElementById("progress");
      if (prg) prg.style.display = on ? "block" : "none";
    }
    async refresh() {
      let info;
      try {
        info = await getImageBundleInfo();
      } catch (e) {
        this.root.getElementById("status").textContent = `\u041E\u0448\u0438\u0431\u043A\u0430: ${e}`;
        return;
      }
      this.root.getElementById("dir").textContent = info.save_dir;
      const ul = this.root.getElementById("files");
      ul.innerHTML = "";
      for (const f of info.files) {
        const li = document.createElement("li");
        li.className = f.exists ? "ok" : "missing";
        const size = f.size_bytes ? ` \u2014 ${formatBytes(f.size_bytes)}` : "";
        li.textContent = `${f.exists ? "\u2713" : "\u25CB"} ${esc(f.filename)} (${f.role})${size}`;
        ul.appendChild(li);
      }
      const missing = info.files.filter((f) => !f.exists).length;
      this.root.getElementById("status").textContent = info.fully_downloaded ? `\u041D\u0430\u0431\u043E\u0440 \u0432\u0430\u043B\u0438\u0434\u0435\u043D: ${info.files.length}/${info.files.length} \u0444\u0430\u0439\u043B\u0430 \u043D\u0430 \u043C\u0435\u0441\u0442\u0435.` : `\u041D\u0435 \u0445\u0432\u0430\u0442\u0430\u0435\u0442 \u0444\u0430\u0439\u043B\u043E\u0432: ${missing} \u0438\u0437 ${info.files.length}.`;
      const resumeBtn = this.root.getElementById("resume");
      if (resumeBtn) {
        resumeBtn.style.display = info.fully_downloaded ? "none" : "inline-block";
      }
    }
    async onResume() {
      if (this.busy) return;
      this.busy = true;
      const btn = this.root.getElementById("resume");
      if (btn) btn.disabled = true;
      this.showProgress(true);
      this.setProgress(0, 0);
      try {
        const cur = await getImageBundleInfo();
        await downloadImageBundle(cur.save_dir);
        notifyBundleChanged();
        toast("\u041D\u0435\u0434\u043E\u0441\u0442\u0430\u044E\u0449\u0438\u0435 \u0444\u0430\u0439\u043B\u044B \u043D\u0430\u0431\u043E\u0440\u0430 \u0441\u043A\u0430\u0447\u0430\u043D\u044B!");
      } catch (e) {
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430 \u0434\u043E\u043A\u0430\u0447\u0438\u0432\u0430\u043D\u0438\u044F \u043D\u0430\u0431\u043E\u0440\u0430: ${e}`, "error");
      } finally {
        this.busy = false;
        if (btn) btn.disabled = false;
        this.showProgress(false);
        await this.refresh();
      }
    }
    async onAdd() {
      if (this.busy) return;
      this.busy = true;
      const btn = this.root.getElementById("add");
      btn.disabled = true;
      try {
        const sel = await open({ directory: true, title: "\u041F\u0430\u043F\u043A\u0430 \u043D\u0430\u0431\u043E\u0440\u0430 ImageGEN" });
        if (!sel) return;
        const path = Array.isArray(sel) ? sel[0] : sel;
        if (!path) return;
        let v;
        try {
          v = await validateImageBundleDir(path);
        } catch (e) {
          toast(`\u041E\u0448\u0438\u0431\u043A\u0430 \u043F\u0440\u043E\u0432\u0435\u0440\u043A\u0438 \u043D\u0430\u0431\u043E\u0440\u0430: ${e}`, "error");
          return;
        }
        await setImageBundleDir(path);
        notifyBundleChanged();
        if (v.valid) {
          toast("\u041D\u0430\u0431\u043E\u0440 \u0434\u043E\u0431\u0430\u0432\u043B\u0435\u043D: \u0432\u0441\u0435 \u0444\u0430\u0439\u043B\u044B \u043D\u0430 \u043C\u0435\u0441\u0442\u0435.");
        } else {
          toast(`\u041F\u0430\u043F\u043A\u0430 \u0432\u044B\u0431\u0440\u0430\u043D\u0430. \u041D\u0435 \u0445\u0432\u0430\u0442\u0430\u0435\u0442 \u0444\u0430\u0439\u043B\u043E\u0432: ${v.missing.length}. \u041D\u0430\u0436\u043C\u0438\u0442\u0435 \xAB\u0414\u043E\u043A\u0430\u0447\u0430\u0442\u044C \u0444\u0430\u0439\u043B\u044B\xBB.`);
        }
        await this.refresh();
      } catch (e) {
        toast(`\u041D\u0435 \u0443\u0434\u0430\u043B\u043E\u0441\u044C \u0434\u043E\u0431\u0430\u0432\u0438\u0442\u044C \u043D\u0430\u0431\u043E\u0440: ${e}`, "error");
      } finally {
        btn.disabled = false;
        this.busy = false;
      }
    }
  };
  if (!customElements.get("image-engine-panel")) {
    customElements.define("image-engine-panel", ImageEnginePanel);
  }
  if (!customElements.get("image-bundle-panel")) {
    customElements.define("image-bundle-panel", ImageBundlePanel);
  }
  if (!customElements.get("image-models-panel")) {
    customElements.define("image-models-panel", ImageModelsPanel);
  }

  // guest-js/index.ts
  var BUNDLE_CHANGED_EVENT = "image:bundle-changed";
  function notifyBundleChanged() {
    document.dispatchEvent(new CustomEvent(BUNDLE_CHANGED_EVENT));
  }
  function getImageEngineStatus() {
    return invoke("plugin:image-engine|get_image_engine_status");
  }
  function installImageEngine() {
    return invoke("plugin:image-engine|install_image_engine");
  }
  function setImageEngineVariant(variant) {
    return invoke("plugin:image-engine|set_image_engine_variant", { variant });
  }
  function checkImageEngineUpdate() {
    return invoke("plugin:image-engine|check_image_engine_update");
  }
  function installImageEngineUpdate() {
    return invoke("plugin:image-engine|install_image_engine_update");
  }
  function removeImageEngine() {
    return invoke("plugin:image-engine|remove_image_engine");
  }
  function setImageEngineDir(path) {
    return invoke("plugin:image-engine|set_image_engine_dir", { path });
  }
  function getImageModelsCatalog() {
    return invoke("plugin:image-engine|get_image_models_catalog");
  }
  function getImageBundleInfo() {
    return invoke("plugin:image-engine|get_image_bundle_info");
  }
  function downloadImageBundle(saveDir) {
    return invoke("plugin:image-engine|download_image_bundle", { saveDir });
  }
  function validateImageBundleDir(path) {
    return invoke("plugin:image-engine|validate_image_bundle_dir", { path });
  }
  function setImageBundleDir(path) {
    return invoke("plugin:image-engine|set_image_bundle_dir", { path });
  }
  function removeImageBundle() {
    return invoke("plugin:image-engine|remove_image_bundle");
  }
  function estimateImageMemory() {
    return invoke("plugin:image-engine|estimate_image_memory");
  }
  function generateImage(opts) {
    return invoke("plugin:image-engine|generate_image", {
      prompt: opts.prompt,
      width: opts.width ?? null,
      height: opts.height ?? null,
      steps: opts.steps ?? null,
      cfgScale: opts.cfgScale ?? null,
      seed: opts.seed ?? null,
      outPath: opts.outPath ?? null
    });
  }
  function editImage(opts) {
    return invoke("plugin:image-engine|edit_image", {
      prompt: opts.prompt,
      refPaths: opts.refPaths,
      width: opts.width ?? null,
      height: opts.height ?? null,
      steps: opts.steps ?? null,
      cfgScale: opts.cfgScale ?? null,
      seed: opts.seed ?? null,
      outPath: opts.outPath ?? null
    });
  }

  // guest-js/iife-entry.ts
  var g4 = window;
  if ("__TAURI__" in window) {
    const tauri = g4.__TAURI__;
    if (tauri && !tauri["image-engine"]) {
      Object.defineProperty(tauri, "image-engine", {
        configurable: true,
        value: {
          checkImageEngineUpdate,
          downloadImageBundle,
          editImage,
          estimateImageMemory,
          generateImage,
          getImageBundleInfo,
          getImageEngineStatus,
          getImageModelsCatalog,
          installImageEngine,
          installImageEngineUpdate,
          notifyBundleChanged,
          removeImageBundle,
          removeImageEngine,
          setImageBundleDir,
          setImageEngineDir,
          setImageEngineVariant,
          validateImageBundleDir
        }
      });
    }
  }
})();
