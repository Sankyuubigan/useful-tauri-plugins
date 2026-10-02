// init: скопировать .bat-шаблоны в корень проекта + создать .build-config.json
// из примера (если ещё нет). Шаблоны разворачиваются с CRLF (окончания Windows).
// Опция --with-logs: вложить LOGS_SETUP.md (гайд подключения tauri-plugin-logs).

const fs = require('fs');
const path = require('path');
const { CONFIG_FILENAME } = require('./config.cjs');

const TOOLKIT_ROOT = path.resolve(__dirname, '..');
const TEMPLATES_DIR = path.join(TOOLKIT_ROOT, 'templates');

const BAT_TEMPLATES = ['build.bat', 'generate_installer.bat', 'release.bat', 'test.bat'];
const LOGS_GUIDE_NAME = 'LOGS_SETUP.md';

function asCrlf(text) {
    return text.replace(/\r?\n/g, '\r\n');
}

// cmd не умеет выполнять строку, начинающуюся с UTF-8 BOM: `@echo off` не
// срабатывает, весь поток команд печатается в лог, а `exit /b` после ошибки
// ведёт себя непредсказуемо. Плюс .bat по правилам проекта — только ASCII и
// CRLF (desktop_rust_tauri/rules.md §2). Поэтому перед записью снимаем BOM и
// приводим переводы строк к CRLF.
function sanitizeBat(text) {
    const withoutBom = text.charCodeAt(0) === 0xfeff ? text.slice(1) : text;
    return asCrlf(withoutBom);
}

function run(cfg, opts = {}) {
    const root = cfg.projectRoot;
    console.log('=== init ===');
    console.log('Installing into:', root);

    for (const name of BAT_TEMPLATES) {
        const src = path.join(TEMPLATES_DIR, name);
        const dest = path.join(root, name);
        if (fs.existsSync(dest)) {
            console.log('exists, skip        :', name);
            continue;
        }
        if (!fs.existsSync(src)) {
            console.warn('template not found  :', src);
            continue;
        }
        const text = sanitizeBat(fs.readFileSync(src, 'utf8'));
        if (text.charCodeAt(0) === 0xfeff) {
            console.warn('BOM stripped        :', name);
        }
        fs.writeFileSync(dest, text, 'utf8');
        console.log('wrote               :', name);
    }

    const cfgPath = path.join(root, CONFIG_FILENAME);
    if (fs.existsSync(cfgPath)) {
        console.log('config exists       :', cfgPath);
    } else {
        // Имя шаблона — `.build-config.example.json` (CONFIG_FILENAME без
        // расширения + `.example.json`), а НЕ `<CONFIG_FILENAME>.example.json`:
        // иначе поиск не находил шаблон и конфиг молча не создавался.
        const example = path.join(
            TEMPLATES_DIR,
            `${CONFIG_FILENAME.replace(/\.json$/, '')}.example.json`
        );
        if (!fs.existsSync(example)) {
            console.warn(`template not found  : ${example} — создай ${CONFIG_FILENAME} вручную`);
            return false;
        }
        fs.writeFileSync(cfgPath, fs.readFileSync(example, 'utf8'), 'utf8');
        console.log('wrote               :', cfgPath, '(edit it!)');
    }

    const withLogs = opts.withLogs || opts['with-logs'];

    if (withLogs) {
        const guideSrc = path.join(TEMPLATES_DIR, LOGS_GUIDE_NAME);
        const guideDest = path.join(root, LOGS_GUIDE_NAME);
        if (!fs.existsSync(guideSrc)) {
            console.warn('template not found  :', guideSrc);
        } else if (fs.existsSync(guideDest)) {
            console.log('exists, skip        :', guideDest);
        } else {
            fs.writeFileSync(guideDest, asCrlf(fs.readFileSync(guideSrc, 'utf8')), 'utf8');
            console.log('wrote               :', guideDest, '(гайд tauri-plugin-logs)');
        }
    }

    if (withLogs) {
        console.log('\n[logs] Инструкция подключения: ' + LOGS_GUIDE_NAME);
    } else {
        console.log('\n(подсказка: `init --with-logs` вложит гайд подключения tauri-plugin-logs)');
    }
    console.log('\nNext: edit .build-config.json, then run build.bat.');
    return true;
}

module.exports = { run };