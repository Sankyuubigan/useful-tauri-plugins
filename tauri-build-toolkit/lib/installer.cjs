// installer: prep + npx tauri build --bundles nsis + РІРµСЂРёС„РёРєР°С†РёСЏ СѓСЃС‚Р°РЅРѕРІС‰РёРєР°
// Рё СЃРёРіРЅР°С‚СѓСЂС‹ (signer lenient: Р±РµР· РєР»СЋС‡Р° вЂ” РїСЂРµРґСѓРїСЂРµР¶РґР°РµРј Рё СЃРѕР±РёСЂР°РµРј Р±РµР· РїРѕРґРїРёСЃРё).

const fs = require('fs');
const path = require('path');

const { run: sh, killApp } = require('./run.cjs');
const { prep, ensureBundleActive, BUNDLE_DIR } = require('./build.cjs');
const { findInstallerBundle } = require('./selector.cjs');
const { setupSigningEnv, signInstaller } = require('./signing.cjs');
const { copyVcRedistTo } = require('./copy-vc-redist.cjs');
const { readVersion } = require('./version.cjs');

function cleanupOldBundle(cfg) {
    // РЈРґР°Р»СЏРµРј СЃС‚Р°СЂС‹Рµ СѓСЃС‚Р°РЅРѕРІС‰РёРєРё/СЃРёРіРЅР°С‚СѓСЂС‹, С‡С‚РѕР±С‹ СЃРµР»РµРєС‚РѕСЂ РЅРµ РЅР°С€С‘Р» Р»РёС€РЅРµРµ.
    const nsisDir = path.join(BUNDLE_DIR(cfg), 'nsis');
    if (!fs.existsSync(nsisDir)) return;
    for (const f of fs.readdirSync(nsisDir)) {
        if (/\.(exe|sig)$/.test(f) && /_setup|setup/.test(f)) {
            try { fs.unlinkSync(path.join(nsisDir, f)); console.log('removed old:', f); } catch (e) {}
        }
    }
}

async function run(cfg) {
    await prep(cfg);

    console.log('\n=== installer ===');
    cleanupOldBundle(cfg);

    killApp(cfg); // РѕСЃРІРѕР±РѕРґРёС‚СЊ file lock exe РґРѕ РїРµСЂРµСЃР±РѕСЂРєРё
    setupSigningEnv(cfg, { strict: false });
    ensureBundleActive(cfg);

    // VC++ runtime DLL РєРѕРїРёСЂСѓСЋС‚СЃСЏ Р”Рћ СЃР±РѕСЂРєРё, С‡С‚РѕР±С‹ РїРѕРїР°СЃС‚СЊ РІ NSIS-Р±Р°РЅРґР».
    copyVcRedistTo(cfg, path.join(cfg.projectRoot, 'src-tauri', 'target', 'release'));

    sh(cfg, 'npx', ['tauri', 'build', '--bundles', 'nsis'], { echo: true });

    const version = readVersion(cfg.projectRoot);
    const installer = findInstallerBundle(BUNDLE_DIR(cfg), version);
    if (!installer) {
        throw new Error(`Installer not found in ${BUNDLE_DIR(cfg)} for version ${version}`);
    }
    console.log('Installer:', installer);

    signInstaller(cfg, installer, { strict: false });

    console.log('\n[installer] done.');
    return installer;
}

module.exports = { run };
