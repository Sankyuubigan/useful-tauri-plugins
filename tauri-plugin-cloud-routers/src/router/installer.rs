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
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use reqwest::blocking::Client;
use tauri::AppHandle;

use crate::commands::RouterId;
use crate::router::config::{dist_dir, node_exe, server_script};

/// Прогресс-колбэк: (stage, скачано_байт, всего_байт, текст).
pub type ProgressFn = Box<dyn Fn(&str, u64, u64, &str) + Send>;

// ─────────────────────────── Блокировка установки ───────────────────────────
//
// Установка роутера ЛЕТАЕТ минуты (скачивание Node + `npm install` зависимостей).
// В это время хост может вызвать `ensure_started` / `get_combos`, которые
// поднимают сервер на ПОЛУРАСПАКАННОМ `dist/`. Итог — падение
// `Cannot find module 'next'` и «установлено, но не работает».
//
// Решение: один мьютекс на роутер. `install_or_update` держит его весь свой
// прогон, а любой запуск сервера сначала берёт тот же мьютекс → сервер НЕ
// может стартовать поверх незавершённой установки. Никакой «стальной» защиты.

static INSTALL_LOCKS: OnceLock<Vec<Mutex<()>>> = OnceLock::new();

fn install_locks() -> &'static Vec<Mutex<()>> {
    INSTALL_LOCKS.get_or_init(|| vec![Mutex::new(()), Mutex::new(()), Mutex::new(())])
}

/// Мьютекс установки роутера. Держать на всё время установки и на всё время
/// запуска сервера (обе операции — внутри `spawn_blocking`, без `.await`).
pub fn install_guard(router_id: RouterId) -> MutexGuard<'static, ()> {
    let lock = &install_locks()[router_id.lock_index()];
    match lock.lock() {
        Ok(g) => g,
        // Отравленный мьютекс = паника в другом потоке; восстановление
        // безопасно: внутри критической секции нет инвариантов, только «жди».
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Маркер завершённого `npm install` — скрытый lock-файл npm.
/// Создаётся в самом конце reify-фазы, поэтому его наличие = зависимости
/// установлены ПОЛНОСТЬЮ. Пока его нет — установка прервана/не завершена.
fn deps_marker(dist: &Path) -> PathBuf {
    dist.join("node_modules").join(".package-lock.json")
}

/// Зависимости роутера установлены? Для standalone-роутов (9router,
/// extremerouter) зависимости вшиты в tgz — проверять нечего.
pub fn deps_present(dir: &Path, router_id: RouterId) -> bool {
    if !router_id.needs_npm_deps() {
        return true;
    }
    deps_marker(&dist_dir(dir)).exists()
}

/// npm распакован целиком? Маркер — вложенный модуль npm с расширением `.cjs`:
/// именно такие файлы терялись при фильтре по расширениям (регрессия, из-за
/// которой `npm install` падал с MODULE_NOT_FOUND на `just-diff`).
pub fn npm_runtime_intact(runtime_dir: &Path) -> bool {
    runtime_dir
        .join("node_modules")
        .join("npm")
        .join("node_modules")
        .join("just-diff")
        .join("index.cjs")
        .exists()
}

/// Аргументы `npm install` для зависимостей роутера (SSOT — здесь, не в вызове).
///
/// `--legacy-peer-deps` обязателен: в `omniroute` есть честный конфликт peer-зависимостей
/// (`marked@^18` в корне против `marked@">=1 <16"` у `marked-terminal`), из-за которого
/// строгий резолв npm падает с ERESOLVE.
///
/// `--ignore-scripts` обязателен: у `omniroute` в `package.json` есть
/// `"prepare": "husky"`, а `husky` — **devDependency** (установщик git-хуков для
/// разработчиков пакета). Мы ставим с `--omit=dev`, поэтому бинаря `husky` нет вовсе,
/// и `cmd /c husky` падает с «не является внутренней или внешней командой» → npm
/// прерывает установку на финальной стадии (в логе: `postinstall` отработал, дальше
/// `> omniroute@3.8.50 prepare` → `error code 1`), и `next` остаётся недопустимым.
///
/// Нуженный `postinstall` (копирует `sql.js`, `@swc/helpers`, `node-machine-id`,
/// патчит `playwright-core`) запускается ОТДЕЛЬНО и ЯВНО — см. `run_package_script`.
/// Выключать скрипты wholesale нельзя: без `postinstall` сервер не взлетит.
pub fn npm_install_args() -> Vec<&'static str> {
    vec![
        "install",
        "--omit=dev",
        "--no-audit",
        "--no-fund",
        "--legacy-peer-deps",
        "--ignore-scripts",
    ]
}

/// Скрипт жизненного цикла из `package.json` пакета (SSOT — сам пакет, не наш хардкод).
/// `dist/package.json` распакован из tgz, поэтому читаем его, а не гадаем о путях.
pub fn package_script(dist: &Path, name: &str) -> Option<String> {
    let raw = fs::read_to_string(dist.join("package.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let script = json.get("scripts")?.get(name)?.as_str()?.trim();
    if script.is_empty() {
        None
    } else {
        Some(script.to_string())
    }
}

/// Установка считается завершённой? Все четыре условия обязательны:
/// версия совпадает, node.exe и серверный скрипт на месте, зависимости
/// реально установлены (иначе прерванная установка выглядит успешной).
pub fn install_is_complete(
    installed_version: Option<&str>,
    npm_ver: &str,
    node_present: bool,
    server_present: bool,
    deps_ok: bool,
) -> bool {
    installed_version == Some(npm_ver) && node_present && server_present && deps_ok
}

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
    let complete = install_is_complete(
        cfg.installed_version.as_deref(),
        &npm_ver,
        node_exe(&dir).exists(),
        server_script_exists(&dir, router_id),
        deps_present(&dir, router_id),
    );
    if complete {
        progress("installed", 0, 0, &format!("{} v{} уже установлен", router_id, npm_ver));
        log::info!("{} v{} уже установлен", router_id, npm_ver);
        return Ok(InstalledInfo::current(npm_ver, cfg.node_version.clone().unwrap_or_default()));
    }
    if cfg.installed_version.is_some() {
        log::warn!(
            "{}: предыдущая установка неполная (node={}, server={}, deps={}) — переустанавливаем",
            router_id,
            node_exe(&dir).exists(),
            server_script_exists(&dir, router_id),
            deps_present(&dir, router_id),
        );
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
    let runtime_dir = dir.join("runtime");
    if !node.exists() || !npm_runtime_intact(&runtime_dir) || force {
        log::info!("{}: скачиваем портативный Node.js {} (npm цел: {})", router_id, node_ver, npm_runtime_intact(&runtime_dir));
        progress("node", 0, 0, "Скачиваем портативный Node.js...");
        let zip_bytes = download_bytes(&client, &node_zip_url(&node_ver))?;
        extract_node_zip(&zip_bytes, &node)?;
    }
    if !node.exists() {
        return Err("node.exe не найден после распаковки".to_string());
    }
    if !npm_runtime_intact(&runtime_dir) {
        return Err(format!(
            "{}: npm распакован не полностью — зависимости невозможно установить. Переустановите роутер.",
            router_id
        ));
    }

    progress("router", 0, 0, &format!("Скачиваем {} v{}...", router_id, npm_ver));
    let tgz = download_bytes(&client, &npm_tgz_url(&npm_ver, router_id))?;
    let dist = dist_dir(&dir);
    if dist.exists() {
        fs::remove_dir_all(&dist).map_err(|e| format!("Не удалось обновить dist: {}", e))?;
    }
    extract_tgz_strip_package(&tgz, &dist)?;

    let server = match find_server_script(&dist, router_id) {
        Some(s) => s,
        None => {
            let expected = server_script(&dist, router_id);
            return Err(format!("В tgz не найден сервер бандла: {}", expected.display()));
        }
    };
    log::info!("{}: серверный скрипт найден: {}", router_id, server.display());

    if router_id.needs_npm_deps() {
        progress("deps", 0, 0, "Устанавливаем зависимости OmniRoute (npm install)...");
        let runtime_npm_cli = runtime_dir.join("node_modules").join("npm").join("bin").join("npm-cli.js");
        install_omniroute_deps(&dist, &node, &runtime_npm_cli, router_id)?;
        if !deps_present(&dir, router_id) {
            return Err(format!(
                "{}: npm install завершился, но маркер зависимостей не создан ({}). Установка неполная.",
                router_id,
                deps_marker(&dist).display()
            ));
        }
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

/// Ищет серверный скрипт внутри `dist`. Сначала проверяет ожидаемый путь,
/// затем рекурсивно обходит подпапки в поисках файла с нужным именем.
fn find_server_script(dist: &Path, router_id: RouterId) -> Option<PathBuf> {
    let expected = server_script(dist, router_id);
    if expected.exists() {
        return Some(expected);
    }
    let filename = Path::new(router_id.server_script_relative()).file_name()?;
    find_file_recursive(dist, filename)
}

fn find_file_recursive(dir: &Path, filename: &std::ffi::OsStr) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    let mut subdirs: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            subdirs.push(path);
        } else if path.file_name() == Some(filename) {
            return Some(path);
        }
    }
    for subdir in subdirs {
        if let Some(found) = find_file_recursive(&subdir, filename) {
            return Some(found);
        }
    }
    None
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
    let runtime_dir = dest.parent().unwrap_or(dest);
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
        } else if lower.contains("node_modules/npm/") {
            let rel = name.splitn(2, '/').nth(1).unwrap_or("");
            if rel.is_empty() {
                continue;
            }
            let out_path = runtime_dir.join(rel);
            if entry.is_dir() || name.ends_with('/') {
                fs::create_dir_all(&out_path).map_err(|e| format!("Не создать папку npm: {}", e))?;
            } else {
                if let Some(parent) = out_path.parent() {
                    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                let mut out = fs::File::create(&out_path).map_err(|e| format!("Не создать {}: {}", out_path.display(), e))?;
                std::io::copy(&mut entry, &mut out).map_err(|e| format!("Распаковка npm: {}", e))?;
            }
        }
    }
    if !dest.exists() {
        return Err("node.exe не найден внутри node.zip".to_string());
    }
    Ok(())
}

/// Установить дерево зависимостей роутера и выполнить его `postinstall`.
///
/// Два шага НАМЕРЕННО разделены:
/// 1. `npm install --ignore-scripts` — только дерево зависимостей, без
///    lifecycle-скриптов пакета (`prepare: husky` не имеет смысла вне git-репо
///    и роняет установку, см. `npm_install_args`).
/// 2. `postinstall` запускается ЯВНО, потому что он нужен: раскладывает
///    `sql.js`, `@swc/helpers`, `node-machine-id` и патчит `playwright-core`
///    в standalone-каталог, без него сервер не поднимется.
fn install_omniroute_deps(
    dist: &Path,
    node: &Path,
    npm_cli: &Path,
    router_id: RouterId,
) -> Result<(), String> {
    if !npm_cli.exists() {
        return Err("npm-cli.js не найден в runtime/node_modules/npm/bin".to_string());
    }
    let args = npm_install_args();
    log::info!("{}: npm {} в {}", router_id, args.join(" "), dist.display());

    let mut cmd = std::process::Command::new(node);
    cmd.arg(npm_cli)
        .args(&args)
        .current_dir(dist)
        .env("NODE_PATH", dist.join("node_modules"));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }

    let started = Instant::now();
    let output = run_captured(&mut cmd, "npm install")?;
    let elapsed = started.elapsed().as_secs_f64();
    if !output.status.success() {
        return Err(format!(
            "{}: npm install не завершился (код {:?}) — подробности в логе приложения.",
            router_id,
            output.status.code()
        ));
    }
    log::info!(
        "{}: дерево зависимостей установлено за {:.1}c ({} пакетов)",
        router_id,
        elapsed,
        fs::read_dir(dist.join("node_modules")).map(|it| it.count()).unwrap_or(0),
    );

    match package_script(dist, "postinstall") {
        Some(script) => {
            log::info!("{}: выполняем postinstall пакета: {}", router_id, script);
            let mut cmd = package_script_command(node, dist, &script);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x08000000);
            }
            let started = Instant::now();
            let output = run_captured(&mut cmd, "postinstall")?;
            if !output.status.success() {
                return Err(format!(
                    "{}: postinstall пакета не завершился (код {:?}) — подробности в логе приложения.",
                    router_id,
                    output.status.code()
                ));
            }
            log::info!("{}: postinstall выполнен за {:.1}c", router_id, started.elapsed().as_secs_f64());
        }
        None => log::info!("{}: у пакета нет postinstall — пропускаем", router_id),
    }
    Ok(())
}

/// Команда запуска npm-скрипта пакета: на Windows через `cmd /c`, иначе через `sh -c`.
fn package_script_command(node: &Path, dist: &Path, script: &str) -> std::process::Command {
    let mut cmd = if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.arg("/d").arg("/s").arg("/c").arg(script);
        c
    } else {
        let mut c = std::process::Command::new("sh");
        c.arg("-c").arg(script);
        c
    };
    // Узлы на PATH не нужны: скрипты пакета рассчитаны на npm-окружение,
    // поэтому кладём наш портативный node/bin и зависимости пакета.
    let node_dir = node.parent().unwrap_or(node);
    let npm_bin = node_dir.join("node_modules").join("npm").join("bin");
    let pkg_bin = dist.join("node_modules").join(".bin");
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let new_path = std::env::split_paths(&inherited).chain([npm_bin.clone(), pkg_bin]);
    cmd.current_dir(dist)
        .env("PATH", std::env::join_paths(new_path).unwrap_or(inherited))
        .env("NODE_PATH", dist.join("node_modules"))
        .env("npm_execpath", npm_bin.join("npm-cli.js"));
    cmd
}

/// Запустить команду и залогировать полный вывод. Ошибка → Err с контекстом,
/// успех → `Output` (вызывающий сам решает, что значит ненулевой код).
fn run_captured(
    cmd: &mut std::process::Command,
    what: &str,
) -> Result<std::process::Output, String> {
    let started = Instant::now();
    let output = cmd
        .output()
        .map_err(|e| format!("Не удалось запустить {}: {}", what, e))?;
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Полный вывод — в лог (десятки КБ), в UI — короткая причина.
        log::error!(
            "{} упал за {:.1}c (code {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
            what,
            started.elapsed().as_secs_f64(),
            output.status.code(),
            stdout.trim(),
            stderr.trim(),
        );
    }
    Ok(output)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Временная папка с автоудалением (тесты не мусорят в системном temp).
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let p = std::env::temp_dir().join(format!("cr_installer_{}_{}", tag, std::process::id()));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).expect("создать temp");
            TempDir(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Собрать zip в памяти: список (имя записи, содержимое).
    fn build_zip(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default();
            for (name, content) in entries {
                if name.ends_with('/') {
                    w.add_directory(*name, opts).expect("каталог в zip");
                } else {
                    w.start_file(*name, opts).expect("файл в zip");
                    w.write_all(content.as_bytes()).expect("запись в zip");
                }
            }
            w.finish().expect("закрыть zip");
        }
        buf.into_inner()
    }

    /// РЕГРЕССИЯ: фильтр по расширениям (`.js/.json/.cmd`) терял вложенные
    /// `.cjs`-модули npm → `npm install` падал с MODULE_NOT_FOUND на just-diff,
    /// а `next` не доустанавливался. Проверяем, что вложенные файлы любых
    /// расширений распаковываются, а служебные файлы Node — нет.
    #[test]
    fn extract_node_zip_keeps_nested_npm_modules_any_extension() {
        let tmp = TempDir::new("extract_npm");
        let dest = tmp.path().join("runtime").join("node.exe");
        let bytes = build_zip(&[
            ("node-v24.21.0-win-x64/node.exe", "MZ-node"),
            ("node-v24.21.0-win-x64/node_modules/npm/", ""),
            ("node-v24.21.0-win-x64/node_modules/npm/bin/npm-cli.js", "// cli"),
            ("node-v24.21.0-win-x64/node_modules/npm/node_modules/", ""),
            // Именно эти файлы терялись из-за фильтра расширений:
            ("node-v24.21.0-win-x64/node_modules/npm/node_modules/just-diff/index.cjs", "// cjs"),
            ("node-v24.21.0-win-x64/node_modules/npm/node_modules/just-diff/index.mjs", "// mjs"),
            ("node-v24.21.0-win-x64/node_modules/npm/node_modules/glob/package.json", "{}"),
            // Служебное содержимое Node (не npm) — распаковывать не нужно.
            ("node-v24.21.0-win-x64/node_modules/corepack/dist/corepack.js", "// corepack"),
        ]);

        extract_node_zip(&bytes, &dest).expect("распаковать node.zip");

        let npm_root = tmp.path().join("runtime").join("node_modules").join("npm");
        assert!(dest.exists(), "node.exe должен быть распакован");
        assert!(npm_root.join("bin").join("npm-cli.js").exists(), "npm-cli.js");
        assert_eq!(
            fs::read_to_string(npm_root.join("node_modules").join("just-diff").join("index.cjs")).unwrap(),
            "// cjs",
            "вложенный .cjs обязан распаковываться (регрессия just-diff)",
        );
        assert!(npm_root.join("node_modules").join("just-diff").join("index.mjs").exists(), ".mjs");
        assert!(npm_root.join("node_modules").join("glob").join("package.json").exists(), ".json");
        assert!(
            !tmp.path().join("runtime").join("node_modules").join("corepack").exists(),
            "corepack — не npm, распаковывать нельзя",
        );
    }

    /// Каталоги вложенных зависимостей создаются, а не падают на File::create.
    #[test]
    fn extract_node_zip_creates_nested_directories() {
        let tmp = TempDir::new("extract_dirs");
        let dest = tmp.path().join("runtime").join("node.exe");
        let bytes = build_zip(&[
            ("node/node.exe", "MZ"),
            ("node/node_modules/npm/lib/", ""),
            ("node/node_modules/npm/lib/utils/read-package-json.js", "// u"),
        ]);

        extract_node_zip(&bytes, &dest).expect("распаковать с каталогами");

        let f = tmp.path().join("runtime").join("node_modules").join("npm").join("lib").join("utils").join("read-package-json.js");
        assert!(f.is_file(), "файл во вложенном каталоге должен существовать");
    }

    /// РЕГРЕССИЯ: `omniroute` содержит конфликт peer-зависимостей
    /// (`marked@^18` против `marked@">=1 <16"` у marked-terminal) — без
    /// `--legacy-peer-deps` npm падает с ERESOLVE и ничего не ставит.
    #[test]
    fn npm_install_args_allow_peer_conflict_resolution() {
        let args = npm_install_args();
        assert_eq!(args[0], "install");
        assert!(
            args.contains(&"--legacy-peer-deps"),
            "ERESOLVE без --legacy-peer-deps: {:?}",
            args
        );
        assert!(args.contains(&"--omit=dev"));
        assert!(args.contains(&"--no-audit"));
        assert!(args.contains(&"--no-fund"));
    }

    /// РЕГРЕССИЯ (реальный лог): `omniroute` содержит `"prepare": "husky"`, а
    /// `husky` — devDependency. При `--omit=dev` бинаря нет, поэтому
    /// `cmd /c husky` падает с «не является внутренней или внешней командой» →
    /// `prepare` роняет весь `npm install` ПОСЛЕ успешного `postinstall`.
    /// Значит `npm install` обязан идти с `--ignore-scripts`.
    #[test]
    fn npm_install_args_skip_lifecycle_scripts_that_break_install() {
        let args = npm_install_args();
        assert!(
            args.contains(&"--ignore-scripts"),
            "prepare:husky уронит установку без --ignore-scripts: {:?}",
            args
        );
        assert!(args.contains(&"--legacy-peer-deps"), "ERESOLVE по peer deps");
        assert!(args.contains(&"--omit=dev"));
    }

    /// Скрипты пакета читаются из ЕГО package.json (SSOT), а не хардкодятся.
    #[test]
    fn package_script_reads_from_package_json() {
        let tmp = TempDir::new("pkg_script");
        fs::write(
            tmp.path().join("package.json"),
            r#"{"scripts":{"prepare":"husky","postinstall":"node scripts/build/postinstall.mjs"}}"#,
        )
        .unwrap();

        assert_eq!(package_script(tmp.path(), "postinstall").as_deref(), Some("node scripts/build/postinstall.mjs"));
        assert_eq!(package_script(tmp.path(), "prepare").as_deref(), Some("husky"));
        assert_eq!(package_script(tmp.path(), "preinstall"), None);
        assert_eq!(package_script(tmp.path(), "missing"), None);

        // Пустой скрипт = его нет.
        fs::write(tmp.path().join("package.json"), r#"{"scripts":{"postinstall":"  "}}"#).unwrap();
        assert_eq!(package_script(tmp.path(), "postinstall"), None);

        // Битый/отсутствующий package.json — не паникуем.
        fs::write(tmp.path().join("package.json"), "{ not json").unwrap();
        assert_eq!(package_script(tmp.path(), "postinstall"), None);
    }

    /// Только OmniRoute требует `npm install`; standalone-роутеры сшиты в tgz.
    #[test]
    fn only_omniroute_requires_npm_deps() {
        assert!(RouterId::OmniRoute.needs_npm_deps());
        assert!(!RouterId::NineRouter.needs_npm_deps());
        assert!(!RouterId::ExtremeRouter.needs_npm_deps());
    }

    /// Прерванная установка не должна выдавать себя за успешную:
    /// без маркера `node_modules/.package-lock.json` зависимостей нет.
    #[test]
    fn deps_present_requires_npm_completion_marker() {
        let tmp = TempDir::new("deps_marker");
        let dist = dist_dir(tmp.path());

        assert!(!deps_present(tmp.path(), RouterId::OmniRoute), "до npm install deps нет");

        fs::create_dir_all(dist.join("node_modules")).unwrap();
        assert!(!deps_present(tmp.path(), RouterId::OmniRoute), "пустой node_modules — не установлено");

        // npm прерван посередине: каталоги есть, маркера нет.
        fs::create_dir_all(dist.join("node_modules").join("next")).unwrap();
        assert!(!deps_present(tmp.path(), RouterId::OmniRoute), "прерванный npm install");

        fs::write(deps_marker(&dist), "{}").unwrap();
        assert!(deps_present(tmp.path(), RouterId::OmniRoute), "маркер создан — установлено");

        // Standalone-роутерам зависимости не нужны.
        assert!(deps_present(tmp.path(), RouterId::NineRouter));
    }

    /// npm считается распакованным по вложенному `.cjs`-модулю.
    #[test]
    fn npm_runtime_intact_detects_partial_unpack() {
        let tmp = TempDir::new("npm_intact");
        let runtime = tmp.path().join("runtime");

        assert!(!npm_runtime_intact(&runtime), "пустой runtime — npm нет");

        let just_diff = runtime.join("node_modules").join("npm").join("node_modules").join("just-diff");
        fs::create_dir_all(&just_diff).unwrap();
        assert!(!npm_runtime_intact(&runtime), "каталог без index.cjs — npm неполон");

        fs::write(just_diff.join("index.cjs"), "// cjs").unwrap();
        assert!(npm_runtime_intact(&runtime), "index.cjs есть — npm цел");
    }

    /// Все четыре условия обязательны: прерванная установка обязана
    /// переустанавливаться, а не считаться завершённой.
    #[test]
    fn install_is_complete_requires_every_signal() {
        assert!(install_is_complete(Some("3.8.50"), "3.8.50", true, true, true));
        assert!(!install_is_complete(Some("3.8.49"), "3.8.50", true, true, true), "версия");
        assert!(!install_is_complete(Some("3.8.50"), "3.8.50", false, true, true), "нет node");
        assert!(!install_is_complete(Some("3.8.50"), "3.8.50", true, false, true), "нет server.js");
        assert!(!install_is_complete(Some("3.8.50"), "3.8.50", true, true, false), "нет зависимостей");
        assert!(!install_is_complete(None, "3.8.50", true, true, true), "не установлен");
    }

    /// Мьютекс установки взаимно исключает запуск сервера: пока установка
    /// держит lock, второй поток БЛОКИРУЕТСЯ — значит сервер не поднимется
    /// поверх полураспакованного dist (корень бага «Cannot find module 'next'»).
    #[test]
    fn install_guard_blocks_start_until_install_finished() {
        use std::sync::mpsc;

        let router = RouterId::OmniRoute;
        let install_guard_held = install_locks()[router.lock_index()].lock().unwrap();

        let (tx, rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _g = install_guard(router); // имитация start_server
            tx.send("acquired").unwrap();
        });

        // Пока установка идёт — старт не проходит.
        assert!(
            rx.recv_timeout(Duration::from_millis(200)).is_err(),
            "start_server не должен был пройти, пока идёт установка",
        );

        drop(install_guard_held);
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            "acquired",
            "после завершения установки старт должен пройти",
        );
        worker.join().unwrap();

        // Другой роутер не блокируется.
        let _nine = install_guard(RouterId::NineRouter);
    }
}
