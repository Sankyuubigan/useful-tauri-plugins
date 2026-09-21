//! Установка / обновление 9router БЕЗ системных зависимостей.
//!
//! Всё скачивается и распаковывается в папку `<router_dir>` (по умолчанию
//! `<exe>/9router`), терминал и админ-права не нужны:
//!
//! ```text
//! <exe>/9router/
//! ├─ runtime/node.exe          ← портативный Node.js (zip c nodejs.org, LTS)
//! └─ dist/app/...              ← standalone 9router (tgz c registry.npmjs.org)
//!    ├─ custom-server.js
//!    └─ node_modules/          ← bundled (sql.js) — видна через NODE_PATH
//! ```
//!
//! SQLite для 9router: на Node >= 22 используется встроенный `node:sqlite`
//! (C-код внутри node.exe), поэтому никакого `npm install` и сборки native
//! модулей не требуется.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::blocking::Client;
use tauri::AppHandle;

use crate::router::config::{dist_dir, node_exe};

/// Прогресс-колбэк: (stage, скачано_байт, всего_байт, текст).
pub type ProgressFn = Box<dyn Fn(&str, u64, u64, &str) + Send>;

/// Ближайшая версия 9router из npm registry.
pub fn latest_npm_version(client: &Client) -> Result<String, String> {
    let url = "https://registry.npmjs.org/9router/latest";
    let resp = client.get(url).timeout(Duration::from_secs(15)).send()
        .map_err(|e| format!("Не удалось получить версию 9router: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("npm registry: HTTP {}", resp.status()));
    }
    let v: serde_json::Value = resp.json().map_err(|e| format!("Bad JSON от npm: {}", e))?;
    v.get("version")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "npm registry: нет поля version".to_string())
}

/// Вся установка 9router (блокирующая; вызывать из spawn_blocking).
///
/// Шаги:
/// 1. Резолвим версии (9router + Node LTS) с hot-cache по конфигу.
/// 2. Качаем tgz 9router → распаковываем в `dist/`.
/// 3. Качаем zip Node → достаём `node.exe` → кладём в `runtime/`.
/// 4. Обновляем `nine_router` в app_config.json.
pub fn install_or_update(
    app: &AppHandle,
    force: bool,
    progress: ProgressFn,
) -> Result<InstalledInfo, String> {
    let cfg = crate::router::config::load_config(app);
    let dir = crate::router::config::router_dir(app);

    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP client: {}", e))?;
    let mut cfg = cfg;

    // Сначала резолвим версию и проверяем актуальность БЕЗ side-эффектов:
    // не трогаем папку и не глушим работающий сервер, если обновлять нечего.
    let npm_ver = latest_npm_version(&client)?;
    let installed_ok = cfg.installed_version.as_deref() == Some(npm_ver.as_str())
        && node_exe(&dir).exists()
        && server_script_exists(&dir);
    if installed_ok {
        progress("installed", 0, 0, &format!("9router v{} уже установлен", npm_ver));
        log::info!("9router v{} уже установлен", npm_ver);
        return Ok(InstalledInfo::current(npm_ver, cfg.node_version.clone().unwrap_or_default()));
    }

    // Реальная установка/обновление/починка: останавливаем работающий сервер,
    // иначе перезапись node.exe/dist падает с «файл занят другим процессом»
    // (os error 32). Глушим и зарегистрированные PID, и «осиротевшие» node.exe
    // по целевому пути (могут жить после рестарта приложения).
    crate::router::process::stop_server();
    crate::router::process::kill_node_processes(&node_exe(&dir));

    fs::create_dir_all(dir.join("runtime"))
        .map_err(|e| format!("Не удалось создать папку установки: {}", e))?;

    // ── 1. Node.js (portable, LTS) ──
    let node_ver = resolve_node_version(&client)?;
    let node = node_exe(&dir);
    if !node.exists() || force {
        progress("node", 0, 0, "Скачиваем портативный Node.js...");
        let zip_bytes = download_bytes(&client, &node_zip_url(&node_ver))?;
        extract_node_zip(&zip_bytes, &node)?;
    }
    if !node.exists() {
        return Err("node.exe не найден после распаковки".to_string());
    }

    // ── 2. 9router (npm tgz) ──
    progress("router", 0, 0, &format!("Скачиваем 9router v{}...", npm_ver));
    let tgz = download_bytes(&client, &npm_tgz_url(&npm_ver))?;
    let dist = dist_dir(&dir);
    if dist.exists() {
        fs::remove_dir_all(&dist).map_err(|e| format!("Не удалось обновить dist: {}", e))?;
    }
    extract_tgz_strip_package(&tgz, &dist)?;

    let server = crate::router::config::server_script(&dist);
    if !server.exists() {
        return Err(format!("В tgz не найден сервер бандла: {}", server.display()));
    }

    // ── 3. Фиксируем конфиг ──
    cfg.installed_version = Some(npm_ver.clone());
    cfg.node_version = Some(node_ver.clone());
    crate::router::config::save_config(app, &cfg);

    progress("done", 0, 0, &format!("✅ 9router v{} установлен (Node {})", npm_ver, node_ver));
    log::info!("✅ 9router v{} установлен (Node {})", npm_ver, node_ver);

    Ok(InstalledInfo::current(npm_ver, node_ver))
}

/// Результат установки.
#[derive(serde::Serialize, Clone, Default)]
pub struct InstalledInfo {
    pub version: String,
    pub node_version: String,
    pub path: String,
}

impl InstalledInfo {
    fn current(version: String, node_version: String) -> Self {
        Self {
            version,
            node_version,
            path: crate::router::config::default_router_dir().to_string_lossy().to_string(),
        }
    }
}

fn server_script_exists(dir: &Path) -> bool {
    let dist = dist_dir(dir);
    let custom = dist.join("app").join("custom-server.js");
    let server = dist.join("app").join("server.js");
    custom.exists() || server.exists()
}

// ─────────────────────────────── Node.js version ───────────────────────────────

/// Резолвим latest LTS из index.json (отсортирован по убыванию). Fallback —
/// захардкоженная версия, если index.json недоступен.
fn resolve_node_version(client: &Client) -> Result<String, String> {
    let url = "https://nodejs.org/dist/index.json";
    match client.get(url).timeout(Duration::from_secs(20)).send() {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(list) = resp.json::<serde_json::Value>() {
                if let Some(arr) = list.as_array() {
                    for entry in arr {
                        // lts — либо true, либо codename-строка. Берём первую LTS.
                        if entry.get("lts").map(|l| !l.is_null() && l != false).unwrap_or(false) {
                            if let Some(v) = entry.get("version").and_then(|v| v.as_str()) {
                                return Ok(v.trim_start_matches('v').to_string());
                            }
                        }
                    }
                }
            }
            log::warn!("Node index.json распарсен неудачно — fallback на известную версию");
        }
        Ok(resp) => log::warn!("nodejs.org: HTTP {}", resp.status()),
        Err(e) => log::warn!("nodejs.org недоступен: {} — fallback на известную версию", e),
    }
    Ok("22.14.0".to_string())
}

fn node_zip_url(version: &str) -> String {
    format!(
        "https://nodejs.org/dist/v{0}/node-v{0}-win-x64.zip",
        version.trim_start_matches('v')
    )
}

/// Из zip node-vX-win-x64 достаём единственный `/node.exe` (первый entry,
/// файл которого оканчивается на `node.exe`).
fn extract_node_zip(zip_bytes: &[u8], dest: &Path) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(zip_bytes))
        .map_err(|e| format!("Ошибка открытия node.zip: {}", e))?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("Ошибка чтения node.zip: {}", e))?;
        let name = entry.name().to_string();
        let lower = name.to_lowercase();
        // В zip только один файл с точным именем node.exe (в папке node-vX-win-x64/).
        if lower.ends_with("/node.exe") || lower == "node.exe" {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut out = fs::File::create(dest).map_err(|e| format!("Не создать {}: {}", dest.display(), e))?;
            std::io::copy(&mut entry, &mut out).map_err(|e| format!("Распаковка node.exe: {}", e))?;
            log::info!("✅ node.exe из {} ({} байт)", name, fs::metadata(dest).map(|m| m.len()).unwrap_or(0));
            return Ok(());
        }
    }
    Err("node.exe не найден внутри node.zip".to_string())
}

// ─────────────────────────────── 9router npm tgz ───────────────────────────────

fn npm_tgz_url(version: &str) -> String {
    format!("https://registry.npmjs.org/9router/-/9router-{}.tgz", version)
}

/// Распаковка tgz: внутрь кладётся `package/`, полос делаем через
/// strip_top_level ("package"). Ставим в `dist/`, чтобы пути сервера совпали:
/// `dist/app/custom-server.js`.
fn extract_tgz_strip_package(tgz_bytes: &[u8], dest: &Path) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|e| format!("Не создать {}: {}", dest.display(), e))?;

    let gz = flate2::read::GzDecoder::new(tgz_bytes);
    let mut archive = tar::Archive::new(gz);
    let entries = archive
        .entries()
        .map_err(|e| format!("Ошибка открытия tgz: {}", e))?;

    for entry in entries {
        let mut entry = entry.map_err(|e| format!("Ошибка чтения tgz-записи: {}", e))?;
        let path = entry
            .path()
            .map_err(|e| format!("Ошибка пути в tgz: {}", e))?
            .to_path_buf();
        // Пропускаем "package/..." верхний уровень.
        let rel = match strip_top_level(&path) {
            Some(r) => r,
            None => continue,
        };
        if rel.as_os_str().is_empty() {
            continue;
        }
        let out_path = dest.join(&rel);
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Не создать {}: {}", parent.display(), e))?;
        }
        let kind = entry.header().entry_type();
        if kind.is_symlink() {
            // Оставляем симлинки как есть — в standalone их не бывает, пропускаем.
            continue;
        }
        if !kind.is_dir() {
            let mut out = fs::File::create(&out_path)
                .map_err(|e| format!("Не создать {}: {}", out_path.display(), e))?;
            std::io::copy(&mut entry, &mut out)
                .map_err(|e| format!("Запись {}: {}", out_path.display(), e))?;
        }
    }
    log::info!("✅ tgz распакован в {}", dest.display());
    Ok(())
}

/// Убрать верхний уровень пути ("package/..."), если он — одиночная папка.
fn strip_top_level(path: &Path) -> Option<PathBuf> {
    let comps: Vec<_> = path.components().collect();
    if comps.is_empty() {
        return None;
    }
    comps[1..].iter().collect::<PathBuf>().into()
}

// ─────────────────────────────── Скачивание байт ───────────────────────────────

/// Скачивание с фоллбэком: reqwest → PowerShell (читает системный прокси,
/// Schannel как браузер). По-байтово, с прогрессом по content-length.
fn download_bytes(client: &Client, url: &str) -> Result<Vec<u8>, String> {
    match client.get(url).send() {
        Ok(resp) if resp.status().is_success() => {
            let total = resp.content_length().unwrap_or(0);
            let bytes = resp.bytes().map_err(|e| format!("Ошибка чтения {}: {}", url, e))?;
            if total > 0 && (bytes.len() as u64) < total {
                return Err(format!("Недокачано {}: {} из {} байт", url, bytes.len(), total));
            }
            Ok(bytes.to_vec())
        }
        Ok(resp) => Err(format!("{}: HTTP {}", url, resp.status())),
        Err(_) => download_via_powershell(url),
    }
}

/// Фоллбэк: PowerShell Invoke-WebRequest в temp-файл → чтение в память.
fn download_via_powershell(url: &str) -> Result<Vec<u8>, String> {
    let tmp = std::env::temp_dir().join(format!("nr_dl_{}.bin", std::process::id()));
    let dest_str = tmp.display().to_string();
    let ps_script = format!(
        "[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; \
         $ProgressPreference = 'SilentlyContinue'; \
         Invoke-WebRequest -Uri '{url}' -OutFile '{dest}' -UseBasicParsing",
        url = url.replace('\'', "''"),
        dest = dest_str.replace('\'', "''"),
    );
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &ps_script])
        .output()
        .map_err(|e| format!("PowerShell не найден: {}", e))?;
    if output.status.success() {
        let bytes = fs::read(&tmp).map_err(|e| format!("Не прочитать temp: {}", e))?;
        let _ = fs::remove_file(&tmp);
        if bytes.is_empty() {
            return Err("PowerShell скачал пустой файл".to_string());
        }
        return Ok(bytes);
    }
    let _ = fs::remove_file(&tmp);
    Err(format!("PowerShell: {}", String::from_utf8_lossy(&output.stderr).trim()))
}

/// Удобный PathBuf импорт (используется снаружи тестами).
#[allow(dead_code)]
fn _pathbuf(path: &str) -> PathBuf { PathBuf::from(path) }