// build / prep. Единый prep (version + sync + npm + icons) используется и
// installer, и release. Dev-overrides (bundle.active=false, devWindow) — только
// для dev-сборки; npm-install и бамп версии остаются как в эталоне.

const fs = require('fs');
const path = require('path');

const { run: sh, launchApp, killApp } = require('./run.cjs');
const { bumpVersion, readVersion } = require('./version.cjs');
const { ensureIcons } = require('./icons.cjs');
const { syncConfiguredResources } = require('./sync-resources.cjs');
const { copyVcRedistIfConfigured } = require('./copy-vc-redist.cjs');

const BUNDLE_DIR = (cfg) => path.join(cfg.projectRoot, 'src-tauri', 'target', 'release', 'bundle');
const RELEASE_EXE = (cfg) =>
    cfg.appExe ? path.join(cfg.projectRoot, 'src-tauri', 'target', 'release', `${cfg.appExe}.exe`) : null;

function readTauriJson(cfg) {
    const p = path.join(cfg.projectRoot, 'src-tauri', 'tauri.conf.json');
    return JSON.parse(fs.readFileSync(p, 'utf8'));
}

function writeTauriJson(cfg, obj) {
    const p = path.join(cfg.projectRoot, 'src-tauri', 'tauri.conf.json');
    fs.writeFileSync(p, JSON.stringify(obj, null, 2) + '\n', 'utf8');
}

function runNpmInstall(cfg) {
    const pkg = path.join(cfg.projectRoot, 'package.json');
    if (!fs.existsSync(pkg)) {
        console.log('package.json not found, skip npm install.');
        return;
    }
    const args = ['install', ...(cfg.npmInstallArgs || [])];
    sh(cfg, 'npm', args, { echo: true });
}

// Dev-оверрайд записывается ОТДЕЛЬНЫМ файлом и подмешивается через
// `npx tauri build --config <override>` — коммиченный tauri.conf.json не
// трогается (как в эталоне). bundle отключается только для dev; окно —
// devWindow из конфига (иначе не трогаем app.windows).
function buildDevOverride(cfg) {
    const override = { bundle: { active: false } };
    if (cfg.devWindow) {
        override.app = { windows: [cfg.devWindow] };
    }
    return override;
}

// Для installer/release: убедиться, что бамдлинг включён (dev-сборка могла его отключить).
function ensureBundleActive(cfg) {
    const conf = readTauriJson(cfg);
    if (conf.bundle && conf.bundle.active === false) {
        conf.bundle.active = true;
        writeTauriJson(cfg, conf);
        console.log('Bundle active restored (true).');
    }
}

function checkExeFresh(cfg) {
    const exe = RELEASE_EXE(cfg);
    if (!exe || !fs.existsSync(exe)) {
        throw new Error(`Built exe not found: ${exe}`);
    }
    const ageSec = (Date.now() - fs.statSync(exe).mtimeMs) / 1000;
    if (ageSec > 600) {
        throw new Error(`Built exe is stale (${Math.round(ageSec)}s). Build did not refresh it: ${exe}`);
    }
    return exe;
}

function getExePath(cfg) {
    const exe = RELEASE_EXE(cfg);
    if (!exe || !fs.existsSync(exe)) return null;
    return exe;
}

// --- prep: version bump + sync resources + npm install + icons. ---
function prep(cfg) {
    console.log('=== prep ===');
    const oldVersion = readVersion(cfg.projectRoot);
    const newVersion = bumpVersion(cfg.projectRoot, { syncPackageJson: cfg.syncPackageJsonVersion });
    console.log(`Version: ${oldVersion} -> ${newVersion}`);
    syncConfiguredResources(cfg);
    runNpmInstall(cfg);
    return ensureIcons(cfg);
}

// --- build (dev) ---
async function run(cfg, opts = {}) {
    const prepOnly = opts.prepOnly === true;

    await prep(cfg);
    if (prepOnly) {
        console.log('\n[prep] done. Run `installer` or `release` next.');
        return { skipped: true };
    }

    console.log('\n=== build (dev) ===');
    const ovrPath = path.join(cfg.projectRoot, 'src-tauri', 'tauri-dev-override.json');

    killApp(cfg); // освободить file lock exe до пересборки
    fs.writeFileSync(ovrPath, JSON.stringify(buildDevOverride(cfg), null, 2) + '\n', 'utf8');
    try {
        sh(cfg, 'npx', ['tauri', 'build', '--config', ovrPath], { echo: true });
    } finally {
        fs.rmSync(ovrPath, { force: true });
    }

    const exe = checkExeFresh(cfg);
    copyVcRedistIfConfigured(cfg, exe);

    launchApp(cfg, exe);
    console.log('\n[build] done.');
    return { exe };
}

module.exports = {
    run,
    prep,
    buildDevOverride,
    ensureBundleActive,
    checkExeFresh,
    getExePath,
    BUNDLE_DIR,
    RELEASE_EXE,
};