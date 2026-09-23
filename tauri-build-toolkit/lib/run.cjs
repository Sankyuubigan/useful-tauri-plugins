// Единые обёртки выполнения команд. ВСЕ spawn/execSync пакета идут только сюда
// и только с `cwd` = projectRoot (правило: операции не должны «утекать» в папку
// плагина или чужой проект).

const { execSync, spawn } = require('child_process');

function quoteArg(a) {
    const s = String(a);
    if (/[ "&|<>^()%@!]/.test(s)) return `"${s.replace(/"/g, '\\"')}"`;
    return s;
}

function buildCmd(cmd, args) {
    const c = Array.isArray(cmd) ? cmd.join(' ') : String(cmd);
    return args && args.length ? `${c} ${args.map(quoteArg).join(' ')}` : c;
}

// Выполнить команду. echo=true печатает команду. capture=true возвращает вывод.
function run(cfg, cmd, args, opts = {}) {
    const { echo = false, capture = false } = opts;
    const full = buildCmd(cmd, args);
    if (echo) console.log('$', full);
    const base = {
        cwd: opts.cwd || (cfg ? cfg.projectRoot : process.cwd()),
        ...(opts.env ? { env: opts.env } : {}),
    };
    const res = execSync(full, capture
        ? { ...base, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }
        : { ...base, stdio: 'inherit' });
    return capture ? String(res).trim() : undefined;
}

// Выполнить и вернуть вывод, либо null при ошибке (для чтения git-состояния).
function tryRun(cfg, cmd, args, opts = {}) {
    try {
        return run(cfg, cmd, args, { ...opts, capture: true });
    } catch (e) {
        return null;
    }
}

// Запуск приложения отвязанным процессом (build.bat не блокирует скрипт).
// После spawn ждём settleMs: если процесс уже умер (PANIC на старте,
// missing DLL) — честно фиксируем фейл, а не печатаем безусловный "Launched:".
// Async: event loop должен крутиться, иначе exit/error не сработают.
function launchApp(cfg, exePath, opts = {}) {
    if (!exePath) return Promise.resolve(false);
    const settleMs = Number(opts.settleMs) > 0 ? Number(opts.settleMs) : 2000;
    const p = spawn(exePath, [], { cwd: cfg.projectRoot, detached: true, stdio: 'ignore' });
    return new Promise((resolve) => {
        let settled = false;
        const finish = (ok) => {
            if (settled) return;
            settled = true;
            clearTimeout(timer);
            resolve(ok);
        };
        const timer = setTimeout(() => {
            // Процесс пережил settle-окно → успех.
            p.unref();
            console.log('Launched:', exePath);
            finish(true);
        }, settleMs);
        p.on('error', (err) => {
            console.error('Launch failed:', exePath, err.message);
            process.exitCode = 1;
            finish(false);
        });
        p.on('exit', (code) => {
            console.error(`App exited immediately (code=${code}):`, exePath);
            process.exitCode = 1;
            finish(false);
        });
    });
}

// Закрытие запущенного инстанса приложения (высвобождает file lock exe).
function killApp(cfg) {
    if (!cfg.appExe) return;
    try {
        run(cfg, 'taskkill', ['/F', '/IM', `${cfg.appExe}.exe`, '/T'], { echo: false });
    } catch (e) {
        /* процесс не был запущен — не ошибка */
    }
}

module.exports = { run, tryRun, launchApp, killApp, buildCmd };