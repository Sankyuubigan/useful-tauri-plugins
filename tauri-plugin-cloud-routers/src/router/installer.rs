//! Установка / обновление облачных роутеров БЕЗ системных зависимостей.
//!
//! Всё скачивается и распаковывается в папку `<router_dir>` (по умолчанию
//! `<exe>/cloud_routers/<id>`), терминал и админ-права не нужны:
//!
//! ```text
//! <exe>/cloud_routers/<id>/
//! ├─ runtime/node.exe          ← портативный Node.js (zip c nodejs.org, LTS)
//! └─ dist/...                  ← standalone роутер (tgz c registry.npmjs.org)
//!    ├─ app/custom-server.js   (9router, extremerouter)
//!    └─ server.js              (omniroute)
//! ```
//!
//! SQLite для роутеров: на Node >= 22 используется встроенный `node:sqlite`
//! или WASM `sql.js`, поэтому никакого `npm install` и сборки native
//! модулей не требуется.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::blocking::Client;
use tauri::AppHandle;

use crate::commands::RouterId;
use crate::router::config::{dist_dir, node_exe, server_script};

/// Прогресс-колбэк: (stage, скачано_байт, всего_байт, текст).
pub type ProgressFn = Box<dyn Fn(&str, u64, u64, &str) + Send>;

/// Ближайшая версия роутера из npm registry.
pub fn latest_npm_version(client: &Client, router_id: RouterId) -> Result<String, String> {
    let pkg = router_id.npm_package();
    let url = format!("https://registry.npmjs.org/{}/latest", pkg);
    let resp = client.get(url).timeout(Duration::from_secs(15)).send()
        .map_err(|e| format!("Не удалось получить версию {}: {}", router_id, e))?;
    if !resp.status().is_success() {
        return Err(format!("npm registry: HTTP {}", resp.status()));
    }
    let v: serde_json::Value = resp.json().map_err(|e| format!("Bad JSON от npm: {}", e))?;
    v.get("version")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "npm registry: нет поля version".to_string())
}

/// Вся установка роутера (блокирующая; вызывать из spawn_blocking).
///
/// Шаги:
/// 1. Резолвим версии (роутер + Node LTS) с hot-cache по конфигу.
/// 2. Качаем tgz роутера → распаковываем в `dist/`.
/// 3. Качаем zip Node → достаём `node.exe` → кладём в `runtime/`.
/// 4. Обновляем `cloud_routers.<id>` в app_config.json.
pub fn install_or_update(
    app: &AppHandle,
    router_id: RouterId,
    force: bool,
    progress: ProgressFn,
) -> Result<InstalledInfo, String> {
    let cfg = crate::router::config::load_config(app, router_id);
    let dir = crate::router::config::router_dir(app, router_id);

    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP client: {}", e))?;
    let mut cfg = cfg;

    let npm_ver = latest_npm_version(&client, router_id)?;
    let installed_ok = cfg.installed_version.as_deref() == Some(npm_ver.as_str())
        && node_exe(&dir).exists()
        && server_script_exists(&dir, router_id);
    if installed_ok {
        progress("installed", 0, 0, &format!("{} v{} уже установлен", router_id, npm_ver));
        log::info!("{} v{} уже установлен", router_id, npm_ver);
        return Ok(InstalledInfo::current(npm_ver, cfg.node_version.clone().unwrap_or_default()));
    }

    crate::router::process::stop_server(
        cfg.port_or_default(router_id),
        &crate::router::config::router_data_dir(app, router_id),
    );
    crate::router::process::kill_node_processes(&node_exe(&dir));

    fs::create_dir_all(dir.join("runtime"))
        .map_err(|e| format!("Не удалось создать папку установки: {}", e))?;

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

    progress("router", 0, 0, &format!("Скачиваем {} v{}...", router_id, npm_ver));
    let tgz = download_bytes(&client, &npm_tgz_url(&npm_ver, router_id))?;
    let dist = dist_dir(&dir);
    if dist.exists() {
        fs::remove_dir_all(&dist).map_err(|e| format!("Не удалось обновить dist: {}", e))?;
    }
    extract_tgz_strip_package(&tgz, &dist)?;

    let server = server_script(&dist, router_id);
    if !server.exists() {
        return Err(format!("В tgz не найден сервер бандла: {}", server.display()));
    }

    cfg.installed_version = Some(npm_ver.clone());
    cfg.node_version = Some(node_ver.clone());
    crate::router::config::save_config(app, router_id, &cfg)?;

    progress("done", 0, 0, &format!("✅ {} v{} установлен (Node {})", router_id, npm_ver, node_ver));
    log::info!("✅ {} v{} установлен (Node {})", router_id, npm_ver, node_ver);

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
            path: crate::router::config::default_router_dir(RouterId::NineRouter).to_string_lossy().to_string(),
        }
    }
}

fn server_script_exists(dir: &Path, router_id: RouterId) -> bool {
    let dist = dist_dir(dir);
    server_script(&dist, router_id).exists()
}

fn resolve_node_version(client: &Client) -> Result<String, String> {
    let url = "https://nodejs.org/dist/index.json";
    match client.get(url).timeout(Duration::from_secs(20)).send() {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(list) = resp.json::<serde_json::Value>() {
                if let Some(arr) = list.as_array() {
                    for entry in arr {
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

fn extract_node_zip(zip_bytes: &[u8], dest: &Path) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(zip_bytes))
        .map_err(|e| format!("Ошибка открытия node.zip: {}", e))?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("Ошибка чтения node.zip: {}", e))?;
        let name = entry.name().to_string();
        let lower = name.to_lowercase();
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

fn npm_tgz_url(version: &str, router_id: RouterId) -> String {
    let pkg = router_id.npm_package();
    format!("https://registry.npmjs.org/{}/-/{}-{}.tgz", pkg, pkg.replace('/', "%2F"), version)
}

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

fn strip_top_level(path: &Path) -> Option<PathBuf> {
    let comps: Vec<_> = path.components().collect();
    if comps.is_empty() {
        return None;
    }
    comps[1..].iter().collect::<PathBuf>().into()
}

fn download_bytes(_client: &Client, url: &str) -> Result<Vec<u8>, String> {
    let opts = tauri_plugin_downloader::DownloadOptions {
        label: "Cloud Routers / Node.js".into(),
        kind: "cloud-routers".into(),
        ..Default::default()
    };
    tauri_plugin_downloader::download_bytes_blocking(url, opts)
}

#[allow(dead_code)]
fn _pathbuf(path: &str) -> PathBuf { PathBuf::from(path) }