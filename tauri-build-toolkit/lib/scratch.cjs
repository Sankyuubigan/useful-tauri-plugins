// Изоляция временной папки СБОРКИ.
//
// Инвариант: ни один процесс в дереве сборки (npm, vite, esbuild, cargo,
// cl.exe, tsc) не пишет во временные файлы в разделяемую системную папку
// %TEMP%. У сборки есть собственная рабочая папка, которую она сама создаёт,
// проверяет и может диагностировать.
//
// Почему это инвариант, а не «лечение симптома»:
//   * системная %TEMP% — общая папка, состоянием которой сборка не владеет:
//     чужие процессы (агенты, npm-инсталляторы) держат в ней файлы, и операции
//     удаления/переименования там отказали с `Access is denied`
//     (воспроизведено: esbuild падал на `remove .../esbuild-<hash>`);
//   * cl.exe на этой машине в той же папке падал с `cl.exe error D8050`;
//   * результат сборки не должен зависеть от состояния папки, которой владеет
//     кто-то ещё.
//
// Где ставится: ОДИН раз, в точке входа CLI (cli.cjs), до запуска любой команды.
// `process.env` наследуется всеми дочерними процессами — включая те, которые
// появятся в тулките позже (spawn/execSync/npm/cargo), поэтому дублировать
// подстановку в каждом вызывающем модуле НЕ нужно.
//
// Молчаливый откат на %TEMP% запрещён: если рабочую папку не удаётся создать
// или записать в неё, сборка падает сразу с точным сообщением. Иначе отказ
// всплыл бы через несколько минут внутри esbuild/cl.exe и выглядел бы как
// « inexplicable» ошибка чужого инструмента.

const fs = require('fs');
const path = require('path');

const DEFAULT_RELATIVE = ['temp', 'buildtmp'];

// ASCII обязателен: cl.exe/link.exe не принимают не-ASCII в TMP (MSVC ищет
// временные файлы по коротким путям). Системная %TEMP% тоже ASCII — значит
// смена пути не должна вносить новый класс отказов.
const NON_ASCII = /[^\x00-\x7F]/;

function defaultScratchDir(projectRoot) {
    return path.join(projectRoot, ...DEFAULT_RELATIVE);
}

function resolveScratchDir(cfg) {
    // Явный путь из .build-config.json — единственный escape hatch для
    // нестандартных случаев (кириллица в пути проекта, короткий путь на C:).
    if (cfg.buildTemp) return path.resolve(cfg.buildTemp);
    return defaultScratchDir(cfg.projectRoot);
}

// Папка пригодна для работы = создаётся, принимает запись, отдаёт файл обратно.
// Именно здесь ловится класс отказов «папка есть, но писать/удалять нельзя».
function assertUsable(dir) {
    fs.mkdirSync(dir, { recursive: true });
    const probe = path.join(dir, `probe-${process.pid}.tmp`);
    fs.writeFileSync(probe, 'probe');
    fs.unlinkSync(probe);
}

function describeFailure(dir, err) {
    return (
        `Не удалось использовать папку сборки ${dir} как временную: ${err.message}\n` +
        `Сборка не будет использовать системную %TEMP%: там уже воспроизводился отказ\n` +
        `удаления временных файлов (esbuild: "remove ...: Access is denied", cl.exe: D8050).\n` +
        `Укажи другую папку в .build-config.json -> "buildTemp": "<путь>",` +
        `либо создай ${dir} вручную и проверь права.`
    );
}

// Основная точка входа. Возвращает путь; бросает исключение, если папка
// непригодна — вызывающий код должен упасть до начала сборки.
function isolateBuildTemp(cfg) {
    const dir = resolveScratchDir(cfg);

    if (NON_ASCII.test(dir)) {
        throw new Error(
            `Путь временной папки сборки содержит не-ASCII символы: ${dir}\n` +
            `cl.exe/link.exe с такими путями не работают. Укажи ASCII-путь в\n` +
            `.build-config.json -> "buildTemp": "<C:/.../buildtmp>".`
        );
    }

    try {
        assertUsable(dir);
    } catch (err) {
        throw new Error(describeFailure(dir, err));
    }

    process.env.TMP = dir;
    process.env.TEMP = dir;
    process.env.TMPDIR = dir;
    cfg.buildTemp = dir;
    return dir;
}

// Команды, которые порождают дерево процессов сборки. `doctor` — read-only
// диагностика, `init` — только раскладка шаблонов: им временная папка не нужна,
// и создавать папки у пользователя «просто так» нельзя.
const SPAWNING_COMMANDS = new Set(['build', 'prep', 'installer', 'release', 'test', 'version']);

module.exports = { isolateBuildTemp, resolveScratchDir, defaultScratchDir, SPAWNING_COMMANDS };