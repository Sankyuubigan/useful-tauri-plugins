"use strict";
(() => {
  // guest-js/shims/core.ts
  var core = window.__TAURI__?.core ?? window.__TAURI_INTERNALS__;
  var invoke = core.invoke.bind(core);

  // guest-js/index.ts
  async function getFfmpegStatus(dir) {
    return invoke("plugin:ffmpeg|ffmpeg_status", { dir });
  }

  // guest-js/iife-entry.ts
  var g = window;
  if ("__TAURI__" in window) {
    const tauri = g.__TAURI__;
    if (tauri && !tauri["ffmpeg"]) {
      Object.defineProperty(tauri, "ffmpeg", {
        configurable: true,
        value: { getFfmpegStatus }
      });
    }
  }
})();
