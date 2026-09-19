// Загрузка конфигурации проекта (.build-config.json) + автодетект.
//
// Единая точка, где резолвится projectRoot. ВСЯ работа со сборкой/релизом
// выполняется строго относительно projectRoot — это исключает «утечку»
// операций в папку плагина или в чужой проект.

const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

const CONFIG_FILENAME = '.build-config.json';
const DEFAULT_SIGNING_KEYS_DIR =
    'D:\\Projects\\docusaurus-starter\\docs\\Sega Mega Note\\Моя картотека\\software\\настройки\\tauri_signed_keys';

function readJson(filePath) {
    return JSON.parse(fs.readFileSync(filePath, 'utf8'));
}

function hasTauriProject(root) {
    return fs.existsSync(path.join(root, 'src-tauri', 'tauri.conf.json'));
}

function resolveProjectRoot(projectArg, cwd) {
    if (projectArg && hasTauriProject(projectArg)) {
        return path.resolve(projectArg);
    }
    if (hasTauriProject(cwd)) {
        return path.resolve(cwd);
    }
    throw new Error(
        `Не найден корень Tauri-проекта (нужен src-tauri/tauri.conf.json).\n` +
        `  --project: ${projectArg}\n  cwd: ${cwd}\n` +
        `Запускай скрипт из корня проекта или передай --project "<корень>".`
    );
}

function readTauriConf(projectRoot) {
    return readJson(path.join(projectRoot, 'src-tauri', 'tauri.conf.json'));
}

function readCargoName(projectRoot) {
    const text = fs.readFileSync(path.join(projectRoot, 'src-tauri', 'Cargo.toml'), 'utf8');
    const articles = text.match(/^name\s*=\s*"([^"]+)"/m);
    return articles ? articles[1] : null;
}

// Репо можно вывести из tauri.conf.json (plugins.about-updates.repo или updater.endpoints).
function detectRepoFromTauri(tauriConf) {
    const plugins = tauriConf.plugins || {};
    if (plugins['about-updates'] && plugins['about-updates'].repo) {
        return plugins['about-updates'].repo;
    }
    if (plugins.updater && Array.isArray(plugins.updater.endpoints)) {
        for (const ep of plugins.updater.endpoints) {
            const m = String(ep).match(/github\.com\/([^/]+\/[^/]+)\/(?:main|master|head)\/latest\.json/);
            if (m) return m[1];
        }
    }
    return null;
}

function normalizeRepo(repo) {
    if (!repo) return null;
    return String(repo)
        .replace(/^https?:\/\/(www\.)?github\.com\//, '')
        .replace(/^git@github\.com:/, '')
        .replace(/\.git$/, '')
        .replace(/\/$/, '');
}

function gitCurrentBranch(projectRoot) {
    try {
        return execSync(`git -C "${projectRoot}" branch --show-current`, {
            encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'],
        }).trim();
    } catch (e) {
        return null;
    }
}

function gitRemoteOrigin(projectRoot) {
    try {
        return execSync(`git -C "${projectRoot}" remote get-url origin`, {
            encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'],
        }).trim();
    } catch (e) {
        return null;
    }
}

// Единый источник правды о проекте. cfgRaw — содержимое .build-config.json
// (только отличия от дефолта), всё остальное либо авто-детектится, либо берёт
// безопасный дефолт. Плагин НЕ содержит имён/репозиториев конкретных проектов.
function loadProjectConfig(projectArg, cwd) {
    const projectRoot = resolveProjectRoot(projectArg, cwd);

    let cfgRaw = {};
    const cfgPath = path.join(projectRoot, CONFIG_FILENAME);
    if (fs.existsSync(cfgPath)) {
        cfgRaw = readJson(cfgPath) || {};
    }

    const tauriConf = readTauriConf(projectRoot);
    const cargoName = readCargoName(projectRoot);
    const origin = gitRemoteOrigin(projectRoot);
    const fallbackRepo = normalizeRepo(cfgRaw.repo) || normalizeRepo(detectRepoFromTauri(tauriConf)) || normalizeRepo(origin);

    const signing = cfgRaw.signing && typeof cfgRaw.signing === 'object' ? cfgRaw.signing : {};

    return {
        projectRoot,
        configPath: cfgPath,
        configRaw: cfgRaw,

        appExe: cfgRaw.appExe || cargoName || null,
        productName: cfgRaw.productName || tauriConf.productName || cargoName || null,
        repo: fallbackRepo,
        branch: cfgRaw.branch || gitCurrentBranch(projectRoot) || 'main',

        npmInstallArgs: Array.isArray(cfgRaw.npmInstallArgs) ? cfgRaw.npmInstallArgs : ['--legacy-peer-deps'],
        syncDirs: Array.isArray(cfgRaw.syncDirs) ? cfgRaw.syncDirs : [],
        devWindow: cfgRaw.devWindow || null,

        vcRedist: cfgRaw.vcRedist === true,
        vcRedistDir: cfgRaw.vcRedistDir || 'redist',
        vcRedistDlls: Array.isArray(cfgRaw.vcRedistDlls) && cfgRaw.vcRedistDlls.length
            ? cfgRaw.vcRedistDlls
            : ['vcruntime140.dll', 'vcruntime140_1.dll', 'msvcp140.dll', 'msvcp140_1.dll', 'concrt140.dll'],

        signing: {
            keyPath: signing.keyPath || null,
            passwordFile: signing.passwordFile || null,
            password: signing.password || null,
        },
        defaultKeysDir: process.env.TAURI_KEYS_DIR || DEFAULT_SIGNING_KEYS_DIR,

        syncPackageJsonVersion: cfgRaw.syncPackageJsonVersion === true,
        releaseNotesTemplate: cfgRaw.releaseNotesTemplate || 'Auto release ${product} ${tag}',

        latestJsonPath: path.join(projectRoot, 'latest.json'),
    };
}

module.exports = {
    loadProjectConfig,
    resolveProjectRoot,
    readJson,
    readTauriConf,
    readCargoName,
    normalizeRepo,
    gitCurrentBranch,
    gitRemoteOrigin,
    CONFIG_FILENAME,
};