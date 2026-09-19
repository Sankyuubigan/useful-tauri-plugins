// Единый скрипт синхронизации версий (правило YY.M.P из глобальной доки §3.2).
// Все сценарии (build/release/prep) используют ТОЛЬКО эти функции — никаких
// отдельных реализаций бампа в оркестраторах.

const fs = require('fs');
const path = require('path');

const VERSION_RE = /"version"\s*:\s*"(\d+)\.(\d+)\.(\d+)"/;

// Читает версию из tauri.conf.json (единственный источник правды).
function readVersion(projectRoot) {
    const confPath = path.join(projectRoot, 'src-tauri', 'tauri.conf.json');
    const confText = fs.readFileSync(confPath, 'utf8');
    const m = confText.match(VERSION_RE);
    if (!m) throw new Error(`Версия не найдена в tauri.conf.json (${confPath})`);
    return `${m[1]}.${m[2]}.${m[3]}`;
}

function readLatestVersion(projectRoot) {
    const latestPath = path.join(projectRoot, 'latest.json');
    if (!fs.existsSync(latestPath)) return null;
    try {
        const latest = JSON.parse(fs.readFileSync(latestPath, 'utf8'));
        const m = String(latest.version).match(/^(\d+)\.(\d+)\.(\d+)$/);
        if (!m) return null;
        return { maj: parseInt(m[1], 10), min: parseInt(m[2], 10), pat: parseInt(m[3], 10) };
    } catch (e) {
        return null; // latest.json повреждён — игнорируем (как в исходном build.cjs)
    }
}

function writeVersionToConf(projectRoot, version, syncPackageJson) {
    const confPath = path.join(projectRoot, 'src-tauri', 'tauri.conf.json');
    const cargoPath = path.join(projectRoot, 'src-tauri', 'Cargo.toml');

    let confText = fs.readFileSync(confPath, 'utf8');
    confText = confText.replace(/"version"\s*:\s*".*?"/, `"version": "${version}"`);
    fs.writeFileSync(confPath, confText, 'utf8');

    let cargoText = fs.readFileSync(cargoPath, 'utf8');
    cargoText = cargoText.replace(/^version\s*=\s*".*?"/m, `version = "${version}"`);
    fs.writeFileSync(cargoPath, cargoText, 'utf8');

    if (syncPackageJson) {
        const pkgPath = path.join(projectRoot, 'package.json');
        if (fs.existsSync(pkgPath)) {
            const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));
            if (pkg.version && pkg.version !== version) {
                pkg.version = version;
                fs.writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + '\n', 'utf8');
                console.log(`  package.json: ${version}`);
            }
        }
    }
}

// Инкремент патча по YY.M.P. При guardAgainstLatest (дефолт) берём строго
// бОльшую версию, чем в latest.json — защита от отката ниже уже выпущенного
// релиза (инцидент 26.8.173 < 26.8.174).
function bumpVersion(projectRoot, opts = {}) {
    const syncPackageJson = opts.syncPackageJson === true;
    const guardAgainstLatest = opts.guardAgainstLatest !== false;

    const confPath = path.join(projectRoot, 'src-tauri', 'tauri.conf.json');
    const confText = fs.readFileSync(confPath, 'utf8');
    const match = confText.match(VERSION_RE);
    if (!match) throw new Error('Версия не найдена в tauri.conf.json');

    const oldMaj = parseInt(match[1], 10);
    const oldMin = parseInt(match[2], 10);
    const oldPat = parseInt(match[3], 10);

    const now = new Date();
    const curMaj = parseInt(now.getFullYear().toString().slice(-2), 10);
    const curMin = now.getMonth() + 1;

    let newMaj = curMaj;
    let newMin = curMin;
    let newPat = (oldMaj === curMaj && oldMin === curMin) ? oldPat + 1 : 1;

    if (guardAgainstLatest) {
        const rel = readLatestVersion(projectRoot);
        if (rel) {
            const below =
                newMaj < rel.maj ||
                (newMaj === rel.maj && newMin < rel.min) ||
                (newMaj === rel.maj && newMin === rel.min && newPat <= rel.pat);
            if (below) {
                newMaj = rel.maj;
                newMin = rel.min;
                newPat = rel.pat + 1;
            }
        }
    }

    const version = `${newMaj}.${newMin}.${newPat}`;
    writeVersionToConf(projectRoot, version, syncPackageJson);
    return version;
}

module.exports = { readVersion, bumpVersion, writeVersionToConf };