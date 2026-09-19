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
  async function save(opts) {
    const dialog = g2.__TAURI__?.dialog;
    if (!dialog?.save) throw new Error("window.__TAURI__.dialog.save \u043D\u0435\u0434\u043E\u0441\u0442\u0443\u043F\u0435\u043D");
    return dialog.save(opts);
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
  .bar-row { display: flex; justify-content: space-between; align-items: center; gap: 8px; flex-wrap: wrap; }
  .badge { font-size: 12px; vertical-align: middle; cursor: help; }
  .hint { color: var(--text-muted, #999); font-size: 12px; white-space: pre-line; }
  .overlay { position: fixed; inset: 0; background: rgba(0,0,0,.5); display: none; align-items: center;
             justify-content: center; z-index: 2000; }
  .overlay.open { display: flex; }
  .box { background: var(--bg-elevated, #1c1c1c); border: 1px solid var(--border, #333); border-radius: 12px;
         padding: 20px; max-width: 520px; width: 90%; }
  .box h3 { margin: 0 0 12px; font-size: 16px; }
  .box p { margin: 8px 0; color: var(--text-muted, #aaa); }
  .box .strong { color: var(--text, #eee); word-break: break-all; }
  .overlay-buttons { display: flex; gap: 10px; justify-content: flex-end; margin-top: 16px; }
  .empty { color: var(--text-muted, #888); font-size: 13px; }
`;
  function fileName(p) {
    const parts = p.split(/[/\\]/);
    return parts[parts.length - 1] || p;
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
    el.style.cssText = `position:fixed; right:16px; bottom:16px; z-index:5000; max-width:420px; padding:10px 14px;border-radius:8px; font:13px var(--font, system-ui, sans-serif); box-shadow:0 3px 14px rgba(0,0,0,.4);background:${kind === "success" ? "var(--primary, #4a90d9)" : "var(--danger, #b54242)"}; color:#fff;`;
    document.body.appendChild(el);
    setTimeout(() => el.remove(), 3500);
  }
  var LlamaEnginePanel = class extends HTMLElement {
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
        <div id="warning" class="hint" style="display:none; margin-top:10px;"></div>
        <div id="overlay" class="overlay">
          <div class="box">
            <h3>\u0423\u0434\u0430\u043B\u0438\u0442\u044C \u0434\u0432\u0438\u0436\u043E\u043A llamacpp?</h3>
            <p>\u0411\u0443\u0434\u0435\u0442 \u043E\u0441\u0432\u043E\u0431\u043E\u0436\u0434\u0435\u043D\u043E ~1 \u0413\u0411 \u043D\u0430 \u0434\u0438\u0441\u043A\u0435. GPU-\u0443\u0441\u043A\u043E\u0440\u0435\u043D\u0438\u0435 \u043E\u0442\u043A\u043B\u044E\u0447\u0438\u0442\u0441\u044F.</p>
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
      void listen("engine_progress", (e) => {
        const { downloaded, total } = e.payload;
        this.setProgress(downloaded, total);
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
      if (installed.length > 0) {
        const names = (st.available_variants || []).filter((x) => installed.includes(x.id)).map((x) => x.label).join(", ");
        lines.push(`\u0423\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D\u044B: ${names || installed.join(", ")}.`);
      } else {
        lines.push("\u041D\u0438 \u043E\u0434\u0438\u043D \u0431\u0435\u043A\u0435\u043D\u0434 \u0435\u0449\u0451 \u043D\u0435 \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D.");
      }
      lines.push("\u0421\u043C\u0435\u043D\u0430 \u0431\u0435\u043A\u0435\u043D\u0434\u0430 \u043F\u0440\u0438\u043C\u0435\u043D\u0438\u0442\u0441\u044F \u043F\u0440\u0438 \u0441\u043B\u0435\u0434\u0443\u044E\u0449\u0435\u043C \u0437\u0430\u043F\u0443\u0441\u043A\u0435 \u043C\u043E\u0434\u0435\u043B\u0438.");
      return lines.join("\n");
    }
    async refresh() {
      let st;
      try {
        st = await getEngineStatus();
      } catch (e) {
        this.setStatus(`\u041E\u0448\u0438\u0431\u043A\u0430: ${e}`);
        return;
      }
      this.setStatus(st.message);
      const gpu = this.root.getElementById("gpu");
      gpu.textContent = st.has_nvidia ? `${st.gpu_name} (\u0434\u0440\u0430\u0439\u0432\u0435\u0440 CUDA ${st.cuda_major}.${st.cuda_minor}${st.compute_cap ? `, compute ${st.compute_cap}` : ""}; \u0440\u0435\u043A\u043E\u043C\u0435\u043D\u0434\u0443\u0435\u043C\u044B\u0439 \u0432\u0430\u0440\u0438\u0430\u043D\u0442: ${st.required_variant || "?"})` : "\u041D\u0435 \u043E\u0431\u043D\u0430\u0440\u0443\u0436\u0435\u043D\u0430";
      this.root.getElementById("path").textContent = st.path || "\u2014";
      const sel = this.root.getElementById("variant");
      const prev = this.applied;
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
      this.root.getElementById("apply").style.display = sel.value === prev ? "none" : sel.value === this.applied ? "none" : "inline-block";
      this.applyButtonStates(st);
    }
    applyButtonStates(st) {
      const installed = st.installed;
      this.root.getElementById("install").style.display = installed ? "none" : "inline-block";
      this.root.getElementById("checkUpdate").style.display = installed ? "inline-block" : "none";
      this.root.getElementById("remove").style.display = installed ? "inline-block" : "none";
      this.root.getElementById("setDir").style.display = "inline-block";
      const installBtn = this.root.getElementById("install");
      const warning = this.root.getElementById("warning");
      if (!installed && st.requires_driver_update) {
        installBtn.disabled = true;
        warning.style.display = "block";
        warning.textContent = `\u26A0\uFE0F \u0412\u0430\u0448 \u0434\u0440\u0430\u0439\u0432\u0435\u0440 NVIDIA \u043F\u043E\u0434\u0434\u0435\u0440\u0436\u0438\u0432\u0430\u0435\u0442 \u0442\u043E\u043B\u044C\u043A\u043E CUDA ${st.cuda_major}.${st.cuda_minor}.
\u0414\u043B\u044F GPU-\u0443\u0441\u043A\u043E\u0440\u0435\u043D\u0438\u044F \u043E\u0431\u043D\u043E\u0432\u0438\u0442\u0435 \u0434\u0440\u0430\u0439\u0432\u0435\u0440 (\u043D\u0443\u0436\u043D\u0430 \u0432\u0435\u0440\u0441\u0438\u044F \u2265 527.41, CUDA 12+).
\u041F\u043E\u043A\u0430 \u043F\u0440\u0438\u043B\u043E\u0436\u0435\u043D\u0438\u0435 \u0440\u0430\u0431\u043E\u0442\u0430\u0435\u0442 \u0432 CPU-\u0440\u0435\u0436\u0438\u043C\u0435.`;
      } else if (!installed) {
        installBtn.disabled = false;
        warning.style.display = "none";
      } else {
        warning.style.display = "none";
      }
    }
    async onVariantChange() {
      const sel = this.root.getElementById("variant");
      const value = sel.value;
      const st = { available_variants: [], installed_variants: [], resolved_variant: "", message: "", path: "", has_nvidia: false, requires_driver_update: false, cuda_major: 0, cuda_minor: 0, gpu_name: "", compute_cap: "", required_variant: "", selected_variant: "", installed: false };
      this.root.getElementById("variantHint").textContent = this.hintText(st, value);
      if (value !== this.applied) this.root.getElementById("apply").style.display = "inline-block";
      else this.root.getElementById("apply").style.display = "none";
      try {
        const s = await getEngineStatus();
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
          installedVariants = (await getEngineStatus()).installed_variants || [];
        } catch {
        }
        if (!installedVariants.includes(variant)) {
          this.showProgress(true);
          this.setProgress(0, 0);
        }
        await setEngineVariant(variant);
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
        await installLlamaCpp();
        await this.refresh();
        toast("\u0414\u0432\u0438\u0436\u043E\u043A llamacpp \u0443\u0441\u0442\u0430\u043D\u043E\u0432\u043B\u0435\u043D!");
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
      this.root.getElementById("installUpdate").style.display = "none";
      try {
        const newTag = await checkEngineUpdate();
        if (newTag) {
          this.setStatus(`\u0414\u043E\u0441\u0442\u0443\u043F\u043D\u043E \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u0435 \u0434\u0432\u0438\u0436\u043A\u0430: ${newTag}`);
          this.root.getElementById("installUpdate").style.display = "inline-block";
        } else {
          this.setStatus("\u0414\u0432\u0438\u0436\u043E\u043A llamacpp \u0430\u043A\u0442\u0443\u0430\u043B\u0435\u043D");
        }
      } catch (e) {
        this.setStatus("");
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430 \u043F\u0440\u043E\u0432\u0435\u0440\u043A\u0438 \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u044F \u0434\u0432\u0438\u0436\u043A\u0430: ${e}`, "error");
      } finally {
        btn.disabled = false;
      }
    }
    async onInstallUpdate() {
      const btn = this.root.getElementById("installUpdate");
      btn.disabled = true;
      this.showProgress(true);
      this.root.getElementById("progressStatus").textContent = "\u041E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u0435\u2026";
      try {
        await installEngineUpdate();
        this.root.getElementById("installUpdate").style.display = "none";
        await this.refresh();
        toast("\u0414\u0432\u0438\u0436\u043E\u043A llamacpp \u043E\u0431\u043D\u043E\u0432\u043B\u0451\u043D.", "success");
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
        await removeEngine();
        await this.refresh();
        toast("\u0414\u0432\u0438\u0436\u043E\u043A llamacpp \u0443\u0434\u0430\u043B\u0451\u043D.");
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
        await setEngineDir(path);
        await this.refresh();
        toast("\u041F\u0443\u0442\u044C \u0434\u0432\u0438\u0436\u043A\u0430 \u0438\u0437\u043C\u0435\u043D\u0451\u043D.");
      } catch (e) {
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430 \u0438\u0437\u043C\u0435\u043D\u0435\u043D\u0438\u044F \u043F\u0443\u0442\u0438: ${e}`, "error");
      }
    }
  };
  var LlamaDownloadPanel = class extends HTMLElement {
    constructor() {
      super(...arguments);
      this.catalog = [];
      this.busy = false;
    }
    connectedCallback() {
      if (this.root) return;
      this.root = this.attachShadow({ mode: "open" });
      this.root.innerHTML = `
      <style>${STYLE}</style>
      <div>
        <div class="row">
          <select id="catalog" style="flex:1;"><option value="">\u0417\u0430\u0433\u0440\u0443\u0437\u043A\u0430 \u043A\u0430\u0442\u0430\u043B\u043E\u0433\u0430\u2026</option></select>
          <button id="download" class="primary">\u0421\u043A\u0430\u0447\u0430\u0442\u044C</button>
        </div>
        <div id="progress" class="progress-container">
          <div class="progress-status" id="progressStatus">0 B / 0 B</div>
          <div class="progress-track"><div class="progress-bar" id="progressBar"></div></div>
        </div>
        <button id="autoDownload" class="primary" style="display:none; margin-top:10px;">\u26A1 \u0421\u043A\u0430\u0447\u0430\u0442\u044C \u0430\u0432\u0442\u043E\u043C\u0430\u0442\u0438\u0447\u0435\u0441\u043A\u0438</button>
        <div id="overlay" class="overlay">
          <div class="box">
            <h3>\u0410\u0432\u0442\u043E\u043C\u0430\u0442\u0438\u0447\u0435\u0441\u043A\u0430\u044F \u0437\u0430\u0433\u0440\u0443\u0437\u043A\u0430 \u043C\u043E\u0434\u0435\u043B\u0438</h3>
            <p>\u0411\u0443\u0434\u0435\u0442 \u0441\u043A\u0430\u0447\u0430\u043D\u0430 \u043C\u043E\u0434\u0435\u043B\u044C: <span class="strong" id="modalName"></span></p>
            <p>\u041F\u0443\u0442\u044C: <span class="strong" id="modalPath"></span></p>
            <p>\u0421\u0432\u043E\u0431\u043E\u0434\u043D\u043E \u043D\u0430 \u0434\u0438\u0441\u043A\u0435: <span class="strong" id="modalSpace"></span></p>
            <p style="margin-top:12px;">\u0421\u043E\u0433\u043B\u0430\u0441\u043D\u044B?</p>
            <div class="overlay-buttons">
              <button id="modalCancel" class="secondary">\u041E\u0442\u043C\u0435\u043D\u0430</button>
              <button id="modalOk" class="primary">\u041E\u041A</button>
            </div>
          </div>
        </div>
      </div>`;
      this.root.getElementById("download").addEventListener("click", () => this.onDownload());
      this.root.getElementById("autoDownload").addEventListener("click", () => this.onAutoDownload());
      this.root.getElementById("modalCancel").addEventListener("click", () => {
        this.root.getElementById("overlay").classList.remove("open");
      });
      this.root.getElementById("modalOk").addEventListener("click", () => {
        this.root.getElementById("overlay").classList.remove("open");
        void this.doAutoDownload();
      });
      void this.refresh();
      void listen("download_progress", (e) => {
        const { downloaded, total, speed_bps } = e.payload;
        const pct = total > 0 ? downloaded / total * 100 : 0;
        const bar = this.root.getElementById("progressBar");
        bar.style.width = `${pct}%`;
        const st = this.root.getElementById("progressStatus");
        const speed = formatBytes(speed_bps ?? 0);
        st.textContent = `${formatBytes(downloaded)} / ${total > 0 ? formatBytes(total) : "?"}${total > 0 ? ` (${pct.toFixed(0)}%)` : ""} \xB7 ${speed}`;
      }).then((u) => {
        this.unlisten = u;
      }).catch(() => {
      });
    }
    disconnectedCallback() {
      this.unlisten?.();
    }
    showProgress(on) {
      this.root.getElementById("progress").style.display = on ? "block" : "none";
    }
    async refresh() {
      try {
        this.catalog = await getModelsCatalog();
        const sel = this.root.getElementById("catalog");
        sel.innerHTML = '<option value="">-- \u0412\u044B\u0431\u0435\u0440\u0438\u0442\u0435 \u043C\u043E\u0434\u0435\u043B\u044C --</option>';
        for (const m of this.catalog) {
          const o = document.createElement("option");
          o.value = m.name;
          const badges = this.badges(m).join(" ");
          o.textContent = (m.size_gb ? `${m.name} (${m.size_gb} GB)` : m.name) + (badges ? `  ${badges}` : "");
          sel.appendChild(o);
        }
        let cfg;
        try {
          cfg = await getEngineConfig();
        } catch {
          cfg = { models: [], model_params: {}, mmproj_files: {}, model_meta: {} };
        }
        this.root.getElementById("autoDownload").style.display = (cfg.models || []).length === 0 ? "inline-block" : "none";
      } catch (e) {
        this.root.getElementById("catalog").innerHTML = '<option value="">\u041E\u0448\u0438\u0431\u043A\u0430</option>';
      }
    }
    badges(m) {
      const out = [];
      if (m.uncen) out.push("\u{1F608}");
      if (m.vision) out.push("\u{1F441}\uFE0F");
      if (m.audio) out.push("\u{1F3B5}");
      return out;
    }
    async onDownload() {
      if (this.busy) return;
      const sel = this.root.getElementById("catalog");
      const name = sel.value;
      if (!name) return;
      const model = this.catalog.find((m) => m.name === name);
      if (!model) return;
      this.busy = true;
      const btn = this.root.getElementById("download");
      btn.disabled = true;
      try {
        const defaultName = (model.download_url.split("/").pop() || `${model.name}.gguf`).split("?")[0];
        const savePath = await save({ defaultPath: defaultName, filters: [{ name: "GGUF", extensions: ["gguf"] }] });
        if (!savePath) return;
        this.showProgress(true);
        this.root.getElementById("progressStatus").textContent = "\u041F\u043E\u0434\u043A\u043B\u044E\u0447\u0435\u043D\u0438\u0435\u2026";
        await downloadModel(model.download_url, savePath);
        if (model.mmproj_url) {
          const sep = savePath.includes("\\") ? "\\" : "/";
          const dir = savePath.substring(0, savePath.lastIndexOf(sep));
          const mpName = (model.mmproj_url.split("/").pop() || "mmproj.gguf").split("?")[0];
          await downloadModel(model.mmproj_url, `${dir}${sep}${mpName}`);
        }
        const res = await addModel(savePath, { uncen: !!model.uncen, vision: !!model.vision, audio: !!model.audio });
        if (res?.warning) toast(res.warning, "error");
        notifyModelsChanged();
        void this.refresh();
        toast(`\u041C\u043E\u0434\u0435\u043B\u044C ${model.name} \u0441\u043A\u0430\u0447\u0430\u043D\u0430!`);
      } catch (e) {
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430: ${e}`, "error");
      } finally {
        btn.disabled = false;
        this.busy = false;
        this.showProgress(false);
      }
    }
    async onAutoDownload() {
      try {
        const info = await getAutoDownloadInfo();
        this.root.getElementById("modalName").textContent = info.size_gb ? `${info.model_name} (${info.size_gb} GB)` : info.model_name;
        this.root.getElementById("modalPath").textContent = info.save_path;
        this.root.getElementById("modalSpace").textContent = `${info.free_space_gb} GB`;
        this.root.getElementById("overlay").classList.add("open");
      } catch (e) {
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430: ${e}`, "error");
      }
    }
    async doAutoDownload() {
      if (this.busy) return;
      this.busy = true;
      const btn = this.root.getElementById("autoDownload");
      btn.disabled = true;
      try {
        const info = await getAutoDownloadInfo();
        this.showProgress(true);
        this.root.getElementById("progressStatus").textContent = "\u041F\u043E\u0434\u043A\u043B\u044E\u0447\u0435\u043D\u0438\u0435\u2026";
        await autoDownloadDefaultModel(info.save_path);
        notifyModelsChanged();
        void this.refresh();
        toast(`\u041C\u043E\u0434\u0435\u043B\u044C ${info.model_name} \u0441\u043A\u0430\u0447\u0430\u043D\u0430!`);
      } catch (e) {
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430: ${e}`, "error");
      } finally {
        btn.disabled = false;
        this.busy = false;
        this.showProgress(false);
      }
    }
  };
  var LlamaModelsPanel = class extends HTMLElement {
    constructor() {
      super(...arguments);
      this.busy = false;
      this.pendingAction = null;
      this.pendingPath = "";
      this.capMap = {};
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
        <div id="list" style="display:flex; flex-direction:column; gap:8px;"></div>
        <button id="add" class="secondary" style="margin-top:10px;">+ \u0414\u043E\u0431\u0430\u0432\u0438\u0442\u044C \u043C\u043E\u0434\u0435\u043B\u044C</button>
        <div id="overlay" class="overlay">
          <div class="box">
            <h3 id="overlayTitle">\u0423\u0434\u0430\u043B\u0438\u0442\u044C</h3>
            <p id="overlayMsg"></p>
            <div class="overlay-buttons">
              <button id="modalCancel" class="secondary">\u041E\u0442\u043C\u0435\u043D\u0430</button>
              <button id="modalOk" class="danger">\u0423\u0434\u0430\u043B\u0438\u0442\u044C</button>
            </div>
          </div>
        </div>
      </div>`;
      this.root.getElementById("add").addEventListener("click", () => this.onAdd());
      this.root.getElementById("modalCancel").addEventListener("click", () => {
        this.root.getElementById("overlay").classList.remove("open");
        this.pendingAction = null;
      });
      this.root.getElementById("modalOk").addEventListener("click", () => {
        this.root.getElementById("overlay").classList.remove("open");
        const action = this.pendingAction;
        const path = this.pendingPath;
        this.pendingAction = null;
        this.pendingPath = "";
        if (action) void this.doAction(action, path);
      });
      document.addEventListener("llama:models-changed", this.onChangedBound);
      void this.refresh();
    }
    disconnectedCallback() {
      document.removeEventListener("llama:models-changed", this.onChangedBound);
    }
    async refresh() {
      let cfg;
      try {
        cfg = await getEngineConfig();
      } catch {
        return;
      }
      try {
        this.capMap = await getAllCapabilities();
      } catch {
        this.capMap = {};
      }
      const list = this.root.getElementById("list");
      list.innerHTML = "";
      const models = cfg.models || [];
      if (models.length === 0) {
        const empty = document.createElement("div");
        empty.className = "empty";
        empty.textContent = "\u041C\u043E\u0434\u0435\u043B\u0438 \u043D\u0435 \u0434\u043E\u0431\u0430\u0432\u043B\u0435\u043D\u044B.";
        list.appendChild(empty);
        return;
      }
      for (const m of models) {
        const row = document.createElement("div");
        row.style.cssText = "display:flex; align-items:center; justify-content:space-between; gap:10px; padding:8px 10px; border:1px solid var(--border,#333); border-radius:6px; background:var(--bg-elevated,#1c1c1c);";
        const info = document.createElement("div");
        info.style.cssText = "display:flex; flex-direction:column; gap:2px; min-width:0;";
        const name = document.createElement("div");
        name.style.cssText = "font-weight:600; color:var(--text,#eee); word-break:break-all;";
        name.textContent = (cfg.last_model === m ? "\u25CF " : "") + fileName(m);
        const meta = this.capMap[m];
        if (meta) {
          if (meta.uncen) {
            const s = span("\u{1F608}", "\u0411\u0435\u0437 \u0446\u0435\u043D\u0437\u0443\u0440\u044B (uncensored)");
            name.appendChild(s);
          }
          if (meta.vision) {
            const s = span("\u{1F441}\uFE0F", "\u0412\u0438\u0434\u0438\u0442 \u0438\u0437\u043E\u0431\u0440\u0430\u0436\u0435\u043D\u0438\u044F (vision)");
            name.appendChild(s);
          }
          if (meta.audio) {
            const s = span("\u{1F3B5}", "\u041F\u043E\u043D\u0438\u043C\u0430\u0435\u0442 \u0430\u0443\u0434\u0438\u043E (audio)");
            name.appendChild(s);
          }
        }
        const path = document.createElement("div");
        path.textContent = m;
        path.style.cssText = "font-size:11px; color:var(--text-muted,#888); word-break:break-all;";
        info.appendChild(name);
        info.appendChild(path);
        const buttons = document.createElement("div");
        buttons.style.cssText = "display:flex; gap:8px; flex-shrink:0; flex-wrap:wrap;";
        const rmv = document.createElement("button");
        rmv.className = "secondary";
        rmv.textContent = "\u0423\u0434\u0430\u043B\u0438\u0442\u044C \u0438\u0437 \u0441\u043F\u0438\u0441\u043A\u0430";
        rmv.addEventListener("click", () => this.confirm("remove", m));
        const del = document.createElement("button");
        del.className = "danger";
        del.textContent = "\u0423\u0434\u0430\u043B\u0438\u0442\u044C \u0444\u0430\u0439\u043B";
        del.addEventListener("click", () => this.confirm("delete", m));
        buttons.appendChild(rmv);
        buttons.appendChild(del);
        row.appendChild(info);
        row.appendChild(buttons);
        list.appendChild(row);
      }
    }
    confirm(action, path) {
      this.pendingAction = action;
      this.pendingPath = path;
      const title = this.root.getElementById("overlayTitle");
      const msg = this.root.getElementById("overlayMsg");
      const ok = this.root.getElementById("modalOk");
      if (action === "remove") {
        title.textContent = `\u0423\u0434\u0430\u043B\u0438\u0442\u044C \xAB${fileName(path)}\xBB \u0438\u0437 \u0441\u043F\u0438\u0441\u043A\u0430?`;
        msg.textContent = "\u0424\u0430\u0439\u043B \u043D\u0430 \u0434\u0438\u0441\u043A\u0435 \u043D\u0435 \u0431\u0443\u0434\u0435\u0442 \u0443\u0434\u0430\u043B\u0451\u043D.";
        ok.className = "secondary";
        ok.textContent = "\u0423\u0434\u0430\u043B\u0438\u0442\u044C \u0438\u0437 \u0441\u043F\u0438\u0441\u043A\u0430";
      } else {
        title.textContent = `\u0423\u0434\u0430\u043B\u0438\u0442\u044C \u0444\u0430\u0439\u043B \xAB${fileName(path)}\xBB?`;
        msg.textContent = "\u0424\u0430\u0439\u043B \u0441 \u0434\u0438\u0441\u043A\u0430 \u0431\u0443\u0434\u0435\u0442 \u0443\u0434\u0430\u043B\u0451\u043D \u0411\u0415\u0417\u0412\u041E\u0417\u0412\u0420\u0410\u0422\u041D\u041E. \u0417\u0430\u043F\u0438\u0441\u044C \u0442\u043E\u0436\u0435 \u0438\u0441\u0447\u0435\u0437\u043D\u0435\u0442 \u0438\u0437 \u0441\u043F\u0438\u0441\u043A\u0430.";
        ok.className = "danger";
        ok.textContent = "\u0423\u0434\u0430\u043B\u0438\u0442\u044C \u0444\u0430\u0439\u043B";
      }
      this.root.getElementById("overlay").classList.add("open");
    }
    async doAction(action, path) {
      if (this.busy) return;
      this.busy = true;
      try {
        if (action === "remove") {
          await removeModel(path);
          toast("\u041C\u043E\u0434\u0435\u043B\u044C \u0443\u0434\u0430\u043B\u0435\u043D\u0430 \u0438\u0437 \u0441\u043F\u0438\u0441\u043A\u0430.");
        } else {
          await deleteModelFile(path);
          toast("\u0424\u0430\u0439\u043B \u043C\u043E\u0434\u0435\u043B\u0438 \u0443\u0434\u0430\u043B\u0451\u043D.");
        }
        notifyModelsChanged();
        await this.refresh();
      } catch (e) {
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430: ${e}`, "error");
      } finally {
        this.busy = false;
      }
    }
    async onAdd() {
      if (this.busy) return;
      this.busy = true;
      const btn = this.root.getElementById("add");
      btn.disabled = true;
      try {
        const sel = await open({ filters: [{ name: "Model", extensions: ["gguf"] }] });
        if (!sel) return;
        const path = Array.isArray(sel) ? sel[0] : sel;
        if (!path) return;
        const res = await addModel(path, null);
        if (res?.warning) toast(res.warning, "error");
        notifyModelsChanged();
        await this.refresh();
      } catch (e) {
        toast(`\u041D\u0435 \u0443\u0434\u0430\u043B\u043E\u0441\u044C \u0434\u043E\u0431\u0430\u0432\u0438\u0442\u044C \u043C\u043E\u0434\u0435\u043B\u044C: ${e}`, "error");
      } finally {
        btn.disabled = false;
        this.busy = false;
      }
    }
  };
  function span(text, title) {
    const s = document.createElement("span");
    s.textContent = text;
    s.title = title;
    s.className = "badge";
    s.style.marginLeft = "5px";
    return s;
  }
  if (!customElements.get("llama-engine-panel")) {
    customElements.define("llama-engine-panel", LlamaEnginePanel);
  }
  if (!customElements.get("llama-download-panel")) {
    customElements.define("llama-download-panel", LlamaDownloadPanel);
  }
  if (!customElements.get("llama-models-panel")) {
    customElements.define("llama-models-panel", LlamaModelsPanel);
  }

  // guest-js/index.ts
  var MODELS_CHANGED_EVENT = "llama:models-changed";
  function notifyModelsChanged() {
    document.dispatchEvent(new CustomEvent(MODELS_CHANGED_EVENT));
  }
  function getEngineStatus() {
    return invoke("plugin:llama-engine|get_engine_status");
  }
  function installLlamaCpp() {
    return invoke("plugin:llama-engine|install_llamacpp");
  }
  function setEngineVariant(variant) {
    return invoke("plugin:llama-engine|set_engine_variant", { variant });
  }
  function checkEngineUpdate() {
    return invoke("plugin:llama-engine|check_engine_update");
  }
  function installEngineUpdate() {
    return invoke("plugin:llama-engine|install_engine_update");
  }
  function removeEngine() {
    return invoke("plugin:llama-engine|remove_engine");
  }
  function setEngineDir(path) {
    return invoke("plugin:llama-engine|set_engine_dir", { path });
  }
  function getModelsCatalog() {
    return invoke("plugin:llama-engine|get_models_catalog");
  }
  function getEngineConfig() {
    return invoke("plugin:llama-engine|get_engine_config");
  }
  function getAutoDownloadInfo() {
    return invoke("plugin:llama-engine|get_auto_download_info");
  }
  function autoDownloadDefaultModel(savePath) {
    return invoke("plugin:llama-engine|auto_download_default_model", { savePath });
  }
  function addModel(path, flags) {
    return invoke("plugin:llama-engine|add_model", { path, flags });
  }
  function removeModel(path) {
    return invoke("plugin:llama-engine|remove_model", { path });
  }
  function deleteModelFile(path) {
    return invoke("plugin:llama-engine|delete_model_file", { path });
  }
  function downloadModel(url, savePath) {
    return invoke("plugin:llama-engine|download_model", { url, savePath });
  }
  function getMmprojPath(modelPath) {
    return invoke("plugin:llama-engine|get_mmproj_path", { modelPath });
  }
  function ensureMmproj(modelPath) {
    return invoke("plugin:llama-engine|ensure_mmproj", { modelPath });
  }
  function getModelCapabilities(modelPath) {
    return invoke("plugin:llama-engine|get_model_capabilities", { modelPath });
  }
  function getAllCapabilities() {
    return invoke("plugin:llama-engine|get_all_capabilities");
  }
  function estimatePromptMemory(modelPath, contextSize, kvQuantKeys, kvQuantValues, promptTokens, maxGen) {
    return invoke("plugin:llama-engine|estimate_prompt_memory", {
      modelPath,
      contextSize,
      kvQuantKeys,
      kvQuantValues,
      promptTokens,
      maxGen
    });
  }
  function getModelParams(modelPath) {
    return invoke("plugin:llama-engine|get_model_params", { modelPath });
  }
  function setModelParams(modelPath, params) {
    return invoke("plugin:llama-engine|set_model_params", { modelPath, params });
  }
  function resetModelParams(modelPath) {
    return invoke("plugin:llama-engine|reset_model_params", { modelPath });
  }

  // guest-js/iife-entry.ts
  var g4 = window;
  if ("__TAURI__" in window) {
    const tauri = g4.__TAURI__;
    if (tauri && !tauri["llama-engine"]) {
      Object.defineProperty(tauri, "llama-engine", {
        configurable: true,
        value: {
          addModel,
          autoDownloadDefaultModel,
          checkEngineUpdate,
          deleteModelFile,
          downloadModel,
          ensureMmproj,
          estimatePromptMemory,
          getAllCapabilities,
          getAutoDownloadInfo,
          getEngineConfig,
          getEngineStatus,
          getMmprojPath,
          getModelCapabilities,
          getModelParams,
          getModelsCatalog,
          installEngineUpdate,
          installLlamaCpp,
          notifyModelsChanged,
          removeEngine,
          removeModel,
          resetModelParams,
          setEngineDir,
          setEngineVariant,
          setModelParams
        }
      });
    }
  }
})();
