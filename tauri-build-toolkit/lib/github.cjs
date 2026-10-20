// ВСЕ git/gh операции пакета проходят строго через этот модуль.
// Здесь живут гарды безопасности:
//   1) origin проекта обязан совпадать с config.repo (иначе никаких git/gh операций);
//   2) gh release create всегда с явным --repo owner/repo;
//   3) ветка push — автодетект (git branch --show-current), без хардкода;
//   4) файлы релиза (latest.json, версии) не должны быть игнорированы .gitignore;
//   5) сетевые git-операции имеют ретрай + partial-state гард (§2.6.1/§2.6.2).

const path = require('path');
const { spawnSync } = require('child_process');
const { run, tryRun } = require('./run.cjs');
const { normalizeRepo } = require('./config.cjs');
const { readVersion } = require('./version.cjs');

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

// Файлы, которые release обязан закоммитить: latest.json — манифест апдейтера
// (его читает клиент по raw.githubusercontent), версии — источник правды сборки.
function releaseCommittedFiles(cfg) {
    return [
        cfg.latestJsonPath,
        path.join(cfg.projectRoot, 'src-tauri', 'tauri.conf.json'),
        path.join(cfg.projectRoot, 'src-tauri', 'Cargo.toml'),
    ];
}

// `git check-ignore` сам возвращает признак через код выхода (0 = игнорируется),
// поэтому читаем состояние через spawnSync, а НЕ через tryRun (он бы проглотил код).
function isIgnored(cfg, filePath) {
    const res = spawnSync('git', ['-C', cfg.projectRoot, 'check-ignore', '-q', '--', filePath], {
        stdio: 'ignore',
        windowsHide: true,
    });
    if (res.error) return false;
    return res.status === 0;
}

// ГАРД №3. Проверяется ДО сборки и публикации: `git add` игнорируемого файла
// падает уже после `gh release create`, и релиз остаётся опубликованным без
// latest.json в репозитории (апдейтер клиентов не видит новую версию).
function checkCommittedFilesNotIgnored(cfg) {
    const ignored = releaseCommittedFiles(cfg).filter((f) => isIgnored(cfg, f));
    if (ignored.length) {
        throw new Error(
            `Git guard failed: файлы релиза игнорируются .gitignore:\n` +
            ignored.map((f) => `  ${f}`).join('\n') +
            `\nУберите правило из .gitignore (latest.json должен попадать в репозиторий:\n` +
            `его читает апдейтер по raw.githubusercontent.com/<repo>/<branch>/latest.json).`
        );
    }
    console.log('Ignored-files guard OK');
}

function gitCommit(cfg, message) {
    if (!message) return;
    run(cfg, 'git', ['commit', '-m', message], { echo: true });
}

// ── §2.6.1: ретрай сетевых git-операций ──
// Сетевой сбой (github.com:443 недоступен ~20 с) на шаге push ПОСЛЕ публикации
// релиза = «частично опубликованный» релиз: ассеты есть, latest.json в репо нет,
// апдейтер клиентов не видит версию (инцидент 20.10.26, King Orch v26.10.9).
// Ретрай — для сети; fail-fast после его исчерпания не отменяется (§2.6).
function sleepMs(ms) {
    return new Promise((resolve) => setTimeout(resolve, ms));
}

// execFn: async (attempt, attempts) => result; бросает при неудаче.
// opts.attempts (default 3), opts.delaysMs (default [15000, 45000, 90000]).
// Возвращает результат пейattempt'а; последняя неудача бросает ИСХОДНУЮ ошибку —
// глотать или подменять причину запрещено.
async function withNetworkRetry(execFn, opts = {}) {
    const attempts = opts.attempts > 0 ? opts.attempts : 3;
    const delaysMs = Array.isArray(opts.delaysMs) ? opts.delaysMs : [15000, 45000, 90000];
    let lastErr;
    for (let attempt = 1; attempt <= attempts; attempt++) {
        try {
            return await execFn(attempt, attempts);
        } catch (e) {
            lastErr = e;
            const delay = delaysMs[attempt - 1] > 0 ? delaysMs[attempt - 1] : 0;
            if (attempt < attempts && delay > 0) {
                console.log(
                    `[retry] attempt ${attempt}/${attempts} failed (${e.message}); ` +
                    `retrying in ${Math.round(delay / 1000)}s`
                );
                await sleepMs(delay);
            }
        }
    }
    throw lastErr;
}

const PUSH_RETRY = { attempts: 3, delaysMs: [15000, 45000, 90000] };
const FETCH_TAGS_RETRY = { attempts: 2, delaysMs: [15000, 45000] };

async function gitPush(cfg, branch, opts = {}) {
    const b = branch || getBranch(cfg);
    return withNetworkRetry(async (attempt, attempts) => {
        console.log(`$ git push origin ${b} (attempt ${attempt}/${attempts})`);
        run(cfg, 'git', ['push', 'origin', b], { echo: false });
        return true;
    }, opts.retry || PUSH_RETRY);
}

async function gitFetchTagsForce(cfg) {
    return withNetworkRetry(async (attempt, attempts) => {
        console.log(`$ git fetch --tags --force origin (attempt ${attempt}/${attempts})`);
        run(cfg, 'git', ['fetch', '--tags', '--force', 'origin'], { echo: false });
        return true;
    }, FETCH_TAGS_RETRY);
}

// Единый сценарий: add -> commit -> push по автодетектной ветке.
// Перед вызовом обязан быть выполнен checkRepoMatches.
// Возвращает { committed, pushed }. Провал ПУША бросает ошибку с .steps
// { committed: true, pushed: false } — release.cjs обязан этими флагами
// достроить state-отчёт (lib/release-state.cjs), а не потерять этап.
async function commitAndPush(cfg, { files, message, branch, pushRetry }) {
    if (!files.length) return { committed: false, pushed: false };
    gitAdd(cfg, files);
    gitCommit(cfg, message);
    const committed = true;
    try {
        await gitPush(cfg, branch, { retry: pushRetry });
        return { committed, pushed: true };
    } catch (e) {
        e.steps = { committed, pushed: false };
        throw e;
    }
}

// ── §2.6.2: pre-check частично опубликованного релиза (ДО сборки) ──

// Тег существует? { exists, assets }. «Тега нет» и сетевая ошибка gh — РАЗНЫЕ
// исходы: сетевую ошибку бросаем (выдавать её за «не существует» — ложь).
function checkTagExists(cfg, tag) {
    const res = spawnSync(
        'gh',
        ['release', 'view', tag, '--repo', cfg.repo, '--json', 'tagName,assets'],
        { encoding: 'utf8', cwd: cfg.projectRoot, windowsHide: true }
    );
    if (res.status === 0) {
        let assets = [];
        try {
            const parsed = JSON.parse(res.stdout || '{}');
            assets = (parsed.assets || []).map((a) => a.name);
        } catch (e) {
            // stdout не JSON — главное (exists) уже подтверждено нулевым кодом.
            console.warn(`checkTagExists: cannot parse gh output: ${e.message}`);
        }
        return { exists: true, assets };
    }
    const errText = `${res.stderr || ''}\n${res.stdout || ''}`;
    if (/release\s+not\s+found|could not resolve to a release/i.test(errText)) {
        return { exists: false, assets: [] };
    }
    throw new Error(
        `gh release view ${tag} failed; cannot distinguish missing tag from an error:\n` +
        errText.trim()
    );
}

// Сколько коммитов ветка опережает origin/<branch> (null — если неизвестно).
function getBranchAhead(cfg, branch) {
    const out = tryRun(cfg, 'git', ['-C', cfg.projectRoot, 'rev-list', '--count', `origin/${branch}..${branch}`]);
    if (out == null) return null;
    const n = parseInt(String(out).trim(), 10);
    return Number.isFinite(n) ? n : null;
}

function getHeadCommitSubject(cfg) {
    return tryRun(cfg, 'git', ['-C', cfg.projectRoot, 'log', '-1', '--pretty=%s']);
}

// ГАРД частично опубликованного релиза (инцидент 20.10.26).
// Частичное состояние: тег v<_version из tauri.conf.json, ДО бампа> существует на GitHub,
// а локальная ветка опережает origin ровно незапушенным release-коммитом этой версии.
// Тогда: бросаем с инструкцией восстановления — ДО prep(), т.е. ДО траты минут
// на пересборку и ДО ложного бампа версии (prep() инкрементирует версию ->
// повторный запуск незаметно уехал бы на v26.10.10 и оставил v26.10.9 сиротой).
// «Тег существует» сам по себе НЕ признак сбоя: если release-коммит запушен —
// это обычный предыдущий релиз, работаем дальше как новый (следующий тег).
// Автоудаление/пересоздание существующего тега запрещено (§2.6.2).
//
// probes — точки внедрения для node:test (по умолчанию реальные gh/git):
//   tagExists(tag) -> { exists, assets[] }, branchAhead() -> int|null,
//   headSubject() -> string.
function assertNotAlreadyPartiallyPublished(cfg, probes = {}) {
    const tagExists = probes.tagExists || ((t) => checkTagExists(cfg, t));
    const branchAhead = probes.branchAhead || (() => getBranchAhead(cfg, getBranch(cfg)));
    const headSubject = probes.headSubject || (() => String(getHeadCommitSubject(cfg) || '').trim());

    const version = readVersion(cfg.projectRoot);
    const tag = `v${version}`;
    const info = tagExists(tag);
    if (!info.exists) return;
    const ahead = branchAhead();
    const subject = headSubject();
    const headIsReleaseCommit = new RegExp(`^chore\\(release\\): ${tag}$`).test(subject);
    if (!(ahead > 0 && headIsReleaseCommit)) return;

    const branch = getBranch(cfg);
    throw new Error(
        `\n[PARTIAL STATE] Release ${tag} is already published on GitHub (partially).\n` +
        `  GitHub tag  : ${tag} exists (assets: ${info.assets.join(', ') || 'none'})\n` +
        `  Local branch: ${ahead} commit(s) ahead of the remote\n` +
        `  HEAD        : "${subject}"\n` +
        `The updater manifest (latest.json) is already committed locally but NOT pushed;\n` +
        `clients will not see ${version} until the push reaches the remote.\n\n` +
        `Recovery: git push origin main\n` +
        `(Do NOT re-run release.bat for this version and do NOT delete the tag.)\n`
    );
}

module.exports = {
    getRemoteOrigin,
    getBranch,
    checkRepoMatches,
    checkGhAuth,
    isIgnored,
    checkCommittedFilesNotIgnored,
    releaseCommittedFiles,
    ghReleaseCreate,
    gitAdd,
    gitCommit,
    gitPush,
    gitFetchTagsForce,
    commitAndPush,
    withNetworkRetry,
    checkTagExists,
    getBranchAhead,
    getHeadCommitSubject,
    assertNotAlreadyPartiallyPublished,
};