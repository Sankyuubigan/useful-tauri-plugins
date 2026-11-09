//! Чтение файлов: read_file (одиночное, с проверкой границ), read_many_files
//! (пакет). Гарды чтения: устройства Windows, бинарное расширение, бинарный
//! снифф содержимого.

use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::contract::{CodeTool, ToolCtx, ToolError, resolve_path, truncate};

use super::{arg_int, arg_str};

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
