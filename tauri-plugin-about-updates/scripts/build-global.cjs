// Сборка vanilla-канала плагина: guest-js/ -> api-iife.js (IIFE, без голых импортов).
// Запуск: npm run build:global. Артефакт КОММИТИТСЯ и руками не правится (SSOT).
const path = require('path')
const { build } = require('esbuild')

const root = path.join(__dirname, '..')

build({
  entryPoints: [path.join(root, 'guest-js', 'iife-entry.ts')],
  bundle: true,
  format: 'iife',
  target: 'es2020',
  outfile: path.join(root, 'api-iife.js'),
  alias: {
    '@tauri-apps/api/core': path.join(root, 'guest-js', 'shims', 'core.ts'),
    '@tauri-apps/plugin-updater': path.join(root, 'guest-js', 'shims', 'updater.ts'),
    '@tauri-apps/plugin-shell': path.join(root, 'guest-js', 'shims', 'shell.ts'),
  },
  banner: {
    js: '// Generated from guest-js/ by esbuild (npm run build:global). Do not edit by hand.',
  },
  logLevel: 'info',
}).catch(() => {
  console.error('[build:global] esbuild failed')
  process.exit(1)
})