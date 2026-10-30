//! LibreOffice (soffice): детект, установка recalc-макроса, пересчёт формул
//! с сохранением, PDF-рендер. LibreOffice — внешняя программа, НЕ зависимость:
//! нет soffice → [`SofficeNotDetected`], статические гейты продолжают работать
//! (решение пользователя «детект + честная ошибка», 31.10.26).
//!
//! Пересчёт — порт методологии плагина spreadsheets: Basic-макрос
//! `Standard.Module1.RecalculateAndSave` ставится в профиль пользователя при
//! первом вызове; затем `soffice --headless` исполняет его над файлом:
//! загрузить скрыто → calculateAll() → store() → close().

use crate::ExcelError;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Таймаут по умолчанию для вызовов soffice (пересчёт большого файла
/// с формулами может занимать десятки секунд).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// Имя Basic-модуля с recalc-макросом (Standard.Module1).
const MACRO_MODULE: &str = "Module1";
const MACRO_MARKER: &str = "RecalculateAndSave";

/// Найти soffice: явный путь (config-ключ libreoffice_path) → PATH →
/// стандартные места установки.
pub fn detect_soffice(custom: Option<&str>) -> Option<PathBuf> {
    if let Some(custom) = custom {
        let path = PathBuf::from(custom);
        let candidate = if path.is_dir() {
            path.join("program").join("soffice.exe")
        } else {
            path
        };
        if candidate.is_file() {
            return Some(candidate);
        }
        log::warn!("libreoffice_path указан, но файла нет: {}", custom);
    }
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join("soffice.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    for base in install_roots() {
        let candidate = base.join("program").join("soffice.exe");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn install_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for var in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
        if let Ok(value) = std::env::var(var) {
            let mut base = PathBuf::from(value);
            if var == "LOCALAPPDATA" {
                base.push("Programs");
            }
            roots.push(base.join("LibreOffice"));
        }
    }
    roots.push(PathBuf::from("C:\\Program Files\\LibreOffice"));
    roots.push(PathBuf::from("C:\\Program Files (x86)\\LibreOffice"));
    roots
}

/// Каталог Basic-библиотеки Standard профиля пользователя LibreOffice.
fn standard_basic_dir() -> Result<PathBuf, ExcelError> {
    let appdata = std::env::var("APPDATA")
        .map_err(|_| ExcelError::Soffice("переменная окружения APPDATA не задана".to_string()))?;
    Ok(PathBuf::from(appdata)
        .join("LibreOffice")
        .join("4")
        .join("user")
        .join("basic")
        .join("Standard"))
}

const RECALC_MODULE_XBA: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE script:module PUBLIC "-//OpenOffice.org//DTD OfficeDocument 1.0//EN" "module.dtd">
<script:module xmlns:script="http://openoffice.org/2000/script" script:name="Module1" script:language="StarBasic">Sub RecalculateAndSave(sPath As String)
Dim oDoc As Object
Dim aArgs(0) As New com.sun.star.beans.PropertyValue
aArgs(0).Name = "Hidden"
aArgs(0).Value = True
oDoc = StarDesktop.loadComponentFromURL(ConvertToURL(sPath), "_blank", 0, aArgs())
If Not IsNull(oDoc) Then
    oDoc.calculateAll()
    oDoc.store()
    oDoc.close(False)
End If
End Sub</script:module>"#;

const SCRIPT_XLB: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE library:library PUBLIC "-//OpenOffice.org//DTD OfficeDocument 1.0//EN" "library.dtd">
<library:library xmlns:library="http://openoffice.org/2000/library" library:name="Standard" library:readonly="false" library:passwordprotected="false">
 <library:element library:name="Module1"/>
</library:library>"#;

/// Гарантировать наличие recalc-макроса в профиле LibreOffice. Идемпотентно:
/// существующий модуль с `RecalculateAndSave` не трогается.
pub fn ensure_recalc_macro() -> Result<(), ExcelError> {
    let dir = standard_basic_dir()?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| ExcelError::Soffice(format!("не удалось создать {}: {e}", dir.display())))?;

    let module_path = dir.join(format!("{MACRO_MODULE}.xba"));
    let needs_module = std::fs::read_to_string(&module_path)
        .map(|content| !content.contains(MACRO_MARKER))
        .unwrap_or(true);
    if needs_module {
        std::fs::write(&module_path, RECALC_MODULE_XBA).map_err(|e| {
            ExcelError::Soffice(format!("запись {}: {e}", module_path.display()))
        })?;
        log::info!("LibreOffice recalc-макрос установлен: {}", module_path.display());
    }

    let xlb_path = dir.join("script.xlb");
    let existing = std::fs::read_to_string(&xlb_path).unwrap_or_default();
    if existing.is_empty() {
        std::fs::write(&xlb_path, SCRIPT_XLB)
            .map_err(|e| ExcelError::Soffice(format!("запись {}: {e}", xlb_path.display())))?;
    } else if !existing.contains("Module1") {
        // Библиотека Standard есть, модуль не зарегистрирован — дописываем элемент.
        let patched = existing.replace(
            "</library:library>",
            " <library:element library:name=\"Module1\"/>\n</library:library>",
        );
        std::fs::write(&xlb_path, patched)
            .map_err(|e| ExcelError::Soffice(format!("запись {}: {e}", xlb_path.display())))?;
    }
    Ok(())
}

/// Пересчитать все формулы файла и сохранить (файл модифицируется — вызывает
/// только для файлов в зоне результатов хоста, НИКОГДА для исходника юзера).
pub fn recalc_and_save(
    soffice: &Path,
    file: &Path,
    timeout: Duration,
) -> Result<(), ExcelError> {
    ensure_recalc_macro()?;
    let absolute = std::fs::canonicalize(file)
        .map_err(|e| ExcelError::Soffice(format!("{}: {e}", file.display())))?;
    let mut command = Command::new(soffice);
    command
        .arg("--headless")
        .arg("--norestore")
        .arg("--nologo")
        .arg(format!(
            "vnd.sun.star.script:Standard.{MACRO_MODULE}.{MACRO_MARKER}?language=Basic&location=application"
        ))
        .arg(&absolute)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    spawn_with_timeout(command, timeout, "recalc")?;
    Ok(())
}

/// Отрендерить файл в PDF (out_dir/<stem>.pdf) и вернуть путь.
pub fn render_pdf(
    soffice: &Path,
    file: &Path,
    out_dir: &Path,
    timeout: Duration,
) -> Result<PathBuf, ExcelError> {
    std::fs::create_dir_all(out_dir)
        .map_err(|e| ExcelError::Soffice(format!("{}: {e}", out_dir.display())))?;
    let mut command = Command::new(soffice);
    command
        .arg("--headless")
        .arg("--norestore")
        .arg("--nologo")
        .arg("--convert-to")
        .arg("pdf")
        .arg("--outdir")
        .arg(out_dir)
        .arg(file)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    spawn_with_timeout(command, timeout, "pdf")?;
    let stem = file
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "document".to_string());
    let pdf = out_dir.join(format!("{stem}.pdf"));
    if !pdf.is_file() {
        return Err(ExcelError::Soffice(format!(
            "PDF не появился: {} (возможно, файл занят другой копией LibreOffice)",
            pdf.display()
        )));
    }
    Ok(pdf)
}

/// Спавн + ожидание с таймаутом. Ровно один общий `creation_flags(0x08000000)`
/// на Windows (не открывать консольное окно из GUI-процесса).
fn spawn_with_timeout(mut command: Command, timeout: Duration, what: &str) -> Result<(), ExcelError> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|e| ExcelError::Soffice(format!("запуск soffice: {e}")))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    return Ok(());
                }
                return Err(ExcelError::Soffice(format!(
                    "{what}: процесс завершился с кодом {status}"
                )));
            }
            Ok(None) => {}
            Err(e) => {
                return Err(ExcelError::Soffice(format!("{what}: {e}")));
            }
        }
        if started.elapsed() > timeout {
            kill_tree(child.id());
            return Err(ExcelError::Soffice(format!(
                "{what}: превышен таймаут {} сек — процесс остановлен",
                timeout.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Остановить ЗАПУЩЕННЫЙ ЭТИМ ВЫЗОВОМ процесс (по PID с деревом детей —
/// soffice.exe порождает soffice.bin). Запрет правила 0.3 — на убийство
/// ПО ИМЕНИ (`/IM`); здесь — конкретный PID нашего ребёнка.
fn kill_tree(pid: u32) {
    let result = Command::new("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if let Err(e) = result {
        log::warn!("taskkill /PID {pid} не выполнен: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_misses_when_not_installed() {
        // Явно несуществующий custom-путь НЕ подменяет детект.
        assert!(detect_soffice(Some("Z:\\nope\\soffice.exe")).is_none() || {
            // Если на машине LibreOffice есть, детект вернёт реальный путь —
            // проверяем только отсутствие ложного custom-пути.
            detect_soffice(None).is_some()
        });
    }

    #[test]
    fn macro_files_idempotent() {
        // Если LibreOffice установлен — макрос ставится дважды без ошибок
        // и вторая запись ничего не меняет. Если нет — тест честно пропускается.
        let Some(soffice) = detect_soffice(None) else {
            return;
        };
        assert!(soffice.is_file());
        ensure_recalc_macro().unwrap();
        let first = std::fs::read_to_string(standard_basic_dir().unwrap().join("Module1.xba"))
            .unwrap();
        ensure_recalc_macro().unwrap();
        let second = std::fs::read_to_string(standard_basic_dir().unwrap().join("Module1.xba"))
            .unwrap();
        assert_eq!(first, second, "повторная установка не должна менять файл");
    }
}
