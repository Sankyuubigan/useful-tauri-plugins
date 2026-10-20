//! Файловые тулы ядра: чтение, пакетное чтение, запись, точечная правка.
//!
//! Перенос из King Orch `src-tauri/src/infra/tools/fs.rs` (SSOT теперь здесь).
//! Чтение — любой путь (без подтверждения). Запись — авторизуется хуком
//! [`ToolCtx::authorize_write`]; политика (авто/плашка/запрет) — на стороне хоста.

use std::fs;
use std::io::Write;
use std::path::Path;

use serde_json::Value;

use crate::contract::{CodeTool, ToolCtx, ToolError, resolve_path, truncate};
use crate::fuzzy::{self, FuzzyMatch};

fn arg_str(args: &Value, key: &str) -> Result<String, ToolError> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| ToolError::Usage(format!("параметр '{}' (строка) обязателен", key)))
}

fn arg_int(args: &Value, key: &str, default: i64) -> i64 {
    args.get(key).and_then(|v| v.as_i64()).unwrap_or(default)
}

/// Одна строка длиннее этого предела режется с пометкой (и модель знает,
/// что нельзя копировать её в edit_file целиком — иначе exact-match упадёт).
const MAX_READ_LINE_CHARS: usize = 2000;

/// Резервные имена устройств Windows. Проверка по stem: `NUL.txt` тоже устройство.
const DEVICE_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "CLOCK$",
    "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
    "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Бинарные расширения: модель читает только текст (как binary-guard у minimax-code).
const BINARY_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "tiff", "tif", "avif",
    "mp4", "avi", "mkv", "mov", "webm", "flv", "wmv", "m4v", "mpg", "mpeg",
    "mp3", "wav", "flac", "ogg", "oga", "aac", "m4a", "wma", "opus",
    "zip", "rar", "7z", "tar", "gz", "bz2", "xz", "tgz", "zst", "lz4",
    "exe", "dll", "so", "dylib", "bin", "obj", "o", "class", "jar", "war",
    "wasm", "pyc", "pyo", "pyd", "pdb", "deb", "rpm", "msi",
    "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "pdf",
    "ttf", "otf", "woff", "woff2", "eot",
    "db", "sqlite", "sqlite3", "mdb", "dbf", "accdb",
    "gguf", "ggml", "safetensors", "ckpt", "onnx", "pb", "tflite", "mlmodel", "pt", "pth",
    "iso", "img", "dmg", "lib", "a", "rlib",
];

fn is_windows_device_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let stem = name.split('.').next().unwrap_or(name);
    let upper = stem.to_ascii_uppercase();
    DEVICE_NAMES.contains(&upper.as_str())
}

fn is_binary_extension(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    BINARY_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
}

/// Сэмпл выглядит бинарным: встречен NUL-байт, либо доля управляющих
/// символов больше 30% (как 4КБ-сэмпл у read-guards minimax-code).
fn detect_binary_bytes(sample: &[u8]) -> bool {
    if sample.contains(&0) {
        return true;
    }
    if sample.is_empty() {
        return false;
    }
    let control = sample
        .iter()
        .filter(|&&b| b < 0x20 && !matches!(b, b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r'))
        .count();
    control * 100 / sample.len() > 30
}

/// Гарды до чтения: устройство, бинарное расширение, не-регулярный файл.
fn guard_read_path(path: &Path) -> Result<(), ToolError> {
    if is_windows_device_path(path) {
        return Err(ToolError::Forbidden(format!(
            "{}: зарезервированное устройство Windows. Модель читает только обычные файлы — используй bash, если нужен реальный доступ",
            path.display()
        )));
    }
    if is_binary_extension(path) {
        return Err(ToolError::Forbidden(format!(
            "{}: бинарный файл. Модель читает только текст — используй bash для работы с этим файлом",
            path.display()
        )));
    }
    match fs::metadata(path) {
        Ok(md) => {
            if !md.is_file() {
                return Err(ToolError::Forbidden(format!(
                    "{}: не обычный файл (каталог/устройство/pipe)",
                    path.display()
                )));
            }
        }
        Err(e) => return Err(ToolError::NotFound(format!("{}: {}", path.display(), e))),
    }
    Ok(())
}

/// Прочитать файл с номерами строк и фрагментацией (offset/limit).
/// Формат строки — `     N→ текст` (5-поз. номер + стрелка), как у minimax-code.
fn read_file_with_lines(path: &Path, offset: usize, limit: usize) -> Result<String, ToolError> {
    guard_read_path(path)?;
    let bytes = fs::read(path)
        .map_err(|e| ToolError::NotFound(format!("{}: {}", path.display(), e)))?;
    if detect_binary_bytes(&bytes[..bytes.len().min(4096)]) {
        return Err(ToolError::Forbidden(format!(
            "{}: содержимое выглядит бинарным. Модель читает только текст — используй bash для работы с этим файлом",
            path.display()
        )));
    }
    let content = String::from_utf8(bytes).map_err(|_| {
        ToolError::Forbidden(format!(
            "{}: файл не в UTF-8 (бинарная кодировка). Модель читает только текст — используй bash",
            path.display()
        ))
    })?;
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    let start = offset.saturating_sub(1);
    let end = (start + limit).min(total);
    let mut out = String::new();
    let mut truncated: Vec<usize> = Vec::new();
    for (i, line) in lines.iter().enumerate().take(end).skip(start) {
        let lineno = i + 1;
        if line.chars().count() > MAX_READ_LINE_CHARS {
            truncated.push(lineno);
            let head: String = line.chars().take(MAX_READ_LINE_CHARS).collect();
            out.push_str(&format!("{:>5}→ {}\n", lineno, head));
        } else {
            out.push_str(&format!("{:>5}→ {}\n", lineno, line));
        }
    }
    if total > end {
        out.push_str(&format!(
            "… [показаны строки {}-{} из {}. Продолжай с offset={}]\n",
            start + 1,
            end,
            total,
            end + 1
        ));
    } else if start > 0 {
        out.push_str(&format!("… [показаны строки {}-{} из {}]\n", start + 1, end, total));
    }
    if !truncated.is_empty() {
        let nums: Vec<String> = truncated.iter().map(|n| n.to_string()).collect();
        out.push_str(&format!(
            "[Строки [{}] обрезаны до {} символов — не копируй их в edit_file целиком.]\n",
            nums.join(", "),
            MAX_READ_LINE_CHARS
        ));
    }
    if out.is_empty() {
        out = format!("[файл пуст или нет строк в диапазоне: {} строк(и)]", total);
    }
    Ok(out)
}

fn write_diff_summary(path: &Path, old: &str, new: &str) -> String {
    if old.is_empty() {
        format!("✅ Файл создан: {} ({} символов)", path.display(), new.chars().count())
    } else if old == new {
        format!("ℹ️ Файл {} уже содержит это содержимое — запись не требуется.", path.display())
    } else {
        let old_lines = old.lines().count();
        let new_lines = new.lines().count();
        format!(
            "✅ Файл обновлён: {} (строк: {} → {})",
            path.display(),
            old_lines,
            new_lines
        )
    }
}

/// `read_file` — чтение одного файла с номерами строк.
pub struct ReadFile;

impl CodeTool for ReadFile {
    fn name(&self) -> &str {
        "read_file"
    }
    fn description(&self) -> &str {
        "Прочитать текстовый файл с номерами строк (формат '     N→ текст'). path — путь к файлу (абсолютный или относительно корня проекта). offset — номер строки, с которой начать (1-based, по умолчанию 1); limit — сколько строк прочитать (по умолчанию 200). Строки длиннее 2000 символов обрезаются с пометкой внизу. Если в конце вывода есть подсказка 'Продолжай с offset=N' — читай дальше диапазонами. Чтение доступно по любому пути. Бинарные файлы и устройства (NUL и т.п.) не читаются — вернётся 'Запрещено'; для них используй bash."
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Путь к файлу (абсолютный или относительно корня проекта)"},
                "offset": {"type": "integer", "description": "Номер строки, с которой начать (1-based, по умолчанию 1)"},
                "limit": {"type": "integer", "description": "Сколько строк прочитать (по умолчанию 200)"}
            },
            "required": ["path"]
        })
    }
    fn is_readonly(&self) -> bool {
        true
    }
    fn execute(&self, args: &Value, ctx: &ToolCtx) -> Result<String, ToolError> {
        let path = resolve_path(ctx.workspace_root, &arg_str(args, "path")?);
        let offset = arg_int(args, "offset", 1).max(1) as usize;
        let limit = arg_int(args, "limit", 200).clamp(1, 2000) as usize;
        Ok(read_file_with_lines(&path, offset, limit)?)
    }
}

/// `read_many_files` — пакетное чтение нескольких файлов.
pub struct ReadManyFiles;

impl CodeTool for ReadManyFiles {
    fn name(&self) -> &str {
        "read_many_files"
    }
    fn description(&self) -> &str {
        "Прочитать несколько файлов одним вызовом. paths — массив путей (абсолютных или относительно корня проекта). Каждый файл выводится в блоке с заголовком. Экономит вызовы: вместо серии read_file. Бинарные файлы и устройства не читаются ('Запрещено') — для них используй bash."
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "paths": {"type": "array", "items": {"type": "string"}, "description": "Массив путей к файлам"}
            },
            "required": ["paths"]
        })
    }
    fn is_readonly(&self) -> bool {
        true
    }
    fn execute(&self, args: &Value, ctx: &ToolCtx) -> Result<String, ToolError> {
        let paths = args
            .get("paths")
            .and_then(|v| v.as_array())
            .ok_or_else(|| ToolError::Usage("параметр 'paths' (массив строк) обязателен".to_string()))?;
        let mut out = String::new();
        for p in paths {
            let p_str = p
                .as_str()
                .ok_or_else(|| ToolError::Usage("элемент 'paths' должен быть строкой".to_string()))?;
            let abs = resolve_path(ctx.workspace_root, p_str);
            out.push_str(&format!("===== {} =====\n", abs.display()));
            match read_file_with_lines(&abs, 1, 2000) {
                Ok(content) => out.push_str(&content),
                Err(e) => out.push_str(&format!("⚠️ {}\n", e)),
            }
            out.push('\n');
        }
        Ok(truncate(&out, 16000))
    }
}

/// `write_file` — создать/перезаписать файл (внутри корня — авто).
pub struct WriteFile;

impl CodeTool for WriteFile {
    fn name(&self) -> &str {
        "write_file"
    }
    fn description(&self) -> &str {
        "Создать новый файл или перезаписать существующий. path — путь (абсолютный или относительно корня проекта). content — полное содержимое файла. Запись внутри корня разрешена автоматически; запись вне корня — запросит подтверждение пользователя. Если после записи появилась новая синтаксическая ошибка — запись откатывается, а ошибка содержит строки вокруг проблемы. Для точечных правок используй edit_file, а не перезапись целиком."
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Путь к файлу (абсолютный или относительно корня проекта)"},
                "content": {"type": "string", "description": "Полное содержимое файла"}
            },
            "required": ["path", "content"]
        })
    }
    fn is_readonly(&self) -> bool {
        false
    }
    fn execute(&self, args: &Value, ctx: &ToolCtx) -> Result<String, ToolError> {
        let path_str = arg_str(args, "path")?;
        let content = arg_str(args, "content")?;
        let abs = resolve_path(ctx.workspace_root, &path_str);
        ctx.authorize_write(&abs, "write_file")?;
        // Прежнее содержимое нужно для отката: файл мог не существовать —
        // тогда откат это удаление, а не восстановление.
        let previous = fs::read_to_string(&abs).ok();
        let old = previous.clone().unwrap_or_default();
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| ToolError::Io(format!("не удалось создать папку {}: {}", parent.display(), e)))?;
        }
        let bytes = content.as_bytes();
        // Baseline проверки качества снимается ДО записи: «до» и «после»
        // должны сравниваться по одному и тому же состоянию файла.
        let baseline = lint_baseline(ctx, &abs);
        let mut f = fs::File::create(&abs)
            .map_err(|e| ToolError::Io(format!("не удалось открыть {}: {}", abs.display(), e)))?;
        f.write_all(bytes)
            .map_err(|e| ToolError::Io(format!("не удалось записать {}: {}", abs.display(), e)))?;
        let mut message = write_diff_summary(&abs, &old, &content);
        match run_lint_gate(ctx, &abs, baseline, previous.as_deref(), &content) {
            Ok(None) => Ok(message),
            Ok(Some(warning)) => {
                message.push('\n');
                message.push_str(&warning);
                Ok(message)
            }
            Err(error) => Err(error),
        }
    }
}

/// Сколько строк вокруг проблемы показывать в сообщении об откате.
pub const LINT_CONTEXT_LINES: u32 = 5;

/// Состояние проверки качества на момент записи.
enum LintBaseline {
    /// Гейт не подключён — проверки нет (это объявлено в описании тула).
    NoGate,
    /// Снимок взят — проверка будет выполнена после записи.
    Checked(Vec<crate::contract::LintProblem>),
    /// Проверка недоступна (сервера нет или сбой) — запись состоится, но агент
    /// увидит предупреждение: непроверенный код не должен выглядеть проверенным.
    Unavailable(String),
}

/// Снять baseline проверки качества ДО записи.
fn lint_baseline(ctx: &ToolCtx, path: &Path) -> LintBaseline {
    let Some(gate) = ctx.lint_gate() else {
        return LintBaseline::NoGate;
    };
    match gate.snapshot(path) {
        Ok(Some(baseline)) => LintBaseline::Checked(baseline),
        Ok(None) => {
            log::warn!(
                "lint-gate: {} — проверка качества не выполнена (сервер проверки недоступен)",
                path.display()
            );
            LintBaseline::Unavailable("сервер проверки недоступен".to_string())
        }
        Err(reason) => {
            log::warn!("lint-gate: {} — проверка не выполнена: {}", path.display(), reason);
            LintBaseline::Unavailable(reason)
        }
    }
}

/// Проверить изменённый файл хуком качества и при необходимости откатить запись.
///
/// ## Что происходит
///
/// - хук не подключён → ничего не проверяем (это объявлено в описании тула);
/// - хук ответил списком новых ошибок → запись ОТКАТЫВАЕТСЯ, а вызывающий
///   получает ошибку с окружением строк вокруг каждой проблемы;
/// - хук не смог ответить (например, LSP-сервер не запущен) → запись
///   остаётся, но сбой проверки попадает и в лог, и в ответ тула: молчать
///   о непроверенном коде нельзя.
fn run_lint_gate(
    ctx: &ToolCtx,
    path: &Path,
    baseline: LintBaseline,
    previous: Option<&str>,
    written: &str,
) -> Result<Option<String>, ToolError> {
    let Some(gate) = ctx.lint_gate() else {
        return Ok(None);
    };
    let baseline = match baseline {
        LintBaseline::NoGate => return Ok(None),
        LintBaseline::Unavailable(reason) => {
            return Ok(Some(format!(
                "⚠️ проверка качества кода не выполнена ({}): {}",
                path.display(),
                reason
            )));
        }
        LintBaseline::Checked(baseline) => baseline,
    };
    match gate.check(path, &baseline) {
        Ok(Some(problems)) => {
            if problems.is_empty() {
                log::debug!(
                    "lint-gate: {} — новых ошибок нет ({} символов)",
                    path.display(),
                    written.chars().count()
                );
                return Ok(None);
            }
            // Откат: возвращаем прежнее содержимое (или убираем новый файл).
            log::info!(
                "lint-gate: {} — новых ошибок {}, запись откатывается",
                path.display(),
                problems.len()
            );
            match previous {
                Some(old) => {
                    fs::write(path, old).map_err(|e| {
                        ToolError::Io(format!("не удалось откатить {}: {}", path.display(), e))
                    })?;
                }
                None => {
                    let _ = fs::remove_file(path);
                }
            }
            let detail = describe_lint_problems(written, &problems);
            Err(ToolError::Lint(detail))
        }
        Ok(None) => {
            log::warn!(
                "lint-gate: {} — проверка качества не выполнена (сервер проверки недоступен)",
                path.display()
            );
            Ok(Some(format!(
                "⚠️ проверка качества кода не выполнена ({}): сервер проверки недоступен",
                path.display()
            )))
        }
        Err(reason) => {
            log::warn!(
                "lint-gate: {} — проверка не выполнена: {}",
                path.display(),
                reason
            );
            Ok(Some(format!(
                "⚠️ проверка качества кода не выполнена ({}): {}",
                path.display(),
                reason
            )))
        }
    }
}

/// Описать проблемы с окружением строк вокруг каждой.
fn describe_lint_problems(content: &str, problems: &[crate::contract::LintProblem]) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut out = String::new();
    for (index, problem) in problems.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        out.push_str(&format!(
            "строка {}: {}\n",
            problem.line, problem.message
        ));
        let center = problem.line.max(1) as usize;
        let start = center.saturating_sub(LINT_CONTEXT_LINES as usize).max(1);
        let end = (center + LINT_CONTEXT_LINES as usize).min(lines.len().max(1));
        for number in start..=end {
            if number > lines.len() {
                break;
            }
            out.push_str(&format!("{:>5}→ {}\n", number, lines[number - 1]));
        }
    }
    out
}

/// `edit_file` — точечная правка по точному совпадению фрагмента (exact-match).
pub struct EditFile;

/// Одна замена в мульти-правке `edits[]`.
#[derive(Clone)]
struct EditSpec {
    old: String,
    new: String,
}

/// Разобрать строку блоков `edits` из аргументов тула.
fn parse_edits_array(args: &Value) -> Result<Vec<EditSpec>, ToolError> {
    let arr = args
        .get("edits")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ToolError::Usage("параметр 'edits' (массив {oldText,newText}) обязателен".to_string()))?;
    if arr.is_empty() {
        return Err(ToolError::Usage("edits не может быть пустым".to_string()));
    }
    let mut specs = Vec::with_capacity(arr.len());
    for (i, item) in arr.iter().enumerate() {
        let old = item
            .get("oldText")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::Usage(format!("edits[{}].oldText (строка) обязателен", i)))?
            .to_string();
        let new = item
            .get("newText")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if old.is_empty() {
            return Err(ToolError::Usage(format!(
                "edits[{}].oldText не может быть пустым",
                i
            )));
        }
        specs.push(EditSpec { old, new });
    }
    Ok(specs)
}

/// Снять с тела строки префикс номера: `<пробелы><цифры><пробелы>(→|\|)<пробелы>`.
/// Возвращает `None`, если префикс не подходит (строка НЕ выглядит как выдача read_file).
fn match_line_number_prefix(body: &str) -> Option<&str> {
    let mut rest = body;
    // Ведущие пробелы.
    if rest.chars().next().is_some_and(|c| c.is_whitespace()) {
        let idx = rest
            .char_indices()
            .find(|(_, c)| !c.is_whitespace())
            .map(|(i, _)| i)
            .unwrap_or(rest.len());
        rest = &rest[idx..];
    }
    // Цифры номера.
    let digits_end = rest
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit())
        .map(|(i, _)| i)
        .unwrap_or(rest.len());
    if digits_end == 0 {
        return None;
    }
    rest = &rest[digits_end..];
    // Пробелы после номера.
    if rest.chars().next().is_some_and(|c| c.is_whitespace()) {
        let idx = rest
            .char_indices()
            .find(|(_, c)| !c.is_whitespace())
            .map(|(i, _)| i)
            .unwrap_or(rest.len());
        rest = &rest[idx..];
    }
    // Маркер: '→' (minimax) или '|' (наш прежний формат).
    let marker = rest.chars().next()?;
    if marker != '→' && marker != '|' {
        return None;
    }
    rest = &rest[marker.len_utf8()..];
    // Хвостовые пробелы.
    if rest.chars().next().is_some_and(|c| c.is_whitespace()) {
        let idx = rest
            .char_indices()
            .find(|(_, c)| !c.is_whitespace())
            .map(|(i, _)| i)
            .unwrap_or(rest.len());
        rest = &rest[idx..];
    }
    Some(rest)
}

/// Снять префиксы номеров со ВСЕГО блока. Возвращает `None`, если хотя бы одна
/// non-empty строка не-префиксная — тогда блок НЕ трогаем (контент может
/// легитимно выглядеть как пронумерованный).
fn strip_line_number_prefixes_from_block(text: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let has_nl = line.ends_with('\n');
        let body = line.strip_suffix('\n').unwrap_or(line);
        if body.trim().is_empty() {
            out.push_str(line);
            continue;
        }
        let stripped = match_line_number_prefix(body)?;
        out.push_str(stripped);
        if has_nl {
            out.push('\n');
        }
    }
    Some(out)
}

/// Ошибка применения правки: текст для модели + класс отказа.
///
/// Отдельный тип нужен, чтобы отличить «правку применять нельзя» (не найдено
/// или неоднозначно) от ошибок ввода-вывода: у них разные последствия и
/// разные коды возврата.
struct EditFailure {
    message: String,
    /// Кандидатов было несколько: сообщение нечёткой попытки полезнее
    /// исходной ошибки exact («не найдено»), поэтому оно доходит до модели.
    ambiguous: bool,
}

/// Разрезать текст по байтовому диапазону (границы символов) на «до» и «после».
///
/// Диапазон вырезается целиком: замена блока — это и есть смысл нечёткого
/// совпадения (фрагмент найден с другими отступами, заменяем именно его).
fn split_byte_range(text: &str, start: usize, end: usize) -> (String, String) {
    let start = start.min(text.len());
    let end = end.clamp(start, text.len());
    (text[..start].to_string(), text[end..].to_string())
}

/// Третья попытка правки: нечёткое совпадение по нормализованным строкам.
///
/// Правило: кандидат должен быть РОВНО ОДИН. Два кандидата — отказ с
/// перечислением, потому что выбор «на глаз» означал бы правку не того места.
fn apply_edits_fuzzy(original: &str, edits: &[EditSpec]) -> Result<(String, String), EditFailure> {
    let mut result = String::with_capacity(original.len());
    let mut rest = original.to_string();
    let mut fuzzy_used = 0usize;

    for (index, edit) in edits.iter().enumerate() {
        // Точное совпадение и здесь имеет приоритет: нечёткий путь — запасной.
        if let Some(pos) = rest.find(&edit.old) {
            result.push_str(&rest[..pos]);
            result.push_str(&edit.new);
            rest = rest[pos + edit.old.len()..].to_string();
            continue;
        }
        match fuzzy::find(&rest, &edit.old) {
            FuzzyMatch::Unique(candidate) => {
                // Отступ замены приводится к отступу найденного фрагмента:
                // иначе код вложенности «поедет» на уровень образца.
                let candidate_first = rest
                    .lines()
                    .nth(candidate.start_line.saturating_sub(1))
                    .unwrap_or_default();
                let pattern_first = edit
                    .old
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or_default();
                let delta = fuzzy::indent_delta(candidate_first, pattern_first);
                let new_text = fuzzy::reindent(&edit.new, delta);
                let (before, after) =
                    split_byte_range(&rest, candidate.start_byte, candidate.end_byte);
                result.push_str(&before);
                result.push_str(&new_text);
                rest = after;
                fuzzy_used += 1;
            }
            FuzzyMatch::Ambiguous(candidates) => {
                let list: Vec<String> = candidates
                    .iter()
                    .map(|c| format!("строки {}-{}", c.start_line, c.end_line))
                    .collect();
                return Err(EditFailure {
                    ambiguous: true,
                    message: format!(
                        "правка {}: нечёткое совпадение неоднозначно — кандидатов {} ({}). \
                         Уточни oldText: впиши больше строк или используй точный фрагмент.",
                        index + 1,
                        candidates.len(),
                        list.join(", ")
                    ),
                });
            }
            FuzzyMatch::None => {
                return Err(EditFailure {
                    ambiguous: false,
                    message: format!(
                        "фрагмент не найден (правка {}): {:?}",
                        index + 1,
                        edit.old.chars().take(80).collect::<String>()
                    ),
                });
            }
        }
    }
    result.push_str(&rest);
    Ok((
        result,
        format!(
            "применено {} правок (из них {} — нечётким совпадением по отступам/пробелам)",
            edits.len(),
            fuzzy_used
        ),
    ))
}

/// Атомарная мульти-правка: применения идут по порядку по ОСТАТКУ исходного
/// контента. Любой oldText не найден → ошибка до записи (ничего не пишем).
fn apply_edits_sequential(original: &str, edits: &[EditSpec]) -> Result<(String, String), String> {
    let mut result = String::with_capacity(original.len());
    let mut rest = original;
    for (i, e) in edits.iter().enumerate() {
        match rest.find(&e.old) {
            Some(pos) => {
                result.push_str(&rest[..pos]);
                result.push_str(&e.new);
                rest = &rest[pos + e.old.len()..];
            }
            None => {
                let preview: String = e.old.chars().take(80).collect();
                return Err(format!("edits[{}] oldText не найден в файле: {:?}", i, preview));
            }
        }
    }
    result.push_str(rest);
    Ok((result, format!("Заменено {} фрагмента(ов)", edits.len())))
}

impl CodeTool for EditFile {
    fn name(&self) -> &str {
        "edit_file"
    }
    fn description(&self) -> &str {
        "Точечная замена фрагментов файла. Три попытки найти фрагмент, по порядку: 1) точное совпадение; 2) снятие префиксов номеров строк (если вставил фрагмент из вывода read_file); 3) нечёткое совпадение — отступы и внутренние пробелы нормализуются, кандидат должен быть ровно один, иначе отказ с перечнем кандидатов. Режимы: edits — массив {oldText,newText} (несколько правок за один вызов, атомарно, вырезы НЕ должны перекрываться); old_string/new_string (legacy) + replace_all. Ошибка возвращается как результат (тишина недопустима). Если после записи появилась новая синтаксическая ошибка — запись откатывается, а ошибка содержит строки вокруг проблемы. Запись вне корня — запросит подтверждение."
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Путь к файлу (абсолютный или относительно корня проекта)"},
                "edits": {"type": "array", "items": {"type": "object", "properties": {
                    "oldText": {"type": "string", "description": "Искомый фрагмент (точное совпадение)"},
                    "newText": {"type": "string", "description": "Замена"}
                }, "required": ["oldText", "newText"]}, "description": "Массив правок (рекомендуется)"},
                "old_string": {"type": "string", "description": "Искомый фрагмент (точное совпадение, legacy)"},
                "new_string": {"type": "string", "description": "Замена (legacy)"},
                "replace_all": {"type": "boolean", "description": "Заменить все вхождения (legacy, по умолчанию false — только первое)"}
            },
            "required": ["path"]
        })
    }
    fn is_readonly(&self) -> bool {
        false
    }
    fn execute(&self, args: &Value, ctx: &ToolCtx) -> Result<String, ToolError> {
        let path = resolve_path(ctx.workspace_root, &arg_str(args, "path")?);
        let has_edits = args.get("edits").is_some();
        let has_legacy = args.get("old_string").is_some() || args.get("new_string").is_some();
        if has_edits && has_legacy {
            return Err(ToolError::Usage(
                "укажи либо edits[], либо old_string/new_string — не оба".to_string(),
            ));
        }
        if !has_edits && !has_legacy {
            return Err(ToolError::Usage(
                "нужен либо edits[] (массив {oldText,newText}), либо old_string + new_string".to_string(),
            ));
        }

        ctx.authorize_write(&path, "edit_file")?;
        let content = fs::read_to_string(&path)
            .map_err(|e| ToolError::NotFound(format!("{}: {}", path.display(), e)))?;

        if has_legacy {
            let old = arg_str(args, "old_string")?;
            if old.is_empty() {
                return Err(ToolError::Usage(
                    "old_string не может быть пустым — использовать write_file для создания файла".to_string(),
                ));
            }
            let new = arg_str(args, "new_string")?;
            let replace_all = args.get("replace_all").and_then(|v| v.as_bool()).unwrap_or(false);
            if !content.contains(&old) {
                // Вторая жизнь того же правила: если точного вхождения нет, ищем
                // нечёткое (нормализованные отступы/пробелы). При нескольких
                // кандидатах — честный отказ, а не выбор «на глаз».
                if replace_all {
                    return Err(ToolError::NotFound(format!(
                        "фрагмент не найден в {}: {:?}",
                        path.display(),
                        &old.chars().take(80).collect::<String>()
                    )));
                }
                let candidate = match fuzzy::find(&content, &old) {
                    FuzzyMatch::Unique(candidate) => candidate,
                    FuzzyMatch::Ambiguous(candidates) => {
                        let list: Vec<String> = candidates
                            .iter()
                            .map(|c| format!("строки {}-{}", c.start_line, c.end_line))
                            .collect();
                        return Err(ToolError::NotFound(format!(
                            "нечёткое совпадение неоднозначно в {} — кандидатов {} ({}). \
                             Уточни old_string",
                            path.display(),
                            candidates.len(),
                            list.join(", ")
                        )));
                    }
                    FuzzyMatch::None => {
                        return Err(ToolError::NotFound(format!(
                            "фрагмент не найден в {}: {:?}",
                            path.display(),
                            &old.chars().take(80).collect::<String>()
                        )));
                    }
                };
                // Отступ берётся из СТРОКИ ФАЙЛА, где начинается найденный
                // диапазон: сам диапазон отступ не содержит (он отброшен при
                // нормализации), и отступ первой строки остаётся в файле.
                let candidate_first = content
                    .lines()
                    .nth(candidate.start_line.saturating_sub(1))
                    .unwrap_or_default();
                let pattern_first = old
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or_default();
                let delta = fuzzy::indent_delta(candidate_first, pattern_first);
                let new_text = fuzzy::reindent(&new, delta);
                let (before, after) =
                    split_byte_range(&content, candidate.start_byte, candidate.end_byte);
                let new_content = format!("{before}{new_text}{after}");
                let baseline = lint_baseline(ctx, &path);
                let mut f = fs::File::create(&path)
                    .map_err(|e| ToolError::Io(format!("не удалось открыть {}: {}", path.display(), e)))?;
                f.write_all(new_content.as_bytes())
                    .map_err(|e| ToolError::Io(format!("не удалось записать {}: {}", path.display(), e)))?;
                let mut message = format!(
                    "✅ Замена выполнена в {} (нечёткое совпадение: отступы/пробелы нормализованы; строки {}-{})",
                    path.display(),
                    candidate.start_line,
                    candidate.end_line
                );
                return match run_lint_gate(ctx, &path, baseline, Some(&content), &new_content) {
                    Ok(None) => Ok(message),
                    Ok(Some(warning)) => {
                        message.push('\n');
                        message.push_str(&warning);
                        Ok(message)
                    }
                    Err(error) => Err(error),
                };
            }
            let new_content = if replace_all {
                content.replace(&old, &new)
            } else {
                content.replacen(&old, &new, 1)
            };
            let baseline = lint_baseline(ctx, &path);
            let mut f = fs::File::create(&path)
                .map_err(|e| ToolError::Io(format!("не удалось открыть {}: {}", path.display(), e)))?;
            f.write_all(new_content.as_bytes())
                .map_err(|e| ToolError::Io(format!("не удалось записать {}: {}", path.display(), e)))?;
            let mut message = format!(
                "✅ Замена выполнена в {} ({} → {}; {} вхождений)",
                path.display(),
                old.chars().count(),
                new.chars().count(),
                if replace_all { content.matches(&old).count() } else { 1 }
            );
            return match run_lint_gate(ctx, &path, baseline, Some(&content), &new_content) {
                Ok(None) => Ok(message),
                Ok(Some(warning)) => {
                    message.push('\n');
                    message.push_str(&warning);
                    Ok(message)
                }
                Err(error) => Err(error),
            };
        }

        // Режим edits[]: первая попытка + один fail-only ретрай с префикс-стрипом
        // + третья попытка (нечёткое совпадение).
        let edits = parse_edits_array(args)?;
        let apply = |specs: &[EditSpec]| -> Result<(String, String, LintBaseline), ToolError> {
            let (new_content, summary) =
                apply_edits_sequential(&content, specs).map_err(ToolError::NotFound)?;
            let baseline = lint_baseline(ctx, &path);
            let mut f = fs::File::create(&path)
                .map_err(|e| ToolError::Io(format!("не удалось открыть {}: {}", path.display(), e)))?;
            f.write_all(new_content.as_bytes())
                .map_err(|e| ToolError::Io(format!("не удалось записать {}: {}", path.display(), e)))?;
            Ok((summary, new_content, baseline))
        };

        match apply(&edits) {
            Ok((summary, new_content, baseline)) => {
                let mut message = format!("✅ {} в {}", summary, path.display());
                return match run_lint_gate(ctx, &path, baseline, Some(&content), &new_content) {
                    Ok(None) => Ok(message),
                    Ok(Some(warning)) => {
                        message.push('\n');
                        message.push_str(&warning);
                        Ok(message)
                    }
                    Err(error) => Err(error),
                };
            }
            // Ретрай — только если упало на не-нахождении (NotFound о тексте),
            // а не на реальной IO-ошибке.
            Err(ToolError::NotFound(first_err)) => {
                // Fail-only ретрай: только если КАЖДЫЙ блок нормализуется (префикс-формы).
                let mut normalized = Vec::with_capacity(edits.len());
                let mut changed = false;
                for e in &edits {
                    let old = strip_line_number_prefixes_from_block(&e.old)
                        .unwrap_or_else(|| e.old.clone());
                    let new = strip_line_number_prefixes_from_block(&e.new)
                        .unwrap_or_else(|| e.new.clone());
                    if old != e.old || new != e.new {
                        changed = true;
                    }
                    normalized.push(EditSpec { old, new });
                }
                if changed {
                    match apply(&normalized) {
                        Ok((summary, new_content, baseline)) => {
                            let mut message = format!(
                                "✅ {} в {} (префиксы номеров строк сняты)",
                                summary,
                                path.display()
                            );
                            return match run_lint_gate(ctx, &path, baseline, Some(&content), &new_content) {
                                Ok(None) => Ok(message),
                                Ok(Some(warning)) => {
                                    message.push('\n');
                                    message.push_str(&warning);
                                    Ok(message)
                                }
                                Err(error) => Err(error),
                            };
                        }
                        Err(_) => {
                            // Снятие префиксов не помогло — падаем к нечёткой попытке.
                        }
                    }
                }
                // Попытка 3 (последняя): нечёткое совпадение — нормализация
                // отступов и пробелов, кандидат должен быть ровно один.
                let fuzzy_edits = if changed { normalized } else { edits.clone() };
                match apply_edits_fuzzy(&content, &fuzzy_edits) {
                    Ok((new_content, summary)) => {
                        let baseline = lint_baseline(ctx, &path);
                        let mut f = fs::File::create(&path).map_err(|e| {
                            ToolError::Io(format!("не удалось открыть {}: {}", path.display(), e))
                        })?;
                        f.write_all(new_content.as_bytes()).map_err(|e| {
                            ToolError::Io(format!("не удалось записать {}: {}", path.display(), e))
                        })?;
                        let mut message = format!(
                            "✅ {} в {} (нечёткое совпадение: отступы/пробелы нормализованы)",
                            summary,
                            path.display()
                        );
                        match run_lint_gate(ctx, &path, baseline, Some(&content), &new_content) {
                            Ok(None) => Ok(message),
                            Ok(Some(warning)) => {
                                message.push('\n');
                                message.push_str(&warning);
                                Ok(message)
                            }
                            Err(error) => Err(error),
                        }
                    }
                    Err(failure) => {
                        // Три попытки не помогли. При неоднозначности модель
                        // получает перечень кандидатов (с ней можно работать),
                        // в остальных случаях — исходную ошибку exact: она
                        // указывает РЕАЛЬНЫЙ ожидаемый текст, а не догадку.
                        if failure.ambiguous {
                            return Err(ToolError::NotFound(failure.message));
                        }
                        Err(ToolError::NotFound(first_err))
                    }
                }
            }
            Err(e) => Err(e), // IO/другие ошибки — без бессмысленного ретрая.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::test_util::{auth_for, tmpdir};
    use crate::contract::ToolCtx;
    use std::fs;

    #[test]
    fn read_file_numbers_lines_and_slices() {
        let d = tmpdir("fs", "read");
        let f = d.join("a.txt");
        fs::write(&f, "l1\nl2\nl3\nl4\nl5").unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let r = ReadFile
            .execute(&serde_json::json!({"path": "a.txt", "offset": 2, "limit": 2}), &ctx)
            .unwrap();
        assert!(r.contains("    2→ l2"), "формат '     N→': {}", r);
        assert!(r.contains("    3→ l3"));
        assert!(!r.contains("    1→ l1"));
        assert!(r.contains("строки 2-3"), "произошла замена формата: {}", r);
        assert!(r.contains("Продолжай с offset=4"), "hint продолжения: {}", r);
    }

    #[test]
    fn read_file_clamps_long_lines_and_announces() {
        let d = tmpdir("fs", "read_long");
        let long_line = "x".repeat(5000);
        fs::write(d.join("long.txt"), format!("{}\n{}", long_line, "short")).unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let r = ReadFile
            .execute(&serde_json::json!({"path": "long.txt"}), &ctx)
            .unwrap();
        assert!(r.contains("Строки [1] обрезаны"), "должна быть пометка обрезки: {}", r);
        assert!(!r.contains("x".repeat(2001).as_str()), "строка должна быть обрезана");
    }

    #[test]
    fn read_file_missing_returns_not_found() {
        let d = tmpdir("fs", "read_missing");
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let err = ReadFile
            .execute(&serde_json::json!({"path": "nope.txt"}), &ctx)
            .unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
    }

    #[test]
    fn read_many_files_batches_and_reports_errors() {
        let d = tmpdir("fs", "read_many");
        fs::write(d.join("one.txt"), "one").unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let r = ReadManyFiles
            .execute(
                &serde_json::json!({"paths": ["one.txt", "missing.txt"]}),
                &ctx,
            )
            .unwrap();
        assert!(r.contains("one.txt"), "первый файл прочитан: {}", r);
        assert!(r.contains("⚠️"), "ошибка второго файла видна, не тихая: {}", r);
    }

    #[test]
    fn write_file_creates_and_overwrites() {
        let d = tmpdir("fs", "write");
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let r1 = WriteFile
            .execute(&serde_json::json!({"path": "b.ts", "content": "hello"}), &ctx)
            .unwrap();
        assert!(r1.contains("создан"));
        assert_eq!(fs::read_to_string(d.join("b.ts")).unwrap(), "hello");
        let r2 = WriteFile
            .execute(&serde_json::json!({"path": "b.ts", "content": "world"}), &ctx)
            .unwrap();
        assert!(r2.contains("обновлён"));
        assert_eq!(fs::read_to_string(d.join("b.ts")).unwrap(), "world");
    }

    #[test]
    fn write_outside_root_requires_approval() {
        let d = tmpdir("fs", "write_outside");
        let outside = tmpdir("fs", "outside_test");
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        // Тестовый авторизатор отклоняет всё вне корня → Forbidden.
        let err = WriteFile
            .execute(
                &serde_json::json!({"path": outside.to_string_lossy().to_string(), "content": "x"}),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, ToolError::Forbidden(_)));
    }

    #[test]
    fn edit_file_exact_match_and_error_on_missing() {
        let d = tmpdir("fs", "edit");
        fs::write(d.join("c.rs"), "fn a(){}\nfn b(){}\n").unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let r = EditFile
            .execute(
                &serde_json::json!({"path": "c.rs", "old_string": "fn b(){}", "new_string": "fn b2(){}"}),
                &ctx,
            )
            .unwrap();
        assert!(r.contains("Замена выполнена"));
        let content = fs::read_to_string(d.join("c.rs")).unwrap();
        assert!(content.contains("fn b2(){}"));
        assert!(content.contains("fn a(){}"));
        // Не найдено — НЕ молчим.
        let err = EditFile
            .execute(
                &serde_json::json!({"path": "c.rs", "old_string": "zzz", "new_string": "x"}),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
    }

    // ── Линт-гейт: правка, сломавшая код, откатывается ──

    /// Тестовый хук: возвращает заранее заданный ответ.
    struct StubGate {
        problems: Vec<crate::contract::LintProblem>,
        fail: Option<String>,
    }

    impl crate::contract::LintGate for StubGate {
        fn snapshot(
            &self,
            _path: &Path,
        ) -> Result<Option<Vec<crate::contract::LintProblem>>, String> {
            match &self.fail {
                Some(reason) => Err(reason.clone()),
                None => Ok(Some(Vec::new())),
            }
        }
        fn check(
            &self,
            _path: &Path,
            _baseline: &[crate::contract::LintProblem],
        ) -> Result<Option<Vec<crate::contract::LintProblem>>, String> {
            match &self.fail {
                Some(reason) => Err(reason.clone()),
                None => Ok(Some(self.problems.clone())),
            }
        }
    }

    /// ГЕЙТ Г4: новая синтаксическая ошибка → запись откатывается, ошибка
    /// содержит окружение строк.
    #[test]
    fn lint_gate_rolls_back_a_broken_write() {
        let d = tmpdir("fs", "lint_rollback");
        fs::write(d.join("a.rs"), "fn ok() {}\n").unwrap();
        let gate = StubGate {
            problems: vec![crate::contract::LintProblem {
                line: 1,
                message: "expected `;`".to_string(),
            }],
            fail: None,
        };
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth).with_lint_gate(&gate);
        let err = WriteFile
            .execute(
                &serde_json::json!({"path": "a.rs", "content": "fn broken(\n"}),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, ToolError::Lint(_)), "отказ гейта — свой класс: {err}");
        let text = err.to_string();
        assert!(text.contains("expected `;`"), "сообщение об ошибке: {text}");
        assert!(text.contains("1→ fn broken("), "окружение строк показано: {text}");
        assert_eq!(
            fs::read_to_string(d.join("a.rs")).unwrap(),
            "fn ok() {}\n",
            "запись откатилась к прежнему содержимому"
        );
    }

    /// ГЕЙТ Г4: хук не задан → запись проходит (и это не «тихий пропуск» —
    /// отсутствие гейта объявлено в описании тула).
    #[test]
    fn write_without_lint_gate_succeeds() {
        let d = tmpdir("fs", "lint_absent");
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let r = WriteFile
            .execute(&serde_json::json!({"path": "b.rs", "content": "fn ok() {}\n"}), &ctx)
            .unwrap();
        assert!(r.contains("создан"), "запись состоялась: {r}");
        assert_eq!(fs::read_to_string(d.join("b.rs")).unwrap(), "fn ok() {}\n");
    }

    /// Сбой проверки (сервер не ответил) — запись остаётся, но сбой виден.
    #[test]
    fn lint_gate_failure_keeps_the_write_but_reports_it() {
        let d = tmpdir("fs", "lint_failed");
        let gate = StubGate {
            problems: Vec::new(),
            fail: Some("сервер не запущен".to_string()),
        };
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth).with_lint_gate(&gate);
        let r = WriteFile
            .execute(&serde_json::json!({"path": "c.rs", "content": "fn ok() {}\n"}), &ctx)
            .unwrap();
        assert!(r.contains("не выполнена"), "сбой проверки виден в ответе: {r}");
        assert!(r.contains("сервер не запущен"), "причина сбоя названа: {r}");
        assert_eq!(
            fs::read_to_string(d.join("c.rs")).unwrap(),
            "fn ok() {}\n",
            "запись осталась на месте"
        );
    }

    /// Правка через edit_file тоже проходит через гейт и откатывается.
    #[test]
    fn lint_gate_rolls_back_a_broken_edit() {
        let d = tmpdir("fs", "lint_edit_rollback");
        fs::write(d.join("d.rs"), "fn alpha() {}\n").unwrap();
        let gate = StubGate {
            problems: vec![crate::contract::LintProblem {
                line: 1,
                message: "unexpected token".to_string(),
            }],
            fail: None,
        };
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth).with_lint_gate(&gate);
        let err = EditFile
            .execute(
                &serde_json::json!({
                    "path": "d.rs",
                    "edits": [{"oldText": "fn alpha() {}", "newText": "fn alpha( {"}]
                }),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, ToolError::Lint(_)), "правка отклонена: {err}");
        assert_eq!(
            fs::read_to_string(d.join("d.rs")).unwrap(),
            "fn alpha() {}\n",
            "правка откатилась"
        );
    }

    // ── Третья попытка правки: нечёткое совпадение (отступы/пробелы) ──

    /// Отступ результата не задваивается: отступ первой строки остаётся в
    /// файле, а вставляемый текст приходит без своего отступа.
    #[test]
    fn fuzzy_edit_does_not_double_the_indent() {
        let d = tmpdir("fs", "fuzzy_indent_once");
        fs::write(
            d.join("e.rs"),
            "fn main() {\n    if true {\n        let value = 1;\n    }\n}\n",
        )
        .unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        EditFile
            .execute(
                &serde_json::json!({
                    "path": "e.rs",
                    "edits": [{
                        "oldText": "let value   =   1;",
                        "newText": "let value = 42;"
                    }]
                }),
                &ctx,
            )
            .unwrap();
        let content = fs::read_to_string(d.join("e.rs")).unwrap();
        assert!(
            content.contains("        let value = 42;"),
            "отступ остался прежним, а не удвоился: {content}"
        );
        assert!(
            !content.contains("                let value"),
            "задвоенного отступа быть не должно: {content}"
        );
    }

    /// ГЕЙТ Г4: точный miss → нечёткий hit (кандидат один) → правка применена.
    #[test]
    fn fuzzy_edit_applies_when_indent_differs() {
        let d = tmpdir("fs", "fuzzy_hit");
        fs::write(
            d.join("a.rs"),
            "fn main() {\n    let value = 1;\n    println!(\"{value}\");\n}\n",
        )
        .unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        // Образец без отступа — в файле он с четырьмя пробелами.
        let r = EditFile
            .execute(
                &serde_json::json!({
                    "path": "a.rs",
                    "edits": [{
                        "oldText": "let value = 1;\nprintln!(\"{value}\");",
                        "newText": "let value = 2;\nprintln!(\"{value}\");"
                    }]
                }),
                &ctx,
            )
            .unwrap();
        assert!(r.contains("нечёткое совпадение"), "отчёт о попытке: {r}");
        let content = fs::read_to_string(d.join("a.rs")).unwrap();
        assert!(content.contains("let value = 2;"), "правка применена: {content}");
    }

    /// ГЕЙТ Г4: два кандидата → честный отказ с перечнем, файл НЕ изменён.
    ///
    /// Образец отличается от файла внутренними пробелами: иначе точный поиск
    /// нашёл бы первое вхождение, и нечёткая попытка просто не включалась бы.
    #[test]
    fn fuzzy_edit_refuses_when_candidates_are_ambiguous() {
        let d = tmpdir("fs", "fuzzy_ambiguous");
        let source = "fn a() {\n        let v   =   1;\n}\nfn b() {\n        let v   =   1;\n}\n";
        fs::write(d.join("b.rs"), source).unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let err = EditFile
            .execute(
                &serde_json::json!({
                    "path": "b.rs",
                    "edits": [{
                        "oldText": "let v = 1;",
                        "newText": "let v = 2;"
                    }]
                }),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)), "неоднозначность — отказ: {err}");
        let text = err.to_string();
        assert!(text.contains("неоднозначно"), "причина названа: {text}");
        assert!(text.contains("кандидатов 2"), "число кандидатов названо: {text}");
        let content = fs::read_to_string(d.join("b.rs")).unwrap();
        assert_eq!(content, source, "файл не тронут");
    }

    /// Отступ замены пересчитывается под найденный фрагмент.
    #[test]
    fn fuzzy_edit_reindents_the_replacement() {
        let d = tmpdir("fs", "fuzzy_reindent");
        fs::write(
            d.join("c.rs"),
            "fn main() {\n    if true {\n        let x   =   1;\n    }\n}\n",
        )
        .unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        EditFile
            .execute(
                &serde_json::json!({
                    "path": "c.rs",
                    "edits": [{
                        "oldText": "let x = 1;",
                        "newText": "let x = 2;\nlet y = 3;"
                    }]
                }),
                &ctx,
            )
            .unwrap();
        let content = fs::read_to_string(d.join("c.rs")).unwrap();
        assert!(
            content.contains("        let x = 2;\n        let y = 3;"),
            "обе строки на отступе найденного фрагмента: {content}"
        );
    }

    /// Нечёткий путь не мешает обычным правкам: exact по-прежнему главный.
    #[test]
    fn exact_match_still_wins_over_fuzzy() {
        let d = tmpdir("fs", "fuzzy_exact_first");
        fs::write(d.join("d.rs"), "fn a() { let v = 1; }\n").unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let r = EditFile
            .execute(
                &serde_json::json!({
                    "path": "d.rs",
                    "edits": [{"oldText": "let v = 1;", "newText": "let v = 9;"}]
                }),
                &ctx,
            )
            .unwrap();
        assert!(!r.contains("нечёткое"), "точное совпадение не должно трактоваться как нечёткое: {r}");
    }

    #[test]
    fn edit_file_replace_all() {
        let d = tmpdir("fs", "edit_all");
        fs::write(d.join("d.txt"), "aXaXa").unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        EditFile
            .execute(
                &serde_json::json!({"path": "d.txt", "old_string": "X", "new_string": "Y", "replace_all": true}),
                &ctx,
            )
            .unwrap();
        assert_eq!(fs::read_to_string(d.join("d.txt")).unwrap(), "aYaYa");
    }

    #[test]
    fn edit_file_multi_edits_atomic() {
        let d = tmpdir("fs", "edit_multi");
        fs::write(d.join("m.rs"), "fn a(){}\nfn b(){}\nfn c(){}\n").unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let r = EditFile
            .execute(
                &serde_json::json!({"path": "m.rs", "edits": [
                    {"oldText": "fn a(){}", "newText": "fn a2(){}"},
                    {"oldText": "fn c(){}", "newText": "fn c2(){}"}
                ]}),
                &ctx,
            )
            .unwrap();
        assert!(r.contains("Заменено 2"));
        let content = fs::read_to_string(d.join("m.rs")).unwrap();
        assert!(content.contains("fn a2(){}"));
        assert!(content.contains("fn b(){}"));
        assert!(content.contains("fn c2(){}"));
        assert!(!content.contains("fn a(){}"));
    }

    #[test]
    fn edit_file_multi_edits_do_not_partially_write_on_failure() {
        let d = tmpdir("fs", "edit_multi_fail");
        fs::write(d.join("n.rs"), "fn a(){}\nfn b(){}\n").unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let err = EditFile
            .execute(
                &serde_json::json!({"path": "n.rs", "edits": [
                    {"oldText": "fn a(){}", "newText": "fn a2(){}"},
                    {"oldText": "ZZZ_NOPE", "newText": "x"}
                ]}),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
        // Атомарность: первая правка НЕ применена, файл не изменён.
        let content = fs::read_to_string(d.join("n.rs")).unwrap();
        assert!(content.contains("fn a(){}"), "частичная запись при провале!");
        assert!(!content.contains("fn a2(){}"));
    }

    #[test]
    fn edit_file_strips_line_number_prefixes_on_retry() {
        let d = tmpdir("fs", "edit_retry");
        fs::write(d.join("r.rs"), "fn foo() {\n    return 1;\n}\n").unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        // oldText скопирован из вывода read_file (формат '     N→').
        let r = EditFile
            .execute(
                &serde_json::json!({"path": "r.rs", "edits": [
                    {"oldText": "    2→     return 1;", "newText": "    2→     return 2;"}
                ]}),
                &ctx,
            )
            .unwrap();
        assert!(r.contains("сняты"), "должен сработать ретрай со strip: {}", r);
        let content = fs::read_to_string(d.join("r.rs")).unwrap();
        assert!(content.contains("return 2;"), "замена должна примениться: {}", content);
        assert!(!content.contains("2→"), "новый текст с префиксом — ненормализован");
    }

    #[test]
    fn edit_file_legacy_prefix_style_also_stripped() {
        let d = tmpdir("fs", "edit_retry_legacy");
        fs::write(d.join("s.rs"), "alpha\nbeta\n").unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let r = EditFile
            .execute(
                &serde_json::json!({"path": "s.rs", "edits": [
                    {"oldText": "   2 | beta", "newText": "   2 | gamma"}
                ]}),
                &ctx,
            )
            .unwrap();
        assert!(r.contains("сняты"));
        assert!(fs::read_to_string(d.join("s.rs")).unwrap().contains("gamma"));
    }

    #[test]
    fn edit_file_does_not_strip_mixed_content() {
        let d = tmpdir("fs", "edit_nostrip");
        fs::write(d.join("t.txt"), "hello world\n").unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        // Одна строка префиксная, другая нет → весь блок НЕ нормализуется.
        let err = EditFile
            .execute(
                &serde_json::json!({"path": "t.txt", "edits": [
                    {"oldText": "hello world\nextra non-prefixed line", "newText": "X"}
                ]}),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
        assert_eq!(fs::read_to_string(d.join("t.txt")).unwrap(), "hello world\n");
    }

    #[test]
    fn edit_file_requires_edits_or_legacy() {
        let d = tmpdir("fs", "edit_require");
        fs::write(d.join("u.txt"), "x").unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let err = EditFile
            .execute(&serde_json::json!({"path": "u.txt"}), &ctx)
            .unwrap_err();
        assert!(matches!(err, ToolError::Usage(_)));
        let err2 = EditFile
            .execute(
                &serde_json::json!({"path": "u.txt", "old_string": "x", "new_string": "y", "edits": []}),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err2, ToolError::Usage(_)));
    }

    #[test]
    fn edit_file_empty_oldText_rejected_in_edits() {
        let d = tmpdir("fs", "edit_empty");
        fs::write(d.join("v.txt"), "x").unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let err = EditFile
            .execute(
                &serde_json::json!({"path": "v.txt", "edits": [{"oldText": "", "newText": "y"}]}),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, ToolError::Usage(_)));
    }

    #[test]
    fn read_guard_rejects_windows_device_names() {
        for name in ["NUL", "NUL.txt", "CON", "PRN", "AUX", "COM1", "LPT3", "CLOCK$"] {
            let err = guard_read_path(Path::new(name)).unwrap_err();
            assert!(matches!(err, ToolError::Forbidden(_)), "{} должен быть device", name);
        }
        // Обычный файл — проходит.
        let d = tmpdir("fs", "device");
        let f = d.join("data.txt");
        fs::write(&f, "hi").unwrap();
        assert!(guard_read_path(&f).is_ok());
    }

    #[test]
    fn read_guard_rejects_binary_extension() {
        let d = tmpdir("fs", "bin_ext");
        let f = d.join("image.png");
        fs::write(&f, "not really png").unwrap();
        let err = guard_read_path(&f).unwrap_err();
        assert!(matches!(err, ToolError::Forbidden(_)));
    }

    #[test]
    fn read_file_rejects_binary_sniff() {
        let d = tmpdir("fs", "bin_sniff");
        // NUL-байт без бинарного расширения.
        fs::write(d.join("payload"), b"abc\x00def").unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let err = ReadFile
            .execute(&serde_json::json!({"path": "payload"}), &ctx)
            .unwrap_err();
        assert!(matches!(err, ToolError::Forbidden(_)));
        // Высокая доля управляющих символов.
        fs::write(d.join("ctrl.dat"), [0x01u8; 40]).unwrap();
        let err2 = ReadFile
            .execute(&serde_json::json!({"path": "ctrl.dat"}), &ctx)
            .unwrap_err();
        assert!(matches!(err2, ToolError::Forbidden(_)));
    }

    #[test]
    fn read_accepts_text_with_whitespace_controls() {
        let d = tmpdir("fs", "text_ok");
        fs::write(d.join("ok.ts"), "fn a() {\n\treturn 1;\n}\r\n").unwrap();
        let auth = auth_for(&d);
        let ctx = ToolCtx::new(&d, &auth);
        let r = ReadFile
            .execute(&serde_json::json!({"path": "ok.ts"}), &ctx)
            .unwrap();
        assert!(r.contains("fn a() {"), "обычный текст читается: {}", r);
    }
}
