// node --test lib — unit-тесты withNetworkRetry и partial-state гарда (docs §2.6.1/§2.6.2).
// Чистые тесты: execFn/probes внедряются, реальные git/gh не вызываются.

const fs = require('fs');
const path = require('path');
const { test } = require('node:test');
const assert = require('node:assert/strict');
const { withNetworkRetry, assertNotAlreadyPartiallyPublished } = require('./github.cjs');

test('withNetworkRetry: успех на второй попытке, всего 2 вызова', async () => {
    let calls = 0;
    const res = await withNetworkRetry(
        async (attempt, attempts) => {
            calls++;
            assert.strictEqual(attempts, 3);
            if (attempt < 2) throw new Error(`connect fail ${attempt}`);
            return 'ok';
        },
        { attempts: 3, delaysMs: [0, 0, 0] }
    );
    assert.strictEqual(res, 'ok');
    assert.strictEqual(calls, 2);
});

test('withNetworkRetry: все попытки упали — бросает исходную причину (fail-fast)', async () => {
    let calls = 0;
    await assert.rejects(
        withNetworkRetry(
            async () => {
                calls++;
                throw new Error('Failed to connect to github.com port 443 after 21106 ms');
            },
            { attempts: 3, delaysMs: [0, 0, 0] }
        ),
        /port 443 after 21106 ms/
    );
    assert.strictEqual(calls, 3);
});

test('withNetworkRetry: attempts=1 — одна попытка, без ожиданий', async () => {
    let calls = 0;
    await assert.rejects(
        withNetworkRetry(
            async () => {
                calls++;
                throw new Error('boom-1');
            },
            { attempts: 1, delaysMs: [15000] }
        ),
        /boom-1/
    );
    assert.strictEqual(calls, 1);
});

test('withNetworkRetry: причина последней попытки не теряется при чередовании ошибок', async () => {
    let n = 0;
    await assert.rejects(
        withNetworkRetry(
            async () => {
                n++;
                throw new Error(`err-${n}`);
            },
            { attempts: 2, delaysMs: [0, 0] }
        ),
        /err-2/
    );
});

// ── assertNotAlreadyPartiallyPublished: сценарий инцидента 20.10.26 ──
// Состав зафиксирован по живым фактам инцидента (все три проверены на реальном
// репо до пуша): тег v26.10.9 есть на GitHub с двумя ассетами, ветка опережает
// origin на 1 коммит, HEAD = release-коммит этой версии. Зонды внедряются —
// реальные git/gh в тестах не дергаются; живое подтверждение одиночных входов —
// см. tasks/20.10.26 (G3 one-liner).

function makeIncidentCfg() {
    const root = path.join(__dirname, '..', 'temp', 'partial-state-fixture');
    fs.rmSync(root, { recursive: true, force: true });
    fs.mkdirSync(path.join(root, 'src-tauri'), { recursive: true });
    fs.writeFileSync(
        path.join(root, 'src-tauri', 'tauri.conf.json'),
        JSON.stringify({ version: '26.10.9' }, null, 2),
        'utf8'
    );
    return { projectRoot: root, repo: 'fake-owner/fake-repo' };
}

function rmFixture(cfg) {
    fs.rmSync(cfg.projectRoot, { recursive: true, force: true });
}

const INCIDENT_PROBES = {
    tagExists: () => ({
        exists: true,
        assets: ['King.Orch_26.10.9_x64-setup.exe', 'King.Orch_26.10.9_x64-setup.exe.sig'],
    }),
    branchAhead: () => 1,
    headSubject: () => 'chore(release): v26.10.9',
};

test('гард: незапушенный release-коммит при существующем теге — бросает Recovery', () => {
    const cfg = makeIncidentCfg();
    try {
        assert.throws(
            () => assertNotAlreadyPartiallyPublished(cfg, INCIDENT_PROBES),
            (e) => {
                assert.match(e.message, /\[PARTIAL STATE\] Release v26\.10\.9 is already published/);
                assert.match(e.message, /x64-setup\.exe\.sig/);
                assert.match(e.message, /Recovery: git push origin main/);
                assert.match(e.message, /Do NOT re-run release\.bat/);
                return true;
            }
        );
    } finally {
        rmFixture(cfg);
    }
});

test('гард: та же ситуация после пуша (ahead=0) — НЕ бросает', () => {
    const cfg = makeIncidentCfg();
    try {
        const ok = assertNotAlreadyPartiallyPublished(cfg, {
            ...INCIDENT_PROBES,
            branchAhead: () => 0,
        });
        assert.strictEqual(ok, undefined);
    } finally {
        rmFixture(cfg);
    }
});

test('гард: тега нет (обычный путь нового релиза) — НЕ бросает', () => {
    const cfg = makeIncidentCfg();
    try {
        assertNotAlreadyPartiallyPublished(cfg, {
            ...INCIDENT_PROBES,
            tagExists: () => ({ exists: false, assets: [] }),
        });
    } finally {
        rmFixture(cfg);
    }
});

test('гард: тег есть, но HEAD не release-коммит — НЕ бросает (следующий релиз)', () => {
    const cfg = makeIncidentCfg();
    try {
        assertNotAlreadyPartiallyPublished(cfg, {
            ...INCIDENT_PROBES,
            headSubject: () => 'рабочий коммит',
        });
    } finally {
        rmFixture(cfg);
    }
});
