//! Установка FFmpeg: проверка наличия, автоскачивание официальной сборки
//! (BtbN FFmpeg-Builds, GitHub Releases latest), извлечение ffmpeg.exe +
//! ffprobe.exe, верификация запуском `-version`.
//!
//! Каталог установки передаёт хост (нейтральность: плагин не знает хранилища).
//! `ensure_ffmpeg` — единственная точка «есть ли движок»: глобальный Mutex
//! от параллельных вызовов + повторная проверка после ожидания (паттерн
//! `ensure_mmproj_for_model` из llama-engine). Отказ без движка невозможен:
//! каждый спавн идёт после ensure.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// Имя папки движка внутри корня, переданного хостом.
pub const FFMPEG_DIR_NAME: &str = "ffmpeg";

/// Официальные сборки BtbN (github.com/BtbN/FFmpeg-Builds), latest master,
/// GPL. ZIP содержит `bin/ffmpeg.exe` и `bin/ffprobe.exe` (+ ffplay/doc).
/// URL проверен `curl -I` 10.11.26 (HTTP 200).
const URL_WINDOWS: &str =
    "https://github.com/BtbN/FFmpeg-Builds/releases/latest/download/ffmpeg-master-latest-win64-gpl.zip";

/// Гейт zip: свежая win64-gpl сборка ~100 МБ; меньше 40 МБ — не сборка.
const MIN_ZIP_SIZE: u64 = 40 * 1024 * 1024;

/// Гейт exe: ffmpeg.exe/ffprobe.exe в сборке больше 10 МБ (защита от HTML-заглушки).
const MIN_EXE_SIZE: u64 = 10 * 1024 * 1024;

const DOWNLOAD_RETRIES: u32 = 3;

pub fn ffmpeg_dir(root: &Path) -> PathBuf {
    root.join(FFMPEG_DIR_NAME)
}

pub fn ffmpeg_exe(dir: &Path) -> PathBuf {
    dir.join("bin").join(exe_name("ffmpeg"))
}

pub fn ffprobe_exe(dir: &Path) -> PathBuf {
    dir.join("bin").join(exe_name("ffprobe"))
}

fn exe_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

/// Движок установлен? Оба exe существуют и не подозрительно малы.
pub fn is_installed(dir: &Path) -> bool {
    [ffmpeg_exe(dir), ffprobe_exe(dir)].into_iter().all(|exe| {
        fs::metadata(&exe)
            .map(|m| m.len() > MIN_EXE_SIZE)
            .unwrap_or(false)
    })
}

/// Первая строка `-version` (человекочитаемая идентификация сборки).
pub fn tool_version(exe: &Path) -> Result<String, String> {
    let out = super::runner::run_tool(exe, &["-version".to_string()], Duration::from_secs(30))?;
    if !out.success {
        return Err(format!(
            "{} -version завершился с ошибкой (code {:?}): {}",
            exe.display(),
            out.exit_code,
            out.stderr_tail
        ));
    }
    Ok(out
        .stdout
        .lines()
        .next()
        .unwrap_or("версия неизвестна")
        .trim()
        .to_string())
}

/// Гейт установки: глобальный Mutex — два параллельных ensure не скачают архив
/// дважды; после ожидания блокировки — re-check (движок мог уже поставиться).
pub fn ensure_ffmpeg(dir: &Path, on_log: &dyn Fn(String)) -> Result<(), String> {
    static GATE: OnceLock<Mutex<()>> = OnceLock::new();
    let gate = GATE.get_or_init(|| Mutex::new(()));
    let _guard = gate
        .lock()
        .map_err(|e| format!("Блокировка установки FFmpeg: {e}"))?;

    if is_installed(dir) {
        return Ok(());
    }
    on_log(format!(
        "FFmpeg не найден в {} — скачиваю официальную сборку (один раз)",
        dir.display()
    ));
    install(dir, on_log)
}

fn install(dir: &Path, on_log: &dyn Fn(String)) -> Result<(), String> {
    if !cfg!(windows) {
        // Честный отказ вместо тихого поломанного пути (§2.2).
        let msg = "Автоустановка FFmpeg реализована для Windows. Установите ffmpeg и ffprobe в каталог, переданный хостом, вручную.";
        log::error!("{msg}");
        return Err(msg.to_string());
    }

    fs::create_dir_all(dir).map_err(|e| {
        let msg = format!("Не удалось создать папку {}: {e}", dir.display());
        log::error!("{msg}");
        msg
    })?;
    let bin_dir = dir.join("bin");
    fs::create_dir_all(&bin_dir).map_err(|e| {
        let msg = format!("Не удалось создать папку {}: {e}", bin_dir.display());
        log::error!("{msg}");
        msg
    })?;

    let zip_path = dir.join("ffmpeg-build.zip");
    let mut last_err: Option<String> = None;

    for attempt in 1..=DOWNLOAD_RETRIES {
        on_log(format!("Скачивание FFmpeg… (попытка {attempt}/{DOWNLOAD_RETRIES})"));
        let step = download_and_extract(&zip_path, &bin_dir, on_log).and_then(|_| verify(dir));
        match step {
            Ok(version) => {
                let _ = fs::remove_file(&zip_path);
                on_log(format!("FFmpeg установлен: {version}"));
                return Ok(());
            }
            Err(e) => {
                log::warn!("Попытка {attempt}/{DOWNLOAD_RETRIES} установки FFmpeg не удалась: {e}");
                last_err = Some(e);
                let _ = fs::remove_file(&zip_path);
                if attempt < DOWNLOAD_RETRIES {
                    std::thread::sleep(Duration::from_secs(2 * attempt as u64));
                }
            }
        }
    }

    let msg = last_err.unwrap_or_else(|| "Не удалось установить FFmpeg".to_string());
    Err(msg)
}

fn download_and_extract(
    zip_path: &Path,
    bin_dir: &Path,
    on_log: &dyn Fn(String),
) -> Result<(), String> {
    // download_blocking: логи прогресса идут через log::info («[downloader] …»)
    // → вкладка «Логи» хоста; колбэк там игнорируется по контракту API.
    tauri_plugin_downloader::download_blocking(
        URL_WINDOWS,
        zip_path,
        tauri_plugin_downloader::DownloadOptions {
            label: "FFmpeg".into(),
            kind: "bin".into(),
            min_size: Some(MIN_ZIP_SIZE),
            // Магия ZIP: первые байты "PK\x03\x04" — отсекает HTML-заглушки.
            magic: Some(b"PK\x03\x04".to_vec()),
            ..Default::default()
        },
        None,
    )?;
    extract_executables(zip_path, bin_dir, on_log)
}

/// Извлечь ffmpeg/ffprobe из архива (любая глубина вложенных папок —
/// в сборке BtbN это `<variant>/bin/*.exe`). ffplay и доки не нужны.
pub(crate) fn extract_executables(
    zip_path: &Path,
    bin_dir: &Path,
    on_log: &dyn Fn(String),
) -> Result<(), String> {
    let file = fs::File::open(zip_path).map_err(|e| format!("Не удалось открыть zip: {e}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("Битый zip-архив: {e}"))?;

    let want = [exe_name("ffmpeg"), exe_name("ffprobe")];
    let mut extracted: Vec<PathBuf> = Vec::new();

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("Чтение архива: {e}"))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        let base = name.rsplit(['/', '\\']).next().unwrap_or(&name).to_string();
        if !want.contains(&base) {
            continue;
        }
        let dest = bin_dir.join(&base);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                format!("Не удалось создать папку {}: {e}", parent.display())
            })?;
        }
        let mut out = fs::File::create(&dest)
            .map_err(|e| format!("Не удалось создать {}: {e}", dest.display()))?;
        std::io::copy(&mut entry, &mut out)
            .map_err(|e| format!("Распаковка {}: {e}", dest.display()))?;
        extracted.push(dest);
    }

    if extracted.len() < want.len() {
        let msg = format!(
            "В архиве не найдены все тулы (нужно {:?}, извлечено {} файлов)",
            want,
            extracted.len()
        );
        log::error!("{msg}");
        return Err(msg);
    }
    on_log(format!(
        "Распаковано в {}: {}",
        bin_dir.display(),
        extracted
            .iter()
            .map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ));
    Ok(())
}

/// Верификация установки: оба exe существуют, достаточно большие, запускаются.
fn verify(dir: &Path) -> Result<String, String> {
    let ff = ffmpeg_exe(dir);
    let fp = ffprobe_exe(dir);
    for exe in [&ff, &fp] {
        let meta = fs::metadata(exe).map_err(|e| {
            format!("{} не появился после распаковки: {e}", exe.display())
        })?;
        if meta.len() <= MIN_EXE_SIZE {
            return Err(format!(
                "{} подозрительно мал ({} байт) — сборка битая",
                exe.display(),
                meta.len()
            ));
        }
    }
    let version = tool_version(&ff)?;
    tool_version(&fp).map_err(|e| format!("ffprobe не запускается: {e}"))?;
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ffmpeg_dir_joins_root() {
        assert_eq!(
            ffmpeg_dir(Path::new("D:\\Data")),
            PathBuf::from("D:\\Data\\ffmpeg")
        );
    }

    #[test]
    fn exe_paths_live_in_bin() {
        let dir = Path::new("D:\\Data\\ffmpeg");
        let ff = ffmpeg_exe(dir);
        assert!(ff.starts_with(dir.join("bin")));
        assert!(ff.file_name().unwrap().to_string_lossy().contains("ffmpeg"));
        let fp = ffprobe_exe(dir);
        assert!(fp.file_name().unwrap().to_string_lossy().contains("ffprobe"));
    }

    #[test]
    fn is_installed_false_on_empty_dir() {
        let tmp = std::env::temp_dir().join("ffmpeg_plugin_test_empty");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        assert!(!is_installed(&tmp));
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn extract_pulls_two_exes_from_nested_zip() {
        // Мини-архив с layout'ом BtbN: <variant>/bin/ffmpeg.exe + ffprobe.exe.
        let tmp = std::env::temp_dir().join("ffmpeg_plugin_test_extract");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let zip_path = tmp.join("build.zip");

        let file = fs::File::create(&zip_path).unwrap();
        let mut zw = zip::ZipWriter::new(file);
        let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
        zw.add_directory("ffmpeg-master-latest-win64-gpl/doc", opts)
            .unwrap();
        zw.start_file("ffmpeg-master-latest-win64-gpl/bin/ffmpeg.exe", opts)
            .unwrap();
        std::io::Write::write_all(&mut zw, b"FAKE-FFMPEG-BINARY").unwrap();
        zw.start_file("ffmpeg-master-latest-win64-gpl/bin/ffprobe.exe", opts)
            .unwrap();
        std::io::Write::write_all(&mut zw, b"FAKE-FFPROBE-BINARY").unwrap();
        zw.start_file("ffmpeg-master-latest-win64-gpl/LICENSE.txt", opts)
            .unwrap();
        std::io::Write::write_all(&mut zw, b"license").unwrap();
        zw.finish().unwrap();

        let bin_dir = tmp.join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        extract_executables(&zip_path, &bin_dir, &|_| {}).expect("извлечение должно succeed");

        assert!(bin_dir.join(exe_name("ffmpeg")).exists());
        assert!(bin_dir.join(exe_name("ffprobe")).exists());
        assert!(
            !bin_dir.join("LICENSE.txt").exists(),
            "лишние файлы из архива не должны попадать в bin/"
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn extract_fails_honestly_when_no_exes() {
        let tmp = std::env::temp_dir().join("ffmpeg_plugin_test_extract_empty");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let zip_path = tmp.join("empty.zip");

        let file = fs::File::create(&zip_path).unwrap();
        let mut zw = zip::ZipWriter::new(file);
        let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
        zw.start_file("readme.txt", opts).unwrap();
        std::io::Write::write_all(&mut zw, b"no binaries here").unwrap();
        zw.finish().unwrap();

        let bin_dir = tmp.join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let err = extract_executables(&zip_path, &bin_dir, &|_| {})
            .expect_err("без exe в архиве обязана быть честная ошибка");
        assert!(err.contains("не найдены"), "сообщение: {err}");

        let _ = fs::remove_dir_all(&tmp);
    }
}
