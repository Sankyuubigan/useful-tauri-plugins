// Точка входа vanilla-канала плагина (esbuild -> api-iife.js, см. PLUGIN_STANDARD.md §4.4).
// Не содержит голых импортов '@tauri-apps/*' — они заменены алиасами esbuild на shims/,
// которые биндятся на window.__TAURI__ (вшит Tauri при withGlobalTauri: true).
import {
  checkForUpdate,
  downloadAndInstallUpdate,
  getAppVersion,
  getReleaseHistory,
  getSupportUrl,
  installRelease,
} from './index'

const g = window as unknown as { __TAURI__?: Record<string, unknown> }

if ('__TAURI__' in window) {
  const tauri = g.__TAURI__
  if (tauri && !tauri['about-updates']) {
    Object.defineProperty(tauri, 'about-updates', {
      configurable: true,
      value: {
        checkForUpdate,
        downloadAndInstallUpdate,
        getAppVersion,
        getReleaseHistory,
        getSupportUrl,
        installRelease,
      },
    })
  }
}