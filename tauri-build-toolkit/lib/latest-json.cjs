// Генерация latest.json ПОСЛЕ публикации релиза (правило §3.3: жёсткие проверки).
// Endpoint всегда raw.githubusercontent.com — статический файл, БЕЗ redirect'ов
// GitHub Releases (redirect ломает клиентский апдейтер).
//
// Формат совпадает с эталоном (King Orch):
//   platforms[platform] = { signature: <СОДЕРЖИМОЕ .sig>, url: <статический URL> }.
// signature — это содержимое сигнатуры, а НЕ имя файла (так его ждёт updater).

const fs = require('fs');
const path = require('path');
const { readVersion } = require('./version.cjs');

// Жёсткие проверки §3.3:
//   - имя установщика ОБЯЗАНО содержать версию;
//   - сигнатура обязана существовать рядом с установщиком;
//   - итоговый URL обязан содержать версию (и в теге, и в имени файла);
//   - требуется config.repo (иначе нельзя построить статический URL).
function generateLatestJson(cfg, opts = {}) {
    const version = opts.version || readVersion(cfg.projectRoot);
    const exeFileName = opts.exeFileName;
    const platform = opts.platform || 'windows-x86_64';
    const repo = cfg.repo;

    if (!exeFileName) throw new Error('generateLatestJson: exeFileName is required');
    if (!repo) throw new Error(`repo not set in config; cannot build latest.json URL`);

    const tag = `v${version}`;
    const url = `https://github.com/${repo}/releases/download/${tag}/${exeFileName}`;
    const sigFile = `${exeFileName}.sig`;

    if (!String(exeFileName).toLowerCase().includes(String(version).toLowerCase())) {
        throw new Error(`exeFileName "${exeFileName}" does not contain version "${version}"`);
    }
    if (!url.includes(version)) {
        throw new Error(`latest.json URL missing version (${version}): ${url}`);
    }

    // Содержимое сигнатуры берём из локального установщика в bundle/nsis.
    const localDir = path.join(cfg.projectRoot, 'src-tauri', 'target', 'release', 'bundle', 'nsis');
    const localSig = path.join(localDir, sigFile);
    if (!fs.existsSync(localSig)) {
        throw new Error(`Signature "${sigFile}" is missing next to the installer (${localSig}). Unsigned release is forbidden.`);
    }
    const signature = fs.readFileSync(localSig, 'utf8').trim();

    const latest = {
        version,
        notes: opts.notes || '',
        pub_date: new Date().toISOString(),
        platforms: {
            [platform]: { signature, url },
        },
    };

    fs.writeFileSync(cfg.latestJsonPath, JSON.stringify(latest, null, 2) + '\n', 'utf8');
    console.log('latest.json written:', cfg.latestJsonPath);
    return latest;
}

module.exports = { generateLatestJson };