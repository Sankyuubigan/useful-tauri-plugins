// Проверка кодировки файлов проекта (UTF-8 strict, без BOM, без mojibake / кракозябр CP1251)

const fs = require('fs');
const path = require('path');

// Папки и файлы, которые не нужно проверять.
// ВАЖНО: папка проекта "test" исключается лишь частично — вендорные архивы
// zapret внутри неё пропускаются по расширениям, но собственные исходники
// (например .py) проверяются наравне со всем остальным.
const EXCLUDE_DIRS = ['node_modules', 'target', '.git', 'dist', 'build', '.vite', '.next', 'out'];
const EXCLUDE_EXTENSIONS = ['.png', '.jpg', '.jpeg', '.ico', '.exe', '.sys', '.dll', '.bin', '.zip', '.7z', '.rar', '.wixobj', '.wixpdb', '.pdf', '.woff', '.woff2', '.ttf', '.log', '.sqlite', '.db'];
// Файлы-логи сборки: сюда попадает перенаправленный stdout/stderr, их кодировка
// не имеет отношения к исходникам и всегда будет битой из-за консоли Windows.
const EXCLUDE_FILES = ['build_out.txt', 'build_err.txt', 'npm-debug.log', 'yarn-error.log'];
const SOURCE_EXTENSIONS = ['.rs', '.ts', '.tsx', '.js', '.jsx', '.mjs', '.cjs', '.json', '.nsh', '.bat', '.cmd', '.ps1', '.py', '.html', '.css', '.yml', '.yaml', '.toml', '.txt', '.md', '.ini', '.cfg'];

// ---------------------------------------------------------------------------
// Таблица CP1251
// ---------------------------------------------------------------------------
// Диапазон 0x80-0xBF: символ -> байт. Индекс массива = байт - 0x80.
const CP1251_HIGH = [
    0x0402, 0x0403, 0x201a, 0x0453, 0x201e, 0x2026, 0x2020, 0x2021, 0x20ac, 0x2030,
    0x0409, 0x2039, 0x040a, 0x040c, 0x040b, 0x040f, 0x0452, 0x2018, 0x2019, 0x201c,
    0x201d, 0x2022, 0x2013, 0x2014, -1, 0x2122, 0x0459, 0x203a, 0x045a, 0x045c, 0x045b, 0x045f,
    0x00a0, 0x0401, 0x0402, 0x0404, 0x00a4, 0x0490, 0x00a6, 0x00a7, 0x0401, 0x00a9,
    0x0404, 0x00ab, 0x00ac, 0x00ad, 0x00ae, 0x0407, 0x00b0, 0x00b1, 0x0406, 0x0456,
    0x0491, 0x00b5, 0x00b6, 0x00b7, 0x0451, 0x2116, 0x0454, 0x00bb, 0x0458, 0x0405, 0x0455, 0x0457,
];

// Полная карта символ -> байт CP1251 (только то, что реально встречается в русском тексте).
const ENCODE = new Map();
for (let i = 0; i < CP1251_HIGH.length; i++) {
    const code = CP1251_HIGH[i];
    // Приоритет у младших байтов (0x80-0x9F): именно они получаются при
    // декодировании UTF-8, а не 0xA0-0xBF.
    if (code >= 0 && !ENCODE.has(code)) ENCODE.set(code, 0x80 + i);
}
for (let c = 0x0410; c <= 0x044f; c++) ENCODE.set(c, 0xc0 + (c - 0x0410)); // А-Я а-я

// Символы, которые не встречаются в нормальном русском тексте, но всегда есть
// в мохибейке. Используются только для грубой фильтрации кандидатов.
const MOJIBAKE_MARKERS = /[\u0402\u0403\u0453\u201a\u201e\u2026\u2020\u2021\u20ac\u2030\u0409\u2039\u040a\u040c\u040b\u040f\u0452\u2018\u2019\u201c\u201d\u2022\u2013\u2014\u2122\u0459\u203a\u045a\u045c\u045b\u045f\u045e\u0456\u0455\u0457\u0454\u0490\u0491]/;

// Управляющие символы, которые не могут встречаться в исходниках:
// C0 (кроме \t \n \r) и C1 (0x80-0x9F). Последние особенно опасны — это
// "неопределённые" байты CP1251, которые остаются после битой перекодировки
// и ломают вывод в консоль/лог.
const CTRL_CHARS = /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f]/;

// ---------------------------------------------------------------------------
// Валидатор UTF-8: длина корректной последовательности, начинающейся с позиции i.
// Возвращает 0, если байт не может начинать валидную последовательность.
// ---------------------------------------------------------------------------
function utf8SeqLen(bytes, i) {
    const b0 = bytes[i];
    if (b0 === undefined) return 0;
    if (b0 < 0x80) return 1;
    const cont = (n) => (bytes[n] !== undefined && bytes[n] >= 0x80 && bytes[n] <= 0xbf ? 1 : 0);
    if (b0 >= 0xc2 && b0 <= 0xdf) return cont(i + 1) ? 2 : 0;
    if (b0 === 0xe0) return cont(i + 1) && bytes[i + 1] <= 0x9f && cont(i + 2) ? 3 : 0;
    if ((b0 >= 0xe1 && b0 <= 0xec) || b0 === 0xee || b0 === 0xef) {
        return cont(i + 1) && cont(i + 2) ? 3 : 0;
    }
    if (b0 === 0xed) return cont(i + 1) && bytes[i + 1] <= 0x9f && cont(i + 2) ? 3 : 0;
    if (b0 === 0xf0) return cont(i + 1) && bytes[i + 1] >= 0x90 && cont(i + 2) && cont(i + 3) ? 4 : 0;
    if (b0 >= 0xf1 && b0 <= 0xf3) return cont(i + 1) && cont(i + 2) && cont(i + 3) ? 4 : 0;
    if (b0 === 0xf4) return cont(i + 1) && bytes[i + 1] <= 0x8f && cont(i + 2) && cont(i + 3) ? 4 : 0;
    return 0; // 0x80-0xC1 и 0xF5-0xFF не могут начинать последовательность
}

const isCyrillicCode = (code) => code >= 0x0400 && code <= 0x04ff;

// Символы-разделители, которые вклиниваются ВНУТРЬ мохибейк-прогонов при
// многократной перекодировке. Сами по себе они не текст, но и не разрыв:
// соответствующий им байт (0x98, 0x9C, ...) — это хвост предыдущей
// UTF-8-последовательности, поэтому прогон надо восстанавливать вместе с ними.
const isBridgeChar = (code) =>
    code === 0x0020 || code === 0x00a0 || (code >= 0x0080 && code <= 0x009f);

// Максимальные участки "подозрительных" символов: кириллица + спецсимволы CP1251.
// Пробелы и "дыры" CP1251 (U+0080-U+009F) считаются ЧАСТЬЮ участка, если с
// обеих сторон от них идёт кириллица. Причина: при многократной битой
// перекодировке между буквами появляются "мусорные" пробелы/неразрывные
// пробелы. Если рвать по ним участок, мохибейк рассыпается на короткие
// кусочки и перестаёт детектироваться — а именно это нужно ловить.
function suspiciousSpans(line) {
    const spans = [];
    const codes = [];
    for (let i = 0; i < line.length; i++) codes.push(line.charCodeAt(i));

    const isSuspicious = (c) => c >= 0 && (isCyrillicCode(c) || ENCODE.has(c));
    const isFiller = isBridgeChar;

    let start = -1;
    for (let i = 0; i <= line.length; i++) {
        const code = i < line.length ? codes[i] : -1;
        let extend = false;
        if (isSuspicious(code)) {
            extend = true;
        } else if (isFiller(code) && start !== -1) {
            // Пробел/-дыра продолжают участок, только если дальше снова кириллица.
            let j = i;
            while (j < line.length && isFiller(codes[j])) j++;
            extend = j < line.length && isSuspicious(codes[j]);
        }
        if (extend) {
            if (start === -1) start = i;
        } else if (start !== -1) {
            if (i - start >= 2) spans.push({ start, end: i });
            start = -1;
        }
    }
    return spans;
}

// Дискриминант "настоящий мохибейк" vs "случайное совпадение".
//
// Одиночные кириллические буквы иногда случайно дают валидный UTF-8:
//   "СЁ" -> байты D1 A1 -> декодируется как "Я" (ложное срабатывание!)
// Настоящий же мохибейк всегда длинный и РОВНЫЙ: каждый символ исходника —
// это ровно один байт CP1251, а декодирование даёт сплошной кириллический текст
// без единых "дыр".
function isStrongMojibake(original, decoded) {
    if (!/[\u0400-\u04ff]/.test(decoded)) return false;

    // ОБЯЗАТЕЛЬНЫЙ строгий признак: прогон обязан содержать символ, который
    // невозможен в нормальном русском тексте:
    //   - "дыры" CP1251 (U+0080-U+009F)  — прямой след битой перекодировки
    //   - спецсимволы CP1251 (0x201c/0x201d/0x2018/0x2019/0x2013/0x2014/…)
    //     или "дырявые" кириллические (Ћ, Ќ, ѕ, є, ї, Ґ …)
    // Без этого критерия обычный русский текст даёт ложные срабатывания:
    //   "за" -> D0 B7 D0 B0 -> декодируется как "Р·Р°" -> "от"
    const MUST_HAVE_MARKER = /[\u0080-\u009f\u0402\u0403\u0409\u040a\u040b\u040c\u040f\u0452\u0453\u0454\u0455\u0456\u0457\u0459\u045a\u045b\u045c\u045e\u045f\u0490\u0491\u2013\u2014\u2018\u2019\u201a\u201c\u201d\u201e\u2020\u2021\u2022\u2026\u2030\u2039\u203a\u20ac\u2122\u0408\u040e]/;
    if (!MUST_HAVE_MARKER.test(original)) return false;

    if (decoded.length < 2) return false;
    const bytes = [];
    for (let i = 0; i < original.length; i++) bytes.push(runCharToByte(original.charCodeAt(i)));
    if (bytes.some((b) => b === undefined)) return false;
    const used = Buffer.from(bytes).toString('utf8');
    if (Buffer.compare(Buffer.from(used, 'utf8'), Buffer.from(bytes)) !== 0) return false;
    const cyr = (decoded.match(/[\u0400-\u04ff]/g) || []).length;
    return cyr / decoded.length >= 0.5;
}

// Байтовое представление символа прогона. Мост-символы (пробел/NBSP/U+0098..)
// отображаются в собственные байты 0x20/0xA0/0x98.. — это хвосты, которые
// и образуют "рваный" UTF-8, и должны участвовать в обратном декодировании.
function runCharToByte(code) {
    if (ENCODE.has(code)) return ENCODE.get(code);
    if (isBridgeChar(code)) return code === 0x00a0 ? 0xa0 : code;
    return undefined;
}

// Внутри участка ищем максимальные подпоследовательности байтов, которые
// декодируются как валидный UTF-8. Именно они и есть мохибейк.
function findMojibakeRuns(line) {
    const runs = [];
    for (const span of suspiciousSpans(line)) {
        const raw = line.slice(span.start, span.end);
        const bytes = [];
        for (let i = 0; i < raw.length; i++) bytes.push(runCharToByte(raw.charCodeAt(i)));

        let i = 0;
        while (i < bytes.length) {
            const len = utf8SeqLen(bytes, i);
            if (len === 0) { i++; continue; }
            // Собираем максимальный валидный прогон
            let end = i;
            while (end < bytes.length) {
                const l = utf8SeqLen(bytes, end);
                if (l === 0) break;
                end += l;
            }
            const segStart = span.start + i;
            const segEnd = span.start + end;
            const original = line.slice(segStart, segEnd);
            const decoded = Buffer.from(bytes.slice(i, end)).toString('utf8');
            if (decoded !== original && /[\u0400-\u04ff]/.test(decoded) && isStrongMojibake(original, decoded)) {
                runs.push({ start: segStart, end: segEnd, text: original, fixed: decoded });
            }
            i = end;
        }
    }
    return runs;
}

function repairMojibakeRun(run) {
    return run.fixed;
}

// Восстановление целой строки: заменяем все мохибейк-фрагменты.
function repairLine(line) {
    const runs = findMojibakeRuns(line);
    if (runs.length === 0) return { text: line, repaired: false, runs: [] };
    let out = '';
    let pos = 0;
    for (const run of runs) {
        out += line.slice(pos, run.start) + run.fixed;
        pos = run.end;
    }
    out += line.slice(pos);
    return { text: out, repaired: true, runs };
}

function walkDir(dir, fileList = []) {
    if (!fs.existsSync(dir)) return fileList;
    const entries = fs.readdirSync(dir, { withFileTypes: true });
    for (const entry of entries) {
        if (entry.isDirectory()) {
            if (EXCLUDE_DIRS.includes(entry.name)) continue;
            walkDir(path.join(dir, entry.name), fileList);
        } else if (entry.isFile()) {
            const ext = path.extname(entry.name).toLowerCase();
            if (EXCLUDE_EXTENSIONS.includes(ext)) continue;
            if (EXCLUDE_FILES.includes(entry.name.toLowerCase())) continue;
            fileList.push(path.join(dir, entry.name));
        }
    }
    return fileList;
}

function analyzeFile(text) {
    const lines = text.split('\n');
    const issues = [];
    for (let i = 0; i < lines.length; i++) {
        for (const run of findMojibakeRuns(lines[i])) {
            issues.push({ line: i + 1, raw: run.text, fixed: run.fixed });
        }
        // Отдельная проверка: управляющие символы (C0/C1) в исходниках недопустимы.
        // Остатки битой перекодировки дают именно их.
        if (CTRL_CHARS.test(lines[i])) {
            const bad = Array.from(lines[i]).filter((c) => CTRL_CHARS.test(c));
            issues.push({
                line: i + 1,
                raw: bad.map((c) => 'U+' + c.codePointAt(0).toString(16).toUpperCase().padStart(4, '0')).join(','),
                fixed: null,
                kind: 'ctrl',
            });
        }
    }
    return issues;
}

function repairFileText(text) {
    return text.split('\n').map((line) => repairLine(line).text).join('\n');
}

function checkProjectEncodings(cfg) {
    const root = cfg.projectRoot;
    console.log(`[encoding-check] Scanning project files in ${root} for strict UTF-8 & Mojibake...`);

    const files = walkDir(root);
    let errorCount = 0;
    const report = [];

    for (const file of files) {
        const relPath = path.relative(root, file);
        let buffer;
        try {
            buffer = fs.readFileSync(file);
        } catch (e) {
            continue;
        }

        // 1. Строгая валидность UTF-8
        const decoder = new TextDecoder('utf-8', { fatal: true });
        let text;
        try {
            text = decoder.decode(buffer);
        } catch (e) {
            console.error(`[ERROR] Not valid UTF-8: ${relPath}`);
            report.push(`  ${relPath} :: файл не является валидным UTF-8`);
            errorCount++;
            continue;
        }

        // 2. BOM
        if (buffer.length >= 3 && buffer[0] === 0xef && buffer[1] === 0xbb && buffer[2] === 0xbf) {
            buffer = buffer.slice(3);
            text = decoder.decode(buffer);
            fs.writeFileSync(file, buffer);
            console.log(`[encoding-check] Auto-removed UTF-8 BOM from: ${relPath}`);
        }

        // 3. Mojibake (двойное кодирование UTF-8 -> CP1251)
        const ext = path.extname(file).toLowerCase();
        if (!SOURCE_EXTENSIONS.includes(ext)) continue;

        const issues = analyzeFile(text);
        if (issues.length > 0) {
            errorCount++;
            console.error(`[ERROR] Mojibake detected: ${relPath} (${issues.length} fragment(s))`);
            const fileReport = [`  ${relPath} :: найдено фрагментов: ${issues.length}`];
            for (const issue of issues.slice(0, 5)) {
                const preview = issue.raw.length > 60 ? issue.raw.slice(0, 60) + '...' : issue.raw;
                const fixedText = issue.kind === 'ctrl'
                    ? '<управляющие символы — удалите вручную>'
                    : (issue.fixed.length > 60 ? issue.fixed.slice(0, 60) + '...' : issue.fixed);
                console.error(`    line ${issue.line}: "${preview}" -> "${fixedText}"`);
                fileReport.push(`    line ${issue.line}: "${preview}"`);
                fileReport.push(`      ожидается: "${fixedText}"`);
            }
            if (issues.length > 5) {
                const rest = issues.length - 5;
                console.error(`    ... и ещё ${rest} фрагмент(ов)`);
                fileReport.push(`    ... и ещё ${rest} фрагмент(ов)`);
            }
            report.push(...fileReport);
        }
    }

    if (errorCount > 0) {
        const details = report.length ? '\n' + report.join('\n') : '';
        throw new Error(
            `[ENCODING-CHECK] Найдено ${errorCount} файл(ов) с нарушениями кодировки!` +
            `\nВсе исходники должны быть в UTF-8 без BOM и без moхибейк (кракозябр).` +
            `\nМохибейк = русский текст прошёл двойное кодирование UTF-8 -> CP1251.` +
            `\nТакие строки попадают в интерфейс пользователя как "Р”РѕРјРµРЅ".` +
            `\nИсправь кодировку этих файлов (пересохрани в UTF-8) и повтори сборку.` +
            `\nАвтопочинка: node lib/encoding-fix.cjs --project <path>` +
            `${details}`
        );
    }

    console.log('[encoding-check] All project files passed strict UTF-8 check successfully.');
}

module.exports = {
    checkProjectEncodings,
    analyzeFile,
    repairFileText,
    repairLine,
    findMojibakeRuns,
    repairMojibakeRun,
};