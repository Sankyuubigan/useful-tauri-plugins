// node --test lib — unit-тесты state-отчёта релиза (docs §2.6.1).
// Никакой реальной сети/git — чистая строка.

const { test } = require('node:test');
const assert = require('node:assert/strict');
const { buildReleaseStateReport } = require('./release-state.cjs');

const FAIL = { published: false, latestJson: false, committed: false, pushed: false, fetchTags: false };

test('отчёт: релиз опубликован, пуш не прошёл — Recovery = git push', () => {
    const text = buildReleaseStateReport(
        { published: true, latestJson: true, committed: true, pushed: false, fetchTags: false },
        {
            tag: 'v26.10.9',
            version: '26.10.9',
            branch: 'main',
            error: new Error('Failed to connect to github.com port 443 after 21106 ms'),
        }
    );
    assert.match(text, /RELEASE STATE \(INCOMPLETE\)/);
    assert.match(text, /Published GitHub release\s+yes/);
    assert.match(text, /latest\.json written\s+yes/);
    assert.match(text, /Version commit \(local\)\s+yes/);
    assert.match(text, /Push to origin\/main\s+no/);
    assert.match(text, /Error: Failed to connect to github\.com port 443 after 21106 ms/);
    assert.match(text, /git push origin main/);
    assert.match(text, /not see 26\.10\.9 until main is pushed/);
    assert.doesNotMatch(text, /retry is safe/);
});

test('отчёт: публикация не состоялась — повтор release.bat безопасен, пуш не предлагается', () => {
    const text = buildReleaseStateReport(
        { ...FAIL },
        { tag: 'v26.10.10', version: '26.10.10', branch: 'main' }
    );
    assert.match(text, /Published GitHub release\s+no/);
    assert.match(text, /retry is safe/);
    assert.doesNotMatch(text, /git push origin main/);
    assert.doesNotMatch(text, /RELEASE STATE \(COMPLETE\)/);
});

test('отчёт: успешный путь — все шаги yes, Recovery нет', () => {
    const text = buildReleaseStateReport(
        { published: true, latestJson: true, committed: true, pushed: true, fetchTags: true },
        { tag: 'v26.10.10', version: '26.10.10', branch: 'main' }
    );
    assert.match(text, /RELEASE STATE \(COMPLETE\)/);
    assert.match(text, /Fetch tags \(local\)\s+yes/);
    assert.doesNotMatch(text, /Recovery:/);
});

test('отчёт: push прошёл, fetch не прошёл — Recovery = git fetch', () => {
    const text = buildReleaseStateReport(
        { published: true, latestJson: true, committed: true, pushed: true, fetchTags: false },
        { tag: 'v26.10.10', version: '26.10.10', branch: 'main', error: new Error('network flake') }
    );
    assert.match(text, /Push to origin\/main\s+yes/);
    assert.match(text, /git fetch --tags --force origin/);
    assert.doesNotMatch(text, /git push origin main/);
});
