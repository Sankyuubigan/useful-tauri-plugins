// doctor: read-only диагностика резолва конфигурации + git-безопасность.
// Ничего не пишет и не запускает сборок.

const fs = require('fs');
const path = require('path');
const { getRemoteOrigin, checkRepoMatches, checkCommittedFilesNotIgnored } = require('./github.cjs');
const { resolveKeyPath } = require('./signing.cjs');
const { checkProjectEncodings } = require('./encoding-check.cjs');

function run(cfg) {
    const version = cfg.configRaw && cfg.configRaw._version ? cfg.configRaw._version : null;

    console.log('=== tauri-build-toolkit doctor ===');
    console.log('node            :', process.version, `(${process.platform} ${process.arch})`);
    console.log('toolkit root    :', path.resolve(__dirname, '..'));
    console.log('projectRoot     :', cfg.projectRoot);
    console.log('config file     :', cfg.configPath, fs.existsSync(cfg.configPath) ? '' : '(not found)');
    console.log('appExe          :', cfg.appExe);
    console.log('productName     :', cfg.productName);
    console.log('repo (config)   :', cfg.repo);
    console.log('branch          :', cfg.branch);
    console.log('git origin      :', getRemoteOrigin(cfg) || '(none)');
    console.log('syncDirs        :', cfg.syncDirs.join(', ') || '(none)');
    console.log('devWindow       :', cfg.devWindow ? JSON.stringify(cfg.devWindow) : '(none)');
    console.log('vcRedist        :', cfg.vcRedist);
    console.log('npmInstallArgs  :', cfg.npmInstallArgs.join(' '));
    console.log('syncPkgVersion  :', cfg.syncPackageJsonVersion);

    const keyPath = resolveKeyPath(cfg);
    console.log('signing key     :', keyPath, fs.existsSync(keyPath) ? '(present)' : '(MISSING)');

    console.log('\n--- encoding ---');
    let encodingOk = true;
    try {
        checkProjectEncodings(cfg);
        console.log('encoding check  : OK');
    } catch (e) {
        console.log('encoding check  : FAIL (см. отчёт выше)');
        encodingOk = false;
        process.exitCode = 1;
    }

    console.log('\n--- git safety ---');
    console.log('latest.json     :', cfg.latestJsonPath, fs.existsSync(cfg.latestJsonPath) ? '(exists)' : '(not yet)');
    let repoOk = true;
    try {
        checkRepoMatches(cfg);
        console.log('repo guard      : OK');
    } catch (e) {
        console.log('repo guard      : FAIL ->', e.message);
        repoOk = false;
        process.exitCode = 1;
    }

    console.log('\n--- gitignore guard ---');
    try {
        checkCommittedFilesNotIgnored(cfg);
        console.log('ignored guard   : OK');
    } catch (e) {
        console.log('ignored guard   : FAIL ->', e.message);
        repoOk = false;
        process.exitCode = 1;
    }

    if (encodingOk && repoOk) {
        console.log('\n[doctor] OK.');
    } else {
        console.log('\n[doctor] ЕСТЬ ПРОБЛЕМЫ (см. выше).');
    }
}

module.exports = { run };