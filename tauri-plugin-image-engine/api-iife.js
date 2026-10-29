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

  // guest-js/components/ui-shared.ts
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
  button.tab-btn[aria-pressed="true"] { font-weight: 700; }
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
  .update-badge { display: inline-block; width: 8px; height: 8px; border-radius: 50%;
                  background: #4caf50; margin-left: 6px; box-shadow: 0 0 6px #4caf50; }
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
  function progressText(downloaded, total) {
    const pct = total > 0 ? downloaded / total * 100 : 0;
    return `${formatBytes(downloaded)} / ${total > 0 ? formatBytes(total) : "?"}${total > 0 ? ` (${pct.toFixed(0)}%)` : ""}`;
  }

  // guest-js/updates.ts
  var state = { hasUpdate: false };
  var subs = /* @__PURE__ */ new Set();
  function getUpdateState() {
    return state;
  }
  function onUpdateState(fn) {
    subs.add(fn);
    fn(state);
    return () => {
      subs.delete(fn);
    };
  }
  function setUpdateState(next) {
    state = next;
    subs.forEach((fn) => fn(state));
  }
  async function checkUpdate() {
    const tag = await checkImageEngineUpdate();
    const next = tag ? { hasUpdate: true, tag } : { hasUpdate: false };
    setUpdateState(next);
    return next;
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

  // guest-js/components/engine-panel.ts
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
        <div class="row"><label>\u0421\u0442\u0430\u0442\u0443\u0441:</label><span id="status" class="hint">\u041F\u0440\u043E\u0432\u0435\u0440\u043A\u0430\u2026</span><span id="updateBadge" class="update-badge" style="display:none;"></span></div>
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
      this.unsubUpdate = onUpdateState((s) => this.renderUpdateState(s));
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
      this.unsubUpdate?.();
      this.unlisten?.();
    }
    setStatus(s) {
      const el = this.root.getElementById("status");
      if (el) el.textContent = s;
    }
    setProgress(downloaded, total) {
      const bar = this.root.getElementById("progressBar");
      bar.style.width = `${total > 0 ? downloaded / total * 100 : 0}%`;
      const st = this.root.getElementById("progressStatus");
      st.textContent = progressText(downloaded, total);
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
      this.renderUpdateState(getUpdateState());
    }
    /**
     * Единственное место, где видны бейдж и кнопка «Обновить»: оба выводятся
     * из одного состояния. Раньше в ветке `else` прятался только бейдж — кнопка
     * оставалась на экране после установки обновления.
     */
    renderUpdateState(s) {
      const badge = this.root.getElementById("updateBadge");
      const updateBtn = this.root.getElementById("installUpdate");
      const display = s.hasUpdate ? "inline-block" : "none";
      if (badge) badge.style.display = display;
      if (updateBtn) updateBtn.style.display = display;
      if (s.hasUpdate) {
        this.setStatus(`\u0414\u043E\u0441\u0442\u0443\u043F\u043D\u043E \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u0435 \u0434\u0432\u0438\u0436\u043A\u0430: ${s.tag ?? ""}`);
      }
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
      this.renderUpdateState({ hasUpdate: false });
      try {
        const next = await checkUpdate();
        this.setStatus(
          next.hasUpdate ? `\u0414\u043E\u0441\u0442\u0443\u043F\u043D\u043E \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0438\u0435 \u0434\u0432\u0438\u0436\u043A\u0430: ${next.tag ?? ""}` : "\u0414\u0432\u0438\u0436\u043E\u043A \u0438\u0437\u043E\u0431\u0440\u0430\u0436\u0435\u043D\u0438\u0439 \u0430\u043A\u0442\u0443\u0430\u043B\u0435\u043D"
        );
      } catch (e) {
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
        setUpdateState({ hasUpdate: false });
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
  if (!customElements.get("image-engine-panel")) {
    customElements.define("image-engine-panel", ImageEnginePanel);
  }

  // guest-js/components/services.ts
  var imageService = {
    kind: "image",
    kindLabel: "\u0411\u0430\u043D\u0434\u043B",
    progressKind: "image-model",
    info: (name) => getImageBundleInfo(name).then((i) => i),
    entries: () => getImageModelsCatalog().then(
      (list) => list.map((e) => ({ name: e.name, label: e.label, is_default: e.is_default }))
    ),
    activate: (name) => setSelectedImageBundle(name).then((i) => i),
    validateDir: (path, name) => validateImageBundleDir(path, name),
    setCustomDir: (path) => setImageBundleDir(path).then((i) => i),
    clearCustomDir: () => removeImageBundleFromList().then((i) => i),
    download: (saveDir, name) => downloadImageBundle(saveDir, name).then((i) => i),
    removeFiles: (name) => removeImageBundle(name ?? null).then((i) => i),
    memoryProbe: async (name) => {
      const cur = await getImageBundleInfo(name);
      const info = await imageSizeInfo(cur.bundle_name);
      const mem = await estimateImageMemory(cur.bundle_name);
      const gb = (mb) => `${(mb / 1024).toFixed(1)} \u0413\u0411`;
      return `GPU: ${info.vram_fast_gb ?? "?"} \u0413\u0411 (\u0431\u044B\u0441\u0442\u0440\u043E) / ${info.vram_min_gb ?? "?"} \u0413\u0411 (\u043C\u0438\u043D.) \xB7 \u0441\u0435\u0439\u0447\u0430\u0441 \u0441\u0432\u043E\u0431\u043E\u0434\u043D\u043E ${gb(mem.estimate.vram_free_mb)} VRAM / ${gb(mem.estimate.ram_free_mb)} RAM \u2014 ${mem.message}`;
    }
  };
  async function imageSizeInfo(bundleName) {
    return getImageBundleInfo(bundleName);
  }
  var videoService = {
    kind: "video",
    kindLabel: "\u0412\u0438\u0434\u0435\u043E-\u0431\u0430\u043D\u0434\u043B",
    progressKind: "video-model",
    info: (name) => getVideoBundleInfo(name).then((i) => i),
    entries: () => getVideoBundles().then(
      (list) => list.map((e) => ({ name: e.name, label: e.label, is_default: e.is_default }))
    ),
    activate: (name) => setSelectedVideoBundle(name).then((i) => i),
    validateDir: (path, name) => validateVideoBundleDir(path, name),
    setCustomDir: (path) => setVideoBundleDir(path).then((i) => i),
    clearCustomDir: () => setVideoBundleDir(null).then((i) => i),
    download: (saveDir, name) => downloadVideoBundle(saveDir, name).then((i) => i),
    removeFiles: (name) => removeVideoBundleFiles(name ?? null).then((i) => i)
  };
  function serviceFor(kind) {
    if (kind === "video") return videoService;
    if (kind === "image") return imageService;
    return void 0;
  }

  // guest-js/components/bundle-tab.ts
  var BundleTabElement = class _BundleTabElement extends HTMLElement {
    constructor() {
      super(...arguments);
      this.busy = false;
      this.appliedBundle = "";
      /** Имя бандла, выбранного в селекте — источник правды всех операций вкладки
       * (скачать/докачать/удалить/активировать смотрят на НЕГО, а не на активный;
       * PRP 29.10.26 D1). Инициализируется активным при первом открытии. */
      this.selected = "";
      /** Статусы бандлов из последнего refresh (fully_downloaded и файлы) —
       * рендер панели и гейт кнопок читают ТОЛЬКО отсюда (SSOT). */
      this.statuses = /* @__PURE__ */ new Map();
    }
    connectedCallback() {
      if (this.root) return;
      const kind = this.getAttribute("service") ?? "image";
      const svc = serviceFor(kind);
      if (!svc) {
        this.textContent = `\u041D\u0435\u0438\u0437\u0432\u0435\u0441\u0442\u043D\u044B\u0439 \u0441\u0435\u0440\u0432\u0438\u0441 \u0432\u043A\u043B\u0430\u0434\u043A\u0438: \xAB${kind}\xBB (\u043E\u0436\u0438\u0434\u0430\u0435\u0442\u0441\u044F image|video)`;
        return;
      }
      this.service = svc;
      this.root = this.attachShadow({ mode: "open" });
      this.root.innerHTML = `
      <style>${STYLE}</style>
      <div>
        <div class="row">
          <label>${esc(svc.kindLabel)}:</label>
          <select id="bundle" style="flex:1; min-width:220px;"><option value="">\u2026</option></select>
          <button id="activate" class="primary" style="display:none;">\u0410\u043A\u0442\u0438\u0432\u0438\u0440\u043E\u0432\u0430\u0442\u044C</button>
        </div>
        <div class="row"><label>\u0421\u043E\u0441\u0442\u043E\u044F\u043D\u0438\u0435:</label><span id="state" class="hint" style="flex:1;"></span></div>
        <div class="row"><label>\u041D\u0430\u0431\u043E\u0440:</label><span id="label" class="hint">\u2026</span></div>
        <div id="note" class="hint"></div>
        <ul class="files" id="files"></ul>
        <div class="row" style="margin-top:8px;"><label>\u0414\u0438\u0441\u043A:</label><span id="disk" class="hint"></span></div>
        <div class="row"><label>\u041F\u0430\u043C\u044F\u0442\u044C:</label><span id="mem" class="hint"></span></div>
        <div class="row">
          <label>\u041F\u0430\u043F\u043A\u0430:</label>
          <span id="dir" class="hint" style="word-break:break-all; flex:1;"></span>
          <button id="changeDir" class="secondary">\u0418\u0437\u043C\u0435\u043D\u0438\u0442\u044C \u043F\u0443\u0442\u044C\u2026</button>
          <button id="resetDir" class="secondary" style="display:none;">\u0421\u0431\u0440\u043E\u0441\u0438\u0442\u044C \u043F\u0443\u0442\u044C</button>
        </div>
        <div class="row" style="margin-top:10px;">
          <button id="download" class="primary">\u0421\u043A\u0430\u0447\u0430\u0442\u044C \u043D\u0430\u0431\u043E\u0440</button>
          <button id="resume" class="primary" style="display:none;">\u0414\u043E\u043A\u0430\u0447\u0430\u0442\u044C \u0444\u0430\u0439\u043B\u044B</button>
          <button id="deleteFiles" class="danger" style="display:none;">\u0423\u0434\u0430\u043B\u0438\u0442\u044C \u0444\u0430\u0439\u043B\u044B</button>
        </div>
        <div id="progress" class="progress-container">
          <div class="progress-status" id="progressStatus">0 MB / 0 MB</div>
          <div class="progress-track"><div class="progress-bar" id="progressBar"></div></div>
        </div>
      </div>
      <div id="overlay" class="overlay">
        <div class="box">
          <h3 id="overlayTitle">\u0423\u0434\u0430\u043B\u0438\u0442\u044C \u0444\u0430\u0439\u043B\u044B \u043D\u0430\u0431\u043E\u0440\u0430?</h3>
          <p id="overlayMsg">\u0424\u0430\u0439\u043B\u044B \u0441 \u0434\u0438\u0441\u043A\u0430 \u0431\u0443\u0434\u0443\u0442 \u0443\u0434\u0430\u043B\u0435\u043D\u044B \u0411\u0415\u0417\u0412\u041E\u0417\u0412\u0420\u0410\u0422\u041D\u041E. \u0417\u0430\u043F\u0438\u0441\u044C \u0442\u043E\u0436\u0435 \u0438\u0441\u0447\u0435\u0437\u043D\u0435\u0442 \u0438\u0437 \u0441\u043F\u0438\u0441\u043A\u0430.</p>
          <div class="overlay-buttons">
            <button id="modalCancel" class="secondary">\u041E\u0442\u043C\u0435\u043D\u0430</button>
            <button id="modalOk" class="danger">\u0423\u0434\u0430\u043B\u0438\u0442\u044C \u0444\u0430\u0439\u043B\u044B</button>
          </div>
        </div>
      </div>`;
      this.root.getElementById("bundle").addEventListener("change", () => this.onBundleChange());
      this.root.getElementById("activate").addEventListener("click", () => this.onActivate());
      this.root.getElementById("download").addEventListener("click", () => this.onDownload());
      this.root.getElementById("changeDir").addEventListener("click", () => this.onChangeDir());
      this.root.getElementById("resetDir").addEventListener("click", () => this.onResetDir());
      this.root.getElementById("resume").addEventListener("click", () => this.onResume());
      this.root.getElementById("deleteFiles").addEventListener("click", () => this.confirmDelete());
      this.root.getElementById("modalCancel").addEventListener("click", () => this.closeModal());
      this.root.getElementById("modalOk").addEventListener("click", () => void this.doDeleteFiles());
      void this.refresh();
      void listen("downloader:progress", (e) => {
        const p = e.payload;
        if (p.kind && p.kind !== svc.progressKind) return;
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
      const bar = this.root.getElementById("progressBar");
      bar.style.width = `${total > 0 ? downloaded / total * 100 : 0}%`;
      const st = this.root.getElementById("progressStatus");
      st.textContent = progressText(downloaded, total);
    }
    showProgress(on) {
      this.root.getElementById("progress").style.display = on ? "block" : "none";
    }
    /** Загрузка данных вкладки: применённый (активный) бандл, статусы всех
     * бандлов каталога в `statuses` (SSOT), затем рендер панели под ВЫБРАННЫЙ
     * в селекте бандл. Рендер — отдельный метод, чтобы смена селекта не пережимала весь fetch. */
    async refresh() {
      const svc = this.service;
      if (!svc) return;
      let info;
      try {
        info = await svc.info(null);
      } catch (e) {
        const label = this.root.getElementById("label");
        if (label) label.textContent = `\u041E\u0448\u0438\u0431\u043A\u0430: ${e}`;
        return;
      }
      this.appliedBundle = info.bundle_name;
      if (!this.selected) this.selected = info.bundle_name;
      const sel = this.root.getElementById("bundle");
      let entries = [];
      try {
        entries = await svc.entries();
      } catch {
      }
      sel.innerHTML = "";
      this.statuses = /* @__PURE__ */ new Map();
      this.statuses.set(info.bundle_name, info);
      for (const entry of entries) {
        if (this.statuses.has(entry.name)) continue;
        try {
          this.statuses.set(entry.name, await svc.info(entry.name));
        } catch {
        }
      }
      if (entries.length === 0) {
        const o = document.createElement("option");
        o.value = info.bundle_name;
        o.textContent = info.label;
        sel.appendChild(o);
      } else {
        for (const entry of entries) {
          const st = this.statuses.get(entry.name);
          const o = document.createElement("option");
          o.value = entry.name;
          if (!st) {
            o.textContent = `${entry.label} (\u043D\u0435 \u0441\u043A\u0430\u0447\u0430\u043D)`;
          } else if (st.fully_downloaded) {
            o.textContent = `${st.label} \u2014 \u0441\u043A\u0430\u0447\u0430\u043D${entry.is_default ? " (\u043F\u043E \u0443\u043C\u043E\u043B\u0447\u0430\u043D\u0438\u044E)" : ""}`;
          } else {
            o.textContent = `${st.label} \u2014 \u0447\u0430\u0441\u0442\u0438\u0447\u043D\u043E (${st.files.filter((f) => f.exists).length}/${st.files.length})`;
          }
          sel.appendChild(o);
        }
      }
      sel.value = this.selected;
      await this.renderPanel();
    }
    /** Статус бандла по имени: из кеша `statuses` (SSOT), при отсутствии — один
     * fetch и запись в кеш. Ошибка → undefined (звонящий обязан сказать об этом
     * юзеру, молчаливых отказов нет). */
    async statusFor(name) {
      const svc = this.service;
      if (!svc) return void 0;
      const cached = this.statuses.get(name);
      if (cached) return cached;
      try {
        const info = await svc.info(name || null);
        this.statuses.set(name, info);
        return info;
      } catch {
        return void 0;
      }
    }
    /** Перечень недостающих файлов статуса (строка состояния + подсказка кнопки). */
    static missingList(st) {
      return st.files.filter((f) => !f.exists).map((f) => f.filename);
    }
    /** Рендер ВСЕЙ панели под выбранный бандл (`this.selected`): строка
     * состояния, файлы, диск/память, кнопки операций. Контракт PRP 29.10.26 D1:
     * операции вкладки (скачать/докачать/удалить/активировать) работают с
     * ВЫБРАННЫМ бандлом; молчаливого скрытия кнопки «Активировать» нет —
     * недокачанный всегда назван причиной на экране. */
    async renderPanel() {
      const svc = this.service;
      if (!svc) return;
      const st = await this.statusFor(this.selected);
      const sel = this.root.getElementById("bundle");
      const stateEl = this.root.getElementById("state");
      if (!st) {
        stateEl.textContent = "\u0441\u0442\u0430\u0442\u0443\u0441 \u043D\u0435\u0434\u043E\u0441\u0442\u0443\u043F\u0435\u043D (\u0431\u044D\u043A\u0435\u043D\u0434 \u0432\u0435\u0440\u043D\u0443\u043B \u043E\u0448\u0438\u0431\u043A\u0443)";
        this.root.getElementById("label").textContent = `\u041E\u0448\u0438\u0431\u043A\u0430: \u043D\u0430\u0431\u043E\u0440 \xAB${this.selected}\xBB \u043D\u0435\u0434\u043E\u0441\u0442\u0443\u043F\u0435\u043D`;
        const activateBtn2 = this.root.getElementById("activate");
        activateBtn2.style.display = this.selected === this.appliedBundle ? "none" : "inline-block";
        activateBtn2.disabled = true;
        activateBtn2.title = "\u0421\u0442\u0430\u0442\u0443\u0441 \u0431\u0430\u043D\u0434\u043B\u0430 \u043D\u0435\u0438\u0437\u0432\u0435\u0441\u0442\u0435\u043D \u2014 \u0430\u043A\u0442\u0438\u0432\u0430\u0446\u0438\u044F \u0437\u0430\u0431\u043B\u043E\u043A\u0438\u0440\u043E\u0432\u0430\u043D\u0430 \u0432\u043A\u043B\u0430\u0434\u043A\u043E\u0439.";
        return;
      }
      sel.value = this.selected;
      const missing = _BundleTabElement.missingList(st);
      if (this.selected === this.appliedBundle) {
        stateEl.textContent = "\u0430\u043A\u0442\u0438\u0432\u043D\u044B\u0439";
      } else if (st.fully_downloaded) {
        stateEl.textContent = "\u0441\u043A\u0430\u0447\u0430\u043D \u043F\u043E\u043B\u043D\u043E\u0441\u0442\u044C\u044E \u2014 \u043C\u043E\u0436\u043D\u043E \u0430\u043A\u0442\u0438\u0432\u0438\u0440\u043E\u0432\u0430\u0442\u044C";
      } else if (missing.length === st.files.length) {
        stateEl.textContent = "\u043D\u0435 \u0441\u043A\u0430\u0447\u0430\u043D";
      } else {
        stateEl.textContent = `\u0447\u0430\u0441\u0442\u0438\u0447\u043D\u043E: \u0444\u0430\u0439\u043B\u043E\u0432 ${st.files.length - missing.length} \u0438\u0437 ${st.files.length}, \u043D\u0435\u0442: ${missing.join(", ")}`;
      }
      const activateBtn = this.root.getElementById("activate");
      if (this.selected === this.appliedBundle) {
        activateBtn.style.display = "none";
      } else {
        activateBtn.style.display = "inline-block";
        activateBtn.disabled = !st.fully_downloaded;
        activateBtn.title = st.fully_downloaded ? "" : `\u041D\u0435\u0434\u043E\u043A\u0430\u0447\u0430\u043D\u043D\u044B\u0435 \u0444\u0430\u0439\u043B\u044B (${missing.length}):
${missing.join("\n")}`;
      }
      this.root.getElementById("label").textContent = `${st.label}${st.fully_downloaded ? "" : " (\u043D\u0435 \u0432\u0441\u0435 \u0444\u0430\u0439\u043B\u044B \u0441\u043A\u0430\u0447\u0430\u043D\u044B)"}`;
      this.root.getElementById("note").textContent = st.note || "";
      const ul = this.root.getElementById("files");
      ul.innerHTML = "";
      let anyExists = false;
      for (const f of st.files) {
        if (f.exists) anyExists = true;
        const li = document.createElement("li");
        li.className = f.exists ? "ok" : "missing";
        const size = f.size_bytes ? ` \u2014 ${formatBytes(f.size_bytes)}` : "";
        li.textContent = `${f.exists ? "\u2713" : "\u25CB"} ${esc(f.filename)} (${f.role})${size}`;
        ul.appendChild(li);
      }
      const totalGb = (st.total_bytes / 1024 / 1024 / 1024).toFixed(1);
      this.root.getElementById("disk").textContent = `~${totalGb} \u0413\u0411 \u043D\u0430 \u0434\u0438\u0441\u043A\u0435 \xB7 \u0441\u0432\u043E\u0431\u043E\u0434\u043D\u043E ${st.free_space_gb} \u0413\u0411`;
      this.root.getElementById("mem").textContent = `GPU: ${st.vram_fast_gb ?? "?"} \u0413\u0411 (\u0431\u044B\u0441\u0442\u0440\u043E) / ${st.vram_min_gb ?? "?"} \u0413\u0411 (\u043C\u0438\u043D.) \xB7 RAM \u2265${st.ram_min_gb ?? "?"} \u0413\u0411`;
      this.root.getElementById("dir").textContent = st.save_dir;
      const dlBtn = this.root.getElementById("download");
      dlBtn.style.display = st.fully_downloaded ? "none" : "inline-block";
      dlBtn.textContent = anyExists ? "\u0414\u043E\u043A\u0430\u0447\u0430\u0442\u044C \u0444\u0430\u0439\u043B\u044B" : "\u0421\u043A\u0430\u0447\u0430\u0442\u044C \u043D\u0430\u0431\u043E\u0440";
      const resumeBtn = this.root.getElementById("resume");
      resumeBtn.style.display = st.fully_downloaded ? "none" : "inline-block";
      resumeBtn.disabled = !anyExists;
      this.root.getElementById("resetDir").style.display = st.save_dir ? "inline-block" : "none";
      this.root.getElementById("deleteFiles").style.display = st.fully_downloaded ? "inline-block" : "none";
      if (svc.memoryProbe) {
        try {
          this.root.getElementById("mem").textContent = await svc.memoryProbe(st.bundle_name);
        } catch {
        }
      }
    }
    /** Смена выбора в селекте: панель целиком перерисовывается под выбранный
     * бандл (строка состояния, файлы, кнопки) — больше не только «показать/
     * скрыть кнопку Activate». */
    async onBundleChange() {
      const sel = this.root.getElementById("bundle");
      this.selected = sel.value;
      await this.renderPanel();
    }
    async onActivate() {
      const svc = this.service;
      if (!svc || this.busy) return;
      const sel = this.root.getElementById("bundle");
      if (!sel.value) return;
      this.busy = true;
      try {
        await svc.activate(sel.value);
        notifyBundleChanged();
        toast(`\u0410\u043A\u0442\u0438\u0432\u043D\u044B\u0439 ${svc.kindLabel.toLowerCase()}: ${sel.value}`);
      } catch (e) {
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430 \u0430\u043A\u0442\u0438\u0432\u0430\u0446\u0438\u0438: ${e}`, "error");
      } finally {
        this.busy = false;
        await this.refresh();
      }
    }
    /** «Скачать набор»/«Докачать файлы»: качает ВЫБРАННЫЙ в селекте бандл
     * (PRP 29.10.26 D1 — раньше смотрел на активный и кнопка «докачать выбранный»
     * превращалась в тупик), всегда в настроенную папку (`save_dir` — кастом или
     * дефолт KingOrchData, пустым не бывает — бэкенд `commands.rs` гарантирует
     * дефолт). Выбор пути — ровно одно место на вкладке: кнопка «Изменить путь…»
     * (SSOT решений). Выбранная папка = ПУЛ для всех наборов (25.10.2026):
     * подпапка `${base}/${bundle_name}` не создаётся — она порождала перекачку
     * общих файлов. */
    async onDownload() {
      const svc = this.service;
      if (!svc || this.busy) return;
      const st = await this.statusFor(this.selected);
      if (!st) {
        toast("\u0421\u0442\u0430\u0442\u0443\u0441 \u0432\u044B\u0431\u0440\u0430\u043D\u043D\u043E\u0433\u043E \u043D\u0430\u0431\u043E\u0440\u0430 \u043D\u0435\u0434\u043E\u0441\u0442\u0443\u043F\u0435\u043D \u2014 \u0441\u043A\u0430\u0447\u0438\u0432\u0430\u043D\u0438\u0435 \u043E\u0442\u043C\u0435\u043D\u0435\u043D\u043E.", "error");
        return;
      }
      this.busy = true;
      const btn = this.root.getElementById("download");
      btn.disabled = true;
      this.showProgress(true);
      this.setProgress(0, 0);
      try {
        await svc.download(st.save_dir, st.bundle_name);
        notifyBundleChanged();
        toast(`\u041D\u0430\u0431\u043E\u0440 ${svc.kind === "video" ? "\u0432\u0438\u0434\u0435\u043E" : "\u0438\u0437\u043E\u0431\u0440\u0430\u0436\u0435\u043D\u0438\u0439"} \u0441\u043A\u0430\u0447\u0430\u043D!`);
      } catch (e) {
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430 \u0441\u043A\u0430\u0447\u0438\u0432\u0430\u043D\u0438\u044F \u043D\u0430\u0431\u043E\u0440\u0430: ${e}`, "error");
      } finally {
        this.busy = false;
        btn.disabled = false;
        this.showProgress(false);
        await this.refresh();
      }
    }
    /** «Изменить путь…»: диалог → проверка наличия файлов ВЫБРАННОГО бандла →
     * set → честный тост о нехватке. Папка задаётся ОДНА для всех бандлов
     * (общий пул, 25.10.2026); валидация — по выбранному имени (D1). */
    async onChangeDir() {
      const svc = this.service;
      if (!svc || this.busy) return;
      this.busy = true;
      const btn = this.root.getElementById("changeDir");
      btn.disabled = true;
      try {
        const sel = await open({ directory: true, title: `\u041F\u0430\u043F\u043A\u0430 \u043D\u0430\u0431\u043E\u0440\u0430 ${svc.kind === "video" ? "VideoGEN" : "ImageGEN"}` });
        if (!sel) return;
        const path = Array.isArray(sel) ? sel[0] : sel;
        if (!path) return;
        let valid;
        try {
          valid = await svc.validateDir(path, this.selected || null);
        } catch (e) {
          toast(`\u041E\u0448\u0438\u0431\u043A\u0430 \u043F\u0440\u043E\u0432\u0435\u0440\u043A\u0438 \u043D\u0430\u0431\u043E\u0440\u0430: ${e}`, "error");
          return;
        }
        await svc.setCustomDir(path);
        notifyBundleChanged();
        if (valid.valid) {
          toast("\u041F\u0430\u043F\u043A\u0430 \u043D\u0430\u0431\u043E\u0440\u0430 \u043E\u0431\u043D\u043E\u0432\u043B\u0435\u043D\u0430: \u0432\u0441\u0435 \u0444\u0430\u0439\u043B\u044B \u043D\u0430 \u043C\u0435\u0441\u0442\u0435.");
        } else {
          toast(`\u041F\u0430\u043F\u043A\u0430 \u0432\u044B\u0431\u0440\u0430\u043D\u0430. \u041D\u0435 \u0445\u0432\u0430\u0442\u0430\u0435\u0442 \u0444\u0430\u0439\u043B\u043E\u0432: ${valid.missing.length}. \u041D\u0430\u0436\u043C\u0438\u0442\u0435 \xAB\u0414\u043E\u043A\u0430\u0447\u0430\u0442\u044C \u0444\u0430\u0439\u043B\u044B\xBB.`);
        }
        await this.refresh();
      } catch (e) {
        toast(`\u041D\u0435 \u0443\u0434\u0430\u043B\u043E\u0441\u044C \u0441\u043C\u0435\u043D\u0438\u0442\u044C \u043F\u0430\u043F\u043A\u0443 \u043D\u0430\u0431\u043E\u0440\u0430: ${e}`, "error");
      } finally {
        btn.disabled = false;
        this.busy = false;
      }
    }
    /** «Сбросить путь»: вернуть папку набора к дефолту хранилища. */
    async onResetDir() {
      const svc = this.service;
      if (!svc || this.busy) return;
      this.busy = true;
      try {
        await svc.clearCustomDir();
        notifyBundleChanged();
        toast("\u041F\u0430\u043F\u043A\u0430 \u0441\u0431\u0440\u043E\u0448\u0435\u043D\u0430: \u0432\u0435\u0440\u043D\u0443\u043B\u0438\u0441\u044C \u043A \u0445\u0440\u0430\u043D\u0438\u043B\u0438\u0449\u0443 \u043F\u043E \u0443\u043C\u043E\u043B\u0447\u0430\u043D\u0438\u044E.");
      } catch (e) {
        toast(`\u041E\u0448\u0438\u0431\u043A\u0430: ${e}`, "error");
      } finally {
        this.busy = false;
        await this.refresh();
      }
    }
    /** Custom-папка (resume): докачать недостающие ВЫБРАННОГО бандла (PRP
     * 29.10.26 D1 — раньше качал активный, из-за чего выбранный недокачанный
     * бандл было невозможно догнать). */
    async onResume() {
      const svc = this.service;
      if (!svc || this.busy) return;
      const st = await this.statusFor(this.selected);
      if (!st) {
        toast("\u0421\u0442\u0430\u0442\u0443\u0441 \u0432\u044B\u0431\u0440\u0430\u043D\u043D\u043E\u0433\u043E \u043D\u0430\u0431\u043E\u0440\u0430 \u043D\u0435\u0434\u043E\u0441\u0442\u0443\u043F\u0435\u043D \u2014 \u0434\u043E\u043A\u0430\u0447\u043A\u0430 \u043E\u0442\u043C\u0435\u043D\u0435\u043D\u0430.", "error");
        return;
      }
      this.busy = true;
      const btn = this.root.getElementById("resume");
      btn.disabled = true;
      this.showProgress(true);
      this.setProgress(0, 0);
      try {
        await svc.download(st.save_dir, st.bundle_name);
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
    confirmDelete() {
      this.root.getElementById("overlay").classList.add("open");
    }
    closeModal() {
      this.root.getElementById("overlay").classList.remove("open");
    }
    async doDeleteFiles() {
      const svc = this.service;
      if (!svc || this.busy) return;
      this.busy = true;
      this.closeModal();
      try {
        await svc.removeFiles(this.selected || null);
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
  if (!customElements.get("image-bundle-tab")) {
    customElements.define("image-bundle-tab", BundleTabElement);
  }

  // guest-js/components/media-panel.ts
  var MediaEnginePanel = class extends HTMLElement {
    connectedCallback() {
      if (this.root) return;
      this.root = this.attachShadow({ mode: "open" });
      this.root.innerHTML = `
      <style>${STYLE}
        .tabs-row { margin: 0 0 10px; }
        .tab-panel { margin-top: 10px; }
      </style>
      <div>
        <div class="row tabs-row">
          <button id="tabImages" class="tab-btn primary" aria-pressed="true">\u041A\u0430\u0440\u0442\u0438\u043D\u043A\u0438</button>
          <button id="tabVideo" class="tab-btn" aria-pressed="false">\u0412\u0438\u0434\u0435\u043E</button>
        </div>
        <div class="tab-panel" id="panelImages"><image-bundle-tab service="image"></image-bundle-tab></div>
        <div class="tab-panel" id="panelVideo" style="display:none;"><image-bundle-tab service="video"></image-bundle-tab></div>
      </div>`;
      const btnImages = this.root.getElementById("tabImages");
      const btnVideo = this.root.getElementById("tabVideo");
      const panelImages = this.root.getElementById("panelImages");
      const panelVideo = this.root.getElementById("panelVideo");
      const activate = (showVideo) => {
        btnImages.setAttribute("aria-pressed", showVideo ? "false" : "true");
        btnVideo.setAttribute("aria-pressed", showVideo ? "true" : "false");
        btnImages.classList.toggle("primary", !showVideo);
        btnVideo.classList.toggle("primary", showVideo);
        panelImages.style.display = showVideo ? "none" : "block";
        panelVideo.style.display = showVideo ? "block" : "none";
      };
      btnImages.addEventListener("click", () => activate(false));
      btnVideo.addEventListener("click", () => activate(true));
    }
  };
  if (!customElements.get("image-media-panel")) {
    customElements.define("image-media-panel", MediaEnginePanel);
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
  function getImageBundleInfo(bundleName) {
    return invoke("plugin:image-engine|get_image_bundle_info", { bundleName: bundleName ?? null });
  }
  function setSelectedImageBundle(bundleName) {
    return invoke("plugin:image-engine|set_selected_image_bundle", { bundleName });
  }
  function downloadImageBundle(saveDir, bundleName) {
    return invoke("plugin:image-engine|download_image_bundle", { saveDir, bundleName: bundleName ?? null });
  }
  function validateImageBundleDir(path, bundleName) {
    return invoke("plugin:image-engine|validate_image_bundle_dir", { path, bundleName: bundleName ?? null });
  }
  function setImageBundleDir(path) {
    return invoke("plugin:image-engine|set_image_bundle_dir", { path });
  }
  function removeImageBundle(bundleName) {
    return invoke("plugin:image-engine|remove_image_bundle", { bundleName: bundleName ?? null });
  }
  function removeImageBundleFromList() {
    return invoke("plugin:image-engine|remove_image_bundle_from_list");
  }
  function estimateImageMemory(bundleName) {
    return invoke("plugin:image-engine|estimate_image_memory", { bundleName: bundleName ?? null });
  }
  function generateImage(opts) {
    return invoke("plugin:image-engine|generate_image", {
      prompt: opts.prompt,
      width: opts.width ?? null,
      height: opts.height ?? null,
      steps: opts.steps ?? null,
      cfgScale: opts.cfgScale ?? null,
      seed: opts.seed ?? null,
      outPath: opts.outPath ?? null,
      bundleName: opts.bundleName ?? null,
      loraNames: opts.loraNames ?? null,
      loraStrength: opts.loraStrength ?? null
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
      outPath: opts.outPath ?? null,
      bundleName: opts.bundleName ?? null,
      loraNames: opts.loraNames ?? null,
      loraStrength: opts.loraStrength ?? null
    });
  }
  function getVideoBundles() {
    return invoke("plugin:image-engine|get_video_bundles");
  }
  function getVideoBundleInfo(bundleName) {
    return invoke("plugin:image-engine|get_video_bundle_info", { bundleName: bundleName ?? null });
  }
  function setSelectedVideoBundle(bundleName) {
    return invoke("plugin:image-engine|set_selected_video_bundle", { bundleName });
  }
  function downloadVideoBundle(saveDir, bundleName) {
    return invoke("plugin:image-engine|download_video_bundle", { saveDir, bundleName: bundleName ?? null });
  }
  function validateVideoBundleDir(path, bundleName) {
    return invoke("plugin:image-engine|validate_video_bundle_dir", { path, bundleName: bundleName ?? null });
  }
  function setVideoBundleDir(path) {
    return invoke("plugin:image-engine|set_video_bundle_dir", { path: path ?? null });
  }
  function removeVideoBundleFiles(bundleName) {
    return invoke("plugin:image-engine|remove_video_bundle_files", { bundleName: bundleName ?? null });
  }
  function generateVideo(opts) {
    return invoke("plugin:image-engine|generate_video", {
      prompt: opts.prompt,
      negativePrompt: opts.negativePrompt ?? null,
      initPath: opts.initPath ?? null,
      endPath: opts.endPath ?? null,
      refPaths: opts.refPaths ?? [],
      seconds: opts.seconds ?? null,
      width: opts.width ?? null,
      height: opts.height ?? null,
      steps: opts.steps ?? null,
      cfgScale: opts.cfgScale ?? null,
      seed: opts.seed ?? null,
      finalQuality: opts.finalQuality ?? null,
      strength: opts.strength ?? null,
      vaeTiling: opts.vaeTiling ?? null,
      outPath: opts.outPath ?? null,
      bundleName: opts.bundleName ?? null
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
          downloadVideoBundle,
          editImage,
          estimateImageMemory,
          generateImage,
          generateVideo,
          getImageBundleInfo,
          getImageEngineStatus,
          getImageModelsCatalog,
          getVideoBundleInfo,
          getVideoBundles,
          installImageEngine,
          installImageEngineUpdate,
          notifyBundleChanged,
          removeImageBundle,
          removeImageBundleFromList,
          removeVideoBundleFiles,
          removeImageEngine,
          setImageBundleDir,
          setVideoBundleDir,
          setImageEngineDir,
          setImageEngineVariant,
          setSelectedImageBundle,
          setSelectedVideoBundle,
          validateImageBundleDir,
          validateVideoBundleDir
        }
      });
    }
  }
})();
