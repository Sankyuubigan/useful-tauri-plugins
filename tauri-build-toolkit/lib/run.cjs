// Единые обёртки выполнения команд. ВСЕ spawn/execSync пакета идут только сюда
// и только с `cwd` = projectRoot (правило: операции не должны «утекать» в папку
// плагина или чужой проект).

const { execSync, spawn } = require('child_process');
const fs = require('fs');
const path = require('path');

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
// quiet=true — подавить вывод дочернего процесса (напр. taskkill с cp866-мусором).
function run(cfg, cmd, args, opts = {}) {
    const { echo = false, capture = false, quiet = false } = opts;
    const full = buildCmd(cmd, args);
    if (echo) console.log('$', full);
    const base = {
        cwd: opts.cwd || (cfg ? cfg.projectRoot : process.cwd()),
        ...(opts.env ? { env: opts.env } : {}),
    };
    let res;
    if (quiet) res = execSync(full, { ...base, stdio: 'ignore' });
    else if (capture) res = execSync(full, { ...base, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
    else res = execSync(full, { ...base, stdio: 'inherit' });
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
// missing DLL) — честно фиксируем фейл, а не печатаем безусловно "Launched:".
// Async: event loop должен крутиться, иначе exit/error не сработают.
//
// Два важных решения (см. global_ai_docs/desktop_rust_tauri/rules.md §"cwd"):
//   1. cwd = папка exe (НЕ корень репо) — иначе приложение «насрёт» DLL
//      движка в исходниках.
//   2. WEBVIEW2_USER_DATA_FOLDER — отдельный профиль для dev-инстанса.
//      Иначе dev и установленная релизная копия делят один каталог
//      %LOCALAPPDATA%\<имя exe>\EBWebView → конфликт при параллельной работе.
function launchApp(cfg, exePath, opts = {}) {
    if (!exePath) return Promise.resolve(false);
    const settleMs = Number(opts.settleMs) > 0 ? Number(opts.settleMs) : 2000;
    const exeDir = path.dirname(exePath);
    const env = { ...process.env, ...(opts.env || {}) };

    // 2. WEBVIEW2_USER_DATA_FOLDER — отдельный профиль для dev-инстанса.
    //    Иначе dev и установленная релизная копия делят один каталог
    //    %LOCALAPPDATA%\<имя exe>\EBWebView → конфликт при параллельной работе.
    env.WEBVIEW2_USER_DATA_FOLDER = path.join(cfg.projectRoot, 'src-tauri', 'target', 'webview2-dev');

    // 3. APPDATA НЕ подменяем: app_data_dir() хоста резолвится через WinAPI
    //    (SHGetKnownFolderPath) и игнорирует env APPDATA, а плагины читают
    //    std::env::var_os("APPDATA") — подмена ломала плагины (инцидент
    //    «Движок изображений не установлен»). Dev и релиз делят один
    //    %APPDATA%\<app>\ — это осознанно: записи атомарны (ko-json-store).
    //    Stale target/dev-appdata не удаляем автоматически (мусор в target).

    const p = spawn(exePath, [], { cwd: exeDir, detached: true, stdio: 'ignore', env });
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

// ── Убийство ТОЛЬКО dev-инстанса сборки, никогда не пользовательской копии ──
//
// Историческая проблема: killApp делал `taskkill /IM <app>.exe` — это убивал
// ВСЕ процессы с таким именем в системе, включая установленную рабочую копию
// (напр. D:\Programs\nildencorp\King Orch\king_orch.exe). Из-за этого нельзя
// было держать релиз открытым во время сборки, а ИИ-агенты повторяли ту же
// команду при ошибках линковки.
//
// Правило: убивать ТОЛЬКО процессы, чей ExecutablePath лежит внутри
// `src-tauri/target/**` ЭТОГО проекта. Пользовательские установленные копии
// лежат вне projectRoot и не трогаются принципиально.

// Используем PowerShell -EncodedCommand, чтобы полностью исключить проблемы
// кавычек/экранирования в cmd при передаче путей с пробелами и кириллицей.
function psEncoded(script) {
    return Buffer.from(script, 'utf16le').toString('base64');
}

// Собрать список PID процессов с данным именем, чей exe внутри каталога.
function findAppPidsInDir(cfg, nameFilter, dir) {
    const script = [
        `$dir = [IO.Path]::GetFullPath('${dir.replace(/'/g, "''")}')`,
        `$root = $dir.TrimEnd('\\') + '\\'`,
        `Get-CimInstance Win32_Process -Filter "Name='${nameFilter}'" -ErrorAction SilentlyContinue |`,
        `  Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($root, [StringComparison]::OrdinalIgnoreCase) } |`,
        `  ForEach-Object { $_.ProcessId }`,
    ].join('\n');
    const out = tryRun(cfg, 'powershell', ['-NoProfile', '-NonInteractive', '-EncodedCommand', psEncoded(script)]);
    if (!out) return [];
    return out.split(/\r?\n/).map((s) => s.trim()).filter((s) => /^\d+$/.test(s));
}

// Закрытие запущенного dev-инстанса приложения (высвобождает file lock exe).
// Убивает ТОЛЬКО процессы из target/{debug,release} ЭТОГО проекта.
function killApp(cfg, opts = {}) {
    if (!cfg.appExe) return;
    const exeName = `${cfg.appExe}.exe`;
    const targetDir = path.join(cfg.projectRoot, 'src-tauri', 'target');
    const profiles = opts.profiles || ['release', 'debug'];

    for (const profile of profiles) {
        const dir = path.join(targetDir, profile);
        let pids = [];
        try {
            pids = findAppPidsInDir(cfg, exeName, dir);
        } catch (e) {
            continue;
        }
        if (!pids.length) continue;
        console.log(`killApp: ${exeName} (${profile}) -> PID ${pids.join(', ')}`);
        for (const pid of pids) {
            try {
                run(cfg, 'taskkill', ['/F', '/T', '/PID', String(pid)], { quiet: true });
            } catch (e) {
                /* процесс уже завершился — не ошибка */
            }
        }
    }

    // Диагностика: пользовательские копии (вне projectRoot) НЕ трогаем, но если
    // что-то всё же держит target-exe — говорим об этом явно.
    const stillLocked = findAppPidsInDir(cfg, exeName, targetDir);
    if (stillLocked.length) {
        console.warn(
            `killApp: ВНИМАНИЕ — остались процессы в target: PID ${stillLocked.join(', ')}. ` +
            `Если сборка упадёт с "os error 32" — закрой их вручную.`
        );
    }
}

module.exports = { run, tryRun, launchApp, killApp, buildCmd, findAppPidsInDir };