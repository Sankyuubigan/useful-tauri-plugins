// Синхронизация каталогов из корня проекта в src-tauri/resources (для каждого
// имени из config.syncDirs). Заменяет scripts/sync-resources.cjs из эталона.

// Sync-зеркалирование каталогов из корня проекта в target/{debug,release}.
// Каждый элемент cfg.syncDirs — путь относительно projectRoot (может быть вложенным,
// напр. "src-tauri/mcp_servers"); имя итоговой папки = basename(src).
//
// Зачем (эталон King Orch): Tauri копирует ресурсы аддитивно, не удаляя файлы,
// выпиленные из исходников → в target-папках накапливаются «призраки». Здесь
// target-копия зеркалит исходник (копирует новое, удаляет отсутствующее).

const fs = require('fs');
const path = require('path');

function syncDir(src, dest) {
    if (!fs.existsSync(dest)) fs.mkdirSync(dest, { recursive: true });
    const srcNames = new Set(fs.readdirSync(src));
    for (const name of fs.readdirSync(dest)) {
        if (!srcNames.has(name)) {
            fs.rmSync(path.join(dest, name), { recursive: true, force: true });
        }
    }
    for (const name of srcNames) {
        const s = path.join(src, name);
        const d = path.join(dest, name);
        const st = fs.statSync(s);
        if (st.isDirectory()) syncDir(s, d);
        else if (st.isFile()) fs.copyFileSync(s, d);
    }
}

function syncConfiguredResources(cfg) {
    const entries = cfg.syncDirs;
    if (!entries || entries.length === 0) {
        console.log('syncDirs: not configured, skip resource sync.');
        return;
    }
    const targetDir = path.join(cfg.projectRoot, 'src-tauri', 'target');
    for (const rel of entries) {
        const src = path.join(cfg.projectRoot, rel);
        if (!fs.existsSync(src)) {
            console.warn(`syncDirs: source not found, skipped: ${rel}`);
            continue;
        }
        const relName = path.basename(rel);
        for (const profile of ['debug', 'release']) {
            const dest = path.join(targetDir, profile, relName);
            if (!fs.existsSync(path.dirname(dest))) continue;
            syncDir(src, dest);
            console.log(`[sync-resources] ${relName}/ -> ${dest}`);
        }
    }
}

module.exports = { syncConfiguredResources, syncDir };