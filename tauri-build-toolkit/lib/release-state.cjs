// State-отчёт релиза (docs/build_release_rules.md §2.6.1).
// ЕДИНАЯ форма для success- и fail-пути release.cjs: какие шаги выполнены,
// какие нет, и чем восстанавливать. Правдивость §core 2.2: не печатать «успех»
// при недоделанных шагах, показывать точную причину и точную команду Recovery.
//
// Печатает в консоль и ВОЗВРАЩАЕТ текст (возврат — чтобы node:test проверял
// строки без перехвата stdout).

// 'yes'/'no' везде; итоговый заголовок INCOMPLETE/COMPLETE говорит правду целиком.
function buildReleaseStateReport(state, opts = {}) {
    const branch = opts.branch || '<unknown>';
    const tag = opts.tag || '?';
    const version = opts.version || String(tag).replace(/^v/, '');

    const rows = [
        ['Tag', tag],
        ['Published GitHub release', state.published ? 'yes' : 'no'],
        ['latest.json written', state.latestJson ? 'yes' : 'no'],
        ['Version commit (local)', state.committed ? 'yes' : 'no'],
        [`Push to origin/${branch}`, state.pushed ? 'yes' : 'no'],
        ['Fetch tags (local)', state.fetchTags ? 'yes' : 'no'],
    ];

    const complete =
        !!state.published && !!state.latestJson && !!state.committed &&
        !!state.pushed && !!state.fetchTags;

    const w1 = Math.max(...rows.map(([k]) => k.length));
    const bar = '='.repeat(Math.max(40, w1 + 3 + 8));
    const lines = [];

    lines.push('');
    lines.push(bar);
    lines.push(`  RELEASE STATE (${complete ? 'COMPLETE' : 'INCOMPLETE'})`);
    lines.push(bar);
    if (opts.error) {
        const msg = opts.error && opts.error.message ? opts.error.message : String(opts.error);
        const errLines = String(msg).split(/\r?\n/);
        errLines.forEach((l, i) => lines.push(`  ${i === 0 ? 'Error' : '     '}: ${l}`));
        lines.push('');
    }
    for (const [k, v] of rows) {
        lines.push(`  ${k.padEnd(w1)}  ${v}`);
    }

    const recovery = [];
    if (!state.published) {
        recovery.push('Release tag was NOT created. Re-run release.bat once the network is back - retry is safe.');
    } else {
        if (!state.pushed) {
            recovery.push(`git push origin ${branch}`);
            recovery.push(
                `Updater clients will not see ${version} until ${branch} is pushed ` +
                `(raw/<repo>/${branch}/latest.json is still stale).`
            );
        }
        if (state.pushed && !state.fetchTags) {
            recovery.push('git fetch --tags --force origin');
        }
    }
    if (recovery.length) {
        lines.push('');
        lines.push('  Recovery:');
        for (const r of recovery) lines.push(`    ${r}`);
    }

    lines.push(bar);
    return lines.join('\n');
}

function printReleaseState(state, opts = {}) {
    const text = buildReleaseStateReport(state, opts);
    console.log(text);
    return text;
}

module.exports = { buildReleaseStateReport, printReleaseState };
