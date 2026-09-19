// Селектор установщика в bundle/nsis.
// Анти-паттерн (глобальная доку §3.7): имя установщика НЕ должно матчиться
// префиксом — только ТОЧНОЕ соответствие вида `_X.Y.Z_x64-setup.exe`.

const fs = require('fs');
const path = require('path');

// Ищет ровно один `_<version>_x64-setup.exe` внутри target/release/bundle.
// Возвращает полный путь или null, если не найден. Несколько совпадений -> ошибка
// (значит, матч слишком широкий, и выбор может попасть не в тот файл).
function findInstallerBundle(bundleDir, version) {
    const pattern = `_${version}_x64-setup.exe`;
    const nsisDir = path.join(bundleDir, 'nsis');
    if (!fs.existsSync(nsisDir)) return null;

    const matches = fs.readdirSync(nsisDir).filter((f) => f.endsWith(pattern));
    if (matches.length > 1) {
        throw new Error(`More than one installer matching "${pattern}" in ${nsisDir}: ${matches.join(', ')}`);
    }
    if (matches.length === 0) return null;
    return path.join(nsisDir, matches[0]);
}

module.exports = { findInstallerBundle };