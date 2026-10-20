// release: СЃР±РѕСЂРєР° NSIS РїРѕРґРїРёСЃРё -> gh release -> latest.json -> РєРѕРјРјРёС‚ РІРµСЂСЃРёРё.
// Р“Р°СЂРґС‹: checkRepoMatches РїРµСЂРµРґ Р›Р®Р‘Р«РњР git/gh РѕРїРµСЂР°С†РёСЏРјРё; gh release create
// РІСЃРµРіРґР° СЃ --repo; РІРµС‚РєР° вЂ” Р°РІС‚РѕРґРµС‚РµРєС‚; latest.json С‚РѕР»СЊРєРѕ РџРћРЎР›Р• РїСѓР±Р»РёРєР°С†РёРё.

const fs = require('fs');
const path = require('path');

const { run: sh, killApp } = require('./run.cjs');
const { prep, ensureBundleActive, BUNDLE_DIR } = require('./build.cjs');
const { findInstallerBundle } = require('./selector.cjs');
const { setupSigningEnv, signInstaller } = require('./signing.cjs');
const { copyVcRedistTo } = require('./copy-vc-redist.cjs');
const { generateLatestJson } = require('./latest-json.cjs');
const { readVersion } = require('./version.cjs');
const {
    checkRepoMatches,
    checkCommittedFilesNotIgnored,
    checkGhAuth,
    ghReleaseCreate,
    commitAndPush,
    gitFetchTagsForce,
    getBranch,
    releaseCommittedFiles,
    assertNotAlreadyPartiallyPublished,
} = require('./github.cjs');
const { printReleaseState } = require('./release-state.cjs');

function cleanupBundle(cfg) {
    const nsisDir = path.join(BUNDLE_DIR(cfg), 'nsis');
    if (!fs.existsSync(nsisDir)) return;
    for (const f of fs.readdirSync(nsisDir)) {
        if (/_(?:\d+\.){2}\d+_x64-setup\.exe(?:\.sig)?$/.test(f)) {
            try { fs.unlinkSync(path.join(nsisDir, f)); console.log('cleanup:', f); } catch (e) {}
        }
    }
}

async function run(cfg) {
    checkRepoMatches(cfg);
    // Гард №3: latest.json и файлы версий не должны игнорироваться .gitignore.
    // Без этой проверки падение на `git add` случилось бы уже ПОСЛЕ публикации
    // релиза на GitHub — релиз есть, а манифеста апдейтера в репозитории нет.
    checkCommittedFilesNotIgnored(cfg); // Р“РђР Р” в„–1: РґРѕ Р»СЋР±С‹С… git/gh РѕРїРµСЂР°С†РёР№

    // ГАРД §2.6.2: частично опубликованный релиз (тег есть, release-коммит не
    // запушен) НЕ должен пройти дальше prep(): prep() бампает версию -> повтор
    // незаметно переехал бы на следующий тег и оставил незапушенный manifest.
    // Бросок ДО prep() = до траты минут на пересборку.
    assertNotAlreadyPartiallyPublished(cfg);

    await prep(cfg);

    console.log('\n=== release ===');
    cleanupBundle(cfg); // pre-release cleanup (РїСЂР°РІРёР»Рѕ: Р±РµР· Р°СЂС‚РµС„Р°РєС‚РѕРІ РїСЂРѕС€Р»РѕРіРѕ СЂРµР»РёР·Р°)
    killApp(cfg); // РѕСЃРІРѕР±РѕРґРёС‚СЊ file lock exe РґРѕ РїРµСЂРµСЃР±РѕСЂРєРё

    setupSigningEnv(cfg, { strict: true });
    ensureBundleActive(cfg);

    // VC++ runtime DLL РєРѕРїРёСЂСѓСЋС‚СЃСЏ Р”Рћ СЃР±РѕСЂРєРё, С‡С‚РѕР±С‹ РїРѕРїР°СЃС‚СЊ РІ NSIS-Р±Р°РЅРґР».
    copyVcRedistTo(cfg, path.join(cfg.projectRoot, 'src-tauri', 'target', 'release'));

    sh(cfg, 'npx', ['tauri', 'build', '--bundles', 'nsis'], { echo: true });

    const version = readVersion(cfg.projectRoot);
    let installer = findInstallerBundle(BUNDLE_DIR(cfg), version);
    if (!installer) {
        throw new Error(`Installer not found in ${BUNDLE_DIR(cfg)} for version ${version}`);
    }

    const sig = signInstaller(cfg, installer, { strict: true });
    if (!sig) {
        throw new Error('Release requires a signed installer (strict).');
    }

    // РџСЂРѕР±РµР»С‹ РІ РёРјРµРЅРё СѓСЃС‚Р°РЅРѕРІС‰РёРєР° Р»РѕРјР°СЋС‚ URL/CLI вЂ” Р·Р°РјРµРЅСЏРµРј РЅР° С‚РѕС‡РєРё (РєР°Рє СЌС‚Р°Р»РѕРЅ).
    const dir = path.dirname(installer);
    const dottedName = path.basename(installer).replace(/\s+/g, '.');
    if (dottedName !== path.basename(installer)) {
        const renamed = path.join(dir, dottedName);
        fs.renameSync(installer, renamed);
        if (fs.existsSync(`${installer}.sig`)) fs.renameSync(`${installer}.sig`, `${renamed}.sig`);
        installer = renamed;
        console.log('Renamed installer for URL-safe asset:', dottedName);
    }

    const exeFileName = path.basename(installer);
    const tag = `v${version}`;
    const product = cfg.productName || cfg.appExe || 'app';
    const notes = (cfg.releaseNotesTemplate || 'Auto release ${product} ${tag}')
        .replace('${product}', product)
        .replace('${tag}', tag);

    checkGhAuth(cfg);

    // Обогащение каждой фазы общим state (§2.6.1). Печатается на обоих путях
    // (success/fail) — правдивое состояние релиза вместо голого [ERROR].
    const branch = getBranch(cfg);
    const state = { published: false, latestJson: false, committed: false, pushed: false, fetchTags: false };
    try {
        ghReleaseCreate(cfg, {
            tag,
            title: tag,
            notes,
            assets: [installer, `${installer}.sig`],
        });
        state.published = true;

        generateLatestJson(cfg, { version, exeFileName, notes: `${product} ${version}` });
        state.latestJson = true;
    } catch (e) {
        printReleaseState(state, { tag, version, branch, error: e });
        throw e;
    }

    try {
        const pushSteps = await commitAndPush(cfg, {
            // SSOT: тот же список, который проверяет гард checkCommittedFilesNotIgnored.
            files: releaseCommittedFiles(cfg),
            message: `chore(release): ${tag}`,
            branch,
        });
        state.committed = pushSteps.committed;
        state.pushed = pushSteps.pushed;
        await gitFetchTagsForce(cfg);
        state.fetchTags = true;
    } catch (e) {
        const extra = e.steps || {};
        state.committed = Boolean(extra.committed ?? state.committed);
        state.pushed = Boolean(extra.pushed ?? state.pushed);
        printReleaseState(state, { tag, version, branch, error: e });
        throw e;
    }

    printReleaseState(state, { tag, version, branch });

    // Success report: консоль не закрывается молча — печатаем итоги релиза
    // (сам шаблон release.bat делает pause после завершения).
    const releaseUrl = `https://github.com/${cfg.repo}/releases/tag/${tag}`;
    const lines = [
        ['Tag', tag],
        ['Product', product],
        ['Installer', installer],
        ['Release', releaseUrl],
        ['latest.json', cfg.latestJsonPath],
        ['Branch', `${branch} (pushed)`],
    ];
    const w1 = Math.max(...lines.map(([k]) => k.length));
    const w2 = Math.max(...lines.map(([, v]) => String(v).length));
    const bar = '='.repeat(Math.max(40, w1 + w2 + 5));
    console.log('\n' + bar);
    console.log('  RELEASE SUCCESS');
    console.log(bar);
    for (const [k, v] of lines) {
        console.log(`  ${k.padEnd(w1)}  ${String(v).padEnd(w2)}`);
    }
    console.log(bar);

    console.log('\n[release] done. Tag:', tag);
}

module.exports = { run };
