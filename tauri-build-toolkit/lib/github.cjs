// ВСЕ git/gh операции пакета проходят строго через этот модуль.
// Здесь живут гарды безопасности:
//   1) origin проекта обязан совпадать с config.repo (иначе никаких git/gh операций);
//   2) gh release create всегда с явным --repo owner/repo;
//   3) ветка push — автодетект (git branch --show-current), без хардкода.
//
// git `-C <projectRoot>` исполняется из config.cjs; `tryRun` возвращает null
// вместо исключения (состояние читается безопасно).

const { run, tryRun } = require('./run.cjs');
const { normalizeRepo } = require('./config.cjs');

function getRemoteOrigin(cfg) {
    return tryRun(cfg, 'git', ['-C', cfg.projectRoot, 'remote', 'get-url', 'origin']);
}

function getBranch(cfg) {
    return tryRun(cfg, 'git', ['-C', cfg.projectRoot, 'branch', '--show-current']) || cfg.branch || 'main';
}

// ГЛАВНЫЙ ГАРД. Вызывается ПЕРЕД любой git push / gh release create.
// Проект обязан указывать на свой конфигурированный репозиторий. Если origin —
// чужой репозиторий, релиз может улететь не туда — падаем.
function checkRepoMatches(cfg) {
    if (!cfg.repo) {
        throw new Error('repo is not set (config.repo or autodetect). Cannot verify git safety.');
    }
    const origin = normalizeRepo(getRemoteOrigin(cfg));
    if (!origin) {
        throw new Error('Cannot read git remote origin for the project.');
    }
    if (origin !== normalizeRepo(cfg.repo)) {
        throw new Error(
            `Git safety guard failed.\n` +
            `  project origin : ${origin}\n` +
            `  config.repo    : ${cfg.repo}\n` +
            `Operations against a non-matching repository are forbidden.`
        );
    }
    console.log('Repo guard OK:', origin);
    return origin;
}

function checkGhAuth(cfg) {
    run(cfg, 'gh', ['auth', 'status'], { echo: true });
}

// gh release create всегда с явным --repo (гард №2).
function ghReleaseCreate(cfg, { tag, title, notes, assets }) {
    const args = ['release', 'create', tag, '--repo', cfg.repo, '--title', title, '--notes', notes, ...assets];
    run(cfg, 'gh', args, { echo: true });
}

function gitAdd(cfg, paths) {
    for (const p of paths) run(cfg, 'git', ['add', '--', p]);
}

function gitCommit(cfg, message) {
    if (!message) return;
    run(cfg, 'git', ['commit', '-m', message], { echo: true });
}

function gitPush(cfg, branch) {
    const b = branch || getBranch(cfg);
    run(cfg, 'git', ['push', 'origin', b], { echo: true });
}

function gitFetchTagsForce(cfg) {
    run(cfg, 'git', ['fetch', '--tags', '--force', 'origin'], { echo: true });
}

// Единый сценарий: add -> commit -> push по автодетектной ветке.
// Перед вызовом обязан быть выполнен checkRepoMatches.
function commitAndPush(cfg, { files, message, branch }) {
    if (!files.length) return;
    gitAdd(cfg, files);
    gitCommit(cfg, message);
    gitPush(cfg, branch);
}

module.exports = {
    getRemoteOrigin,
    getBranch,
    checkRepoMatches,
    checkGhAuth,
    ghReleaseCreate,
    gitAdd,
    gitCommit,
    gitPush,
    gitFetchTagsForce,
    commitAndPush,
};