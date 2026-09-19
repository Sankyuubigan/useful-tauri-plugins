// doctor: read-only диагностика резолва конфигурации + git-безопасность.
// Ничего не пишет и не запускает сборок.

const fs = require('fs');
const path = require('path');
const { getRemoteOrigin, checkRepoMatches } = require('./github.cjs');
const { resolveKeyPath } = require('./signing.cjs');

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

    console.log('\n--- git safety ---');
    console.log('latest.json     :', cfg.latestJsonPath, fs.existsSync(cfg.latestJsonPath) ? '(exists)' : '(not yet)');
    try {
        checkRepoMatches(cfg);
    } catch (e) {
        console.log('repo guard      : FAIL ->', e.message);
        process.exitCode = 1;
        return;
    }
    console.log('\n[doctor] OK.');
}

module.exports = { run };