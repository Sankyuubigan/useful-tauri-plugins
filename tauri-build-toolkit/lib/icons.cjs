// Иконки: дефолтный набор + регенерация через `npx tauri icon`.
// Если src-tauri/icons/icon.ico отсутствует или битый — берем app-icon.png из
// корня проекта, а если и его нет — скачиваем эталонный Tauri v2 app-icon.png.

const fs = require('fs');
const path = require('path');
const { download } = require('./download.cjs');
const { run } = require('./run.cjs');

const DEFAULT_ICON_URL =
    'https://raw.githubusercontent.com/tauri-apps/tauri/v2/tooling/cli/templates/app/app-icon.png';

const PNG_MAGIC = 0x89504e47;

function isValidPng(buf) {
    return buf && buf.length > 16 && buf.readUInt32BE(0) === PNG_MAGIC;
}

function isValidIco(buf) {
    return buf && buf.length >= 6 && buf.readUInt16LE(0) === 0 && buf.readUInt16LE(2) === 1 && buf.readUInt16LE(4) > 0;
}

function iconsNeedRegen(cfg) {
    const ico = path.join(cfg.projectRoot, 'src-tauri', 'icons', 'icon.ico');
    if (!fs.existsSync(ico)) return true;
    try {
        return !isValidIco(fs.readFileSync(ico));
    } catch (e) {
        return true;
    }
}

async function ensureIcons(cfg) {
    if (!iconsNeedRegen(cfg)) {
        console.log('Icons OK (src-tauri/icons/icon.ico)');
        return;
    }

    let appIcon = path.join(cfg.projectRoot, 'app-icon.png');
    if (!fs.existsSync(appIcon) || !isValidPng(fs.readFileSync(appIcon))) {
        appIcon = path.join(cfg.projectRoot, 'src-tauri', 'icons', 'app-icon.png');
        if (!fs.existsSync(appIcon) || !isValidPng(fs.readFileSync(appIcon))) {
            console.log('Downloading default Tauri app-icon.png...');
            await download(DEFAULT_ICON_URL, appIcon);
        }
    }

    console.log('Regenerating icons via `npx tauri icon`...');
    run(cfg, 'npx', ['tauri', 'icon', appIcon], { echo: true, cwd: cfg.projectRoot });
}

module.exports = { ensureIcons, isValidIco, isValidPng };