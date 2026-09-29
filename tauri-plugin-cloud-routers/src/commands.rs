//! Tauri-команды плагина cloud-routers. Тонкий слой: вся логика — в `crate::router`.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use serde::Deserialize;
use serde_json::json;
use tauri::{AppHandle, Emitter};

use crate::router::{
    client::{self, ChatMessage, ChatRequest},
    config::{self, dist_dir, node_exe, router_dir, server_script},
    installer, process,
};

/// Идентификатор роутера.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RouterId {
    NineRouter,
    ExtremeRouter,
    OmniRoute,
}

impl RouterId {
    pub fn as_str(&self) -> &'static str {
        match self {
            RouterId::NineRouter => "9router",
            RouterId::ExtremeRouter => "extremerouter",
            RouterId::OmniRoute => "omniroute",
        }
    }

    pub fn default_port(&self) -> u16 {
        match self {
            RouterId::NineRouter => 20128,
            RouterId::ExtremeRouter => 20129,
            RouterId::OmniRoute => 20130,
        }
    }

    pub fn npm_package(&self) -> &'static str {
        match self {
            RouterId::NineRouter => "9router",
            RouterId::ExtremeRouter => "@rsalmn/extremerouter",
            RouterId::OmniRoute => "omniroute",
        }
    }

    pub fn dir_name(&self) -> &'static str {
        match self {
            RouterId::NineRouter => "9router",
            RouterId::ExtremeRouter => "extremerouter",
            RouterId::OmniRoute => "omniroute",
        }
    }

    /// Нужен ли `npm install` поверх распакованного tgz?
    ///
    /// 9router/extremerouter — standalone: все зависимости вшиты в пакет.
    /// OmniRoute — Next.js-приложение: `server.js` делает `require('next')`,
    /// поэтому зависимости обязаны быть в `dist/node_modules`.
    pub fn needs_npm_deps(&self) -> bool {
        match self {
            RouterId::NineRouter | RouterId::ExtremeRouter => false,
            RouterId::OmniRoute => true,
        }
    }

    /// Индекс роутера в массиве мьютексов установки.
    pub fn lock_index(&self) -> usize {
        match self {
            RouterId::NineRouter => 0,
            RouterId::ExtremeRouter => 1,
            RouterId::OmniRoute => 2,
        }
    }

    pub fn server_script_relative(&self) -> &'static str {
        match self {
            RouterId::NineRouter => "app/custom-server.js",
            RouterId::ExtremeRouter => "app/custom-server.js",
            RouterId::OmniRoute => "dist/server.js",
        }
    }
}

impl std::fmt::Display for RouterId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for RouterId {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "9router" => Ok(RouterId::NineRouter),
            "extremerouter" => Ok(RouterId::ExtremeRouter),
            "omniroute" => Ok(RouterId::OmniRoute),
            _ => Err(format!("Неизвестный роутер: {}", s)),
        }
    }
}

/// Состояние шлюза для UI (индикатор + кнопки).
#[derive(Serialize, Clone)]
pub struct RouterStatus {
    /// Установлены и node.exe, и бандл роутера.
    pub installed: bool,
    /// Порт отвечает (сервер жив).
    pub running: bool,
    /// Версия роутера (из npm-тега, зафиксированная при установке).
    pub version: Option<String>,
    /// Версия портативного Node.js.
    pub node_version: Option<String>,
    pub port: u16,
    pub base_url: String,
    /// Папка установки (по умолчанию `<exe>/cloud_routers/<id>`).
    pub path: String,
    /// Каталог данных (DATA_DIR).
    pub data_dir: String,
    /// Наличие файла базы данных.
    pub db_present: bool,
    pub node_present: bool,
    pub server_present: bool,
    /// Человеко-читаемое сообщение для UI.
    pub message: String,
}

fn build_status(app: &AppHandle, router_id: RouterId) -> RouterStatus {
    let cfg = config::load_config(app, router_id);
    let dir = router_dir(app, router_id);
    let data_dir = config::router_data_dir(app, router_id);
    let db_present = data_dir.join("db").join("data.sqlite").exists()
        || data_dir.join("storage.sqlite").exists()
        || data_dir.join("omniroute.db").exists();
    let node_present = node_exe(&dir).exists();
    let server_present = server_script(&dist_dir(&dir), router_id).exists();
    // node.exe + server.js НЕ означают «установлено»: у OmniRoute зависимости
    // ставятся отдельным `npm install`, и прерванная установка оставляет
    // server.js без `next`. Без этой проверки UI врал бы «установлено»,
    // а get_combos поднимал бы заведомо падающий сервер.
    let deps_ok = installer::deps_present(&dir, router_id);
    let installed = node_present && server_present && deps_ok;
    let port = cfg.port_or_default(router_id);
    let gw = process::gateway_state(port, &dir, &data_dir);
    let running = installed && gw == process::GatewayState::OursRunning;

    let message = if !installed {
        if node_present && server_present && !deps_ok {
            // Не «не установлен», а «установка неполная» — разные действия
            // пользователя, значит и сообщение должно быть разным (§2.2 «no lies»).
            format!(
                "{}: установка неполная — зависимости не установлены. Нажмите «Установить» заново.",
                router_id
            )
        } else {
            format!("{} не установлен. Нажмите «Установить».", router_id)
        }
    } else if running {
        format!(
            "Активен на порту {}{}",
            port,
            cfg.installed_version
                .as_deref()
                .map(|v| format!(" (v{})", v))
                .unwrap_or_default()
        )
    } else if let process::GatewayState::ForeignOccupant(pid) = gw {
        if pid != 0 {
            if process::process_exe_matches(pid, &node_exe(&dir)) {
                format!(
                    "Порт {} держит {}, запущенный не из этого каталога данных (тот же node.exe). Нажмите «Остановить», чтобы поднять свой.",
                    port, router_id
                )
            } else {
                format!(
                    "Порт {} занят чужим процессом (pid {}). Остановите внешний роутер или смените порт.",
                    port, pid
                )
            }
        } else {
            format!(
                "Порт {} открыт, но {} не отвечает. Возможно, порт занят другим сервисом — смените порт в настройках.",
                port, router_id
            )
        }
    } else {
        "Установлен, не запущен".to_string()
    };

    RouterStatus {
        installed,
        running,
        version: cfg.installed_version.clone(),
        node_version: cfg.node_version.clone(),
        port,
        base_url: cfg.base_url(router_id),
        path: dir.to_string_lossy().to_string(),
        data_dir: data_dir.to_string_lossy().to_string(),
        db_present,
        node_present,
        server_present,
        message,
    }
}

/// Статус шлюза (для индикатора в UI). Не запускает сервер.
#[tauri::command]
pub async fn get_status(app: AppHandle, router: String) -> Result<RouterStatus, String> {
    let router_id = router.parse::<RouterId>()?;
    tauri::async_runtime::spawn_blocking(move || build_status(&app, router_id))
        .await
        .map_err(|e| format!("status task join error: {}", e))
}

/// Установить / обновить роутер (портативный Node.js + npm-бандл).
/// `force=true` — переустановить, даже если версия совпадает.
/// Прогресс шлётся событием `cloud-routers-progress` (`{router, stage, done, total, text}`).
#[tauri::command]
pub async fn install_or_update(app: AppHandle, router: String, force: Option<bool>) -> Result<RouterStatus, String> {
    let router_id = router.parse::<RouterId>()?;
    let app_evt = app.clone();
    let force = force.unwrap_or(false);
    let app_work = app.clone();
    let router_id_clone = router_id.clone();
    let router_id_for_closure = router_id.clone();
    let info = tauri::async_runtime::spawn_blocking(move || {
        // Держим блокировку установки весь прогон: сервер не должен
        // стартовать поверх полураспакованного dist.
        let _guard = installer::install_guard(router_id_clone);
        let progress: installer::ProgressFn = Box::new(move |stage: &str, done: u64, total: u64, text: &str| {
            let _ = app_evt.emit(
                "cloud-routers-progress",
                json!({ "router": router_id_for_closure, "stage": stage, "done": done, "total": total, "text": text }),
            );
        });
        installer::install_or_update(&app_work, router_id_clone, force, progress)
    })
    .await
    .map_err(|e| format!("install task join error: {}", e))??;

    log::info!("{} установлен: v{} (Node {})", router_id, info.version, info.node_version);
    Ok(build_status(&app, router_id))
}

/// Ленивый автозапуск по требованию: если сервер не запущен и установлен —
/// поднять; если уже жив — no-op. Ошибка, если не установлен.
#[tauri::command]
pub async fn ensure_started(app: AppHandle, router: String) -> Result<RouterStatus, String> {
    let router_id = router.parse::<RouterId>()?;
    let status = build_status(&app, router_id);
    if !status.installed {
        return Err(format!("{} не установлен. Откройте Настройки и нажмите «Установить».", router_id));
    }
    if status.running {
        return Ok(status);
    }
    let app_work = app.clone();
    let router_id_clone = router_id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Ждём завершения установки, если она идёт: иначе сервер поднимется
        // на неполном dist и упадёт с MODULE_NOT_FOUND.
        let _guard = installer::install_guard(router_id_clone);
        let cfg = config::load_config(&app_work, router_id_clone);
        process::start_server(&app_work, router_id_clone, &cfg)
    })
    .await
    .map_err(|e| format!("start task join error: {}", e))??;

    Ok(build_status(&app, router_id))
}

/// Остановить сервер роутера.
#[tauri::command]
pub fn stop(app: AppHandle, router: String) -> Result<RouterStatus, String> {
    let router_id = router.parse::<RouterId>()?;
    let cfg = config::load_config(&app, router_id);
    let port = cfg.port_or_default(router_id);
    log::info!("🛑 {}: запрос остановки (порт {})", router_id, port);
    let data_dir = config::router_data_dir(&app, router_id);
    process::stop_server(port, &data_dir);
    if process::wait_port_closed(port, Duration::from_secs(3)) {
        log::info!("✅ {} остановлен (порт {} закрыт)", router_id, port);
    } else {
        log::warn!("⚠️ {}: порт {} не закрылся за 3 сек", router_id, port);
    }
    Ok(build_status(&app, router_id))
}

/// Нормализует пользовательский путь: гарантирует, что последний компонент
/// совпадает с именем папки роутера (регистронезависимо). Если пользователь
/// выбрал родительскую папку — добавляет подпапку роутера. Создаёт папку.
fn normalize_router_dir(path: &str, router_id: RouterId) -> Result<PathBuf, String> {
    if path.trim().is_empty() {
        return Err("Путь установки не может быть пустым".to_string());
    }
    let mut buf = PathBuf::from(path);
    let needs_subdir = buf
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| !n.eq_ignore_ascii_case(router_id.dir_name()))
        .unwrap_or(true);
    if needs_subdir {
        buf.push(router_id.dir_name());
    }
    fs::create_dir_all(&buf).map_err(|e| format!("Не удалось создать папку установки: {}", e))?;
    Ok(buf)
}

/// Сменить папку установки роутера (по умолчанию `<exe>/cloud_routers/<id>`).
#[tauri::command]
pub fn set_router_dir(app: AppHandle, router: String, path: String) -> Result<RouterStatus, String> {
    let router_id = router.parse::<RouterId>()?;
    let normalized = normalize_router_dir(&path, router_id)?;
    let data_dir = config::router_data_dir(&app, router_id);
    let cfg_curr = config::load_config(&app, router_id);
    process::stop_server(cfg_curr.port_or_default(router_id), &data_dir);
    let mut cfg = cfg_curr;
    cfg.dir = Some(normalized.to_string_lossy().to_string());
    config::save_config(&app, router_id, &cfg)?;
    log::info!("{}: папка установки изменена: {}", router_id, cfg.dir.as_deref().unwrap_or(""));
    Ok(build_status(&app, router_id))
}

/// Список комбо роутера (только LLM). Если сервер не запущен и установлен —
/// стартует лениво, но только когда включён автозапуск.
#[tauri::command]
pub async fn get_combos(app: AppHandle, router: String) -> Result<Vec<client::ComboInfo>, String> {
    let router_id = router.parse::<RouterId>()?;
    let status = build_status(&app, router_id);
    if !status.installed {
        log::warn!("get_combos: {} не установлен", router_id);
        return Err(format!("{} не установлен.", router_id));
    }
    let cfg = config::load_config(&app, router_id);
    if !status.running {
        if !cfg.auto_start {
            log::warn!("get_combos: {} остановлен и автозапуск выключен", router_id);
            return Err(format!("{} остановлен и автозапуск выключен.", router_id));
        }
        log::info!("get_combos: {} не запущен, автозапуск...", router_id);
        let app_work = app.clone();
        let router_id_clone = router_id.clone();
        let cfg_work = cfg.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let _guard = installer::install_guard(router_id_clone);
            process::start_server(&app_work, router_id_clone, &cfg_work)
        })
        .await
        .map_err(|e| format!("start task join error: {}", e))??;
    }

    let base = cfg.base_url(router_id);
    let api_key = cfg.api_key.clone();
    log::info!("get_combos: запрос к {} ...", base);
    let base_clone = base.clone();
    match tauri::async_runtime::spawn_blocking(move || client::get_combos(&base_clone, api_key.as_deref()))
        .await
    {
        Ok(Ok(combos)) => {
            log::info!("get_combos: получено {} комбо от {}", combos.len(), base);
            Ok(combos)
        }
        Ok(Err(e)) => {
            log::error!("get_combos: ошибка получения комбо от {}: {}", base, e);
            Err(e)
        }
        Err(e) => {
            log::error!("get_combos: ошибка задачи: {}", e);
            Err(format!("combos task join error: {}", e))
        }
    }
}

/// Открыть веб-дашборд роутера в браузере по умолчанию.
#[tauri::command]
pub async fn open_dashboard(app: AppHandle, router: String) -> Result<(), String> {
    let router_id = router.parse::<RouterId>()?;
    let cfg = config::load_config(&app, router_id);
    let base_url = cfg.base_url(router_id);
    let health_url = base_url.clone();
    let healthy = tauri::async_runtime::spawn_blocking(move || client::is_healthy(&health_url))
        .await
        .map_err(|e| format!("health task join error: {}", e))?;
    if !healthy {
        let status = build_status(&app, router_id);
        if !status.installed {
            return Err(format!("{} не установлен.", router_id));
        }
        if !status.running {
            let app_work = app.clone();
            let router_id_clone = router_id.clone();
            tauri::async_runtime::spawn_blocking(move || {
                let _guard = installer::install_guard(router_id_clone);
                let cfg = config::load_config(&app_work, router_id_clone);
                process::start_server(&app_work, router_id_clone, &cfg)
            })
            .await
            .map_err(|e| format!("start task join error: {}", e))??;
        }
    }
    let url = format!("{}/dashboard", base_url);
    open_in_browser(&url);
    Ok(())
}

/// Проверить наличие обновления роутера (версия npm latest).
#[tauri::command]
pub async fn check_router_update(app: AppHandle, router: String) -> Result<Option<String>, String> {
    let router_id = router.parse::<RouterId>()?;
    tauri::async_runtime::spawn_blocking(move || {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| format!("HTTP client: {}", e))?;
        let latest = installer::latest_npm_version(&client, router_id)?;
        let cfg = config::load_config(&app, router_id);
        if let Some(installed) = &cfg.installed_version {
            if installed == &latest {
                return Ok(None);
            }
        }
        Ok(Some(latest))
    })
    .await
    .map_err(|e| format!("check update task error: {}", e))?
}

/// Чат через роутер (OpenAI-совместимый, стриминг).
/// Каждая порция текста шлётся событием `cloud-routers-chunk` (`{router, text, author, kind}`).
#[tauri::command]
pub async fn chat_completion(
    app: AppHandle,
    router: String,
    model: String,
    messages: Vec<ChatMessage>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    author: Option<String>,
) -> Result<String, String> {
    let router_id = router.parse::<RouterId>()?;
    let cfg = config::load_config(&app, router_id);
    if !build_status(&app, router_id).installed {
        return Err(format!("{} не установлен.", router_id));
    }
    let api_key = cfg.api_key.clone();
    let app_work = app.clone();
    let router_id_clone = router_id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = installer::install_guard(router_id_clone);
        let cfg_inner = config::load_config(&app_work, router_id_clone);
        process::start_server(&app_work, router_id_clone, &cfg_inner)
    })
    .await
    .map_err(|e| format!("start task join error: {}", e))??;

    let base = cfg.base_url(router_id);
    let req = ChatRequest {
        model,
        messages,
        max_tokens,
        temperature,
        tools: None,
        tool_choice: None,
        stream: true,
    };
    let author = author.unwrap_or_default();
    let full = tauri::async_runtime::spawn_blocking(move || {
        client::chat_completion_stream(&base, api_key.as_deref(), &req, |delta| {
            let _ = app.emit(
                "cloud-routers-chunk",
                json!({ "router": router_id, "text": delta, "author": author, "kind": "message" }),
            );
        })
    })
    .await
    .map_err(|e| format!("chat task join error: {}", e))??;

    Ok(full)
}

/// Сохранить API-ключ роутера (для `/v1/chat/completions`).
#[tauri::command]
pub fn set_api_key(app: AppHandle, router: String, key: Option<String>) -> Result<RouterStatus, String> {
    let router_id = router.parse::<RouterId>()?;
    let mut cfg = config::load_config(&app, router_id);
    cfg.api_key = key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty());
    if let Err(e) = config::save_config(&app, router_id, &cfg) {
        log::error!("{}: ошибка сохранения API-ключа: {}", router_id, e);
    }
    if cfg.api_key.is_some() {
        log::info!("{}: API-ключ сохранён", router_id);
    } else {
        log::info!("{}: API-ключ очищен", router_id);
    }
    Ok(build_status(&app, router_id))
}

/// Открыть URL в браузере по умолчанию (без лишних крейтов).
fn open_in_browser(url: &str) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = std::process::Command::new("cmd");
        cmd.args(["/C", "start", "", url]);
        cmd.creation_flags(0x08000000);
        let _ = cmd.spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_router_dir_appends_subdir_for_parent_path() {
        let tmp = std::env::temp_dir().join("cr_test_parent");
        let _ = fs::remove_dir_all(&tmp);
        let result = normalize_router_dir(tmp.to_str().unwrap(), RouterId::OmniRoute).unwrap();
        assert_eq!(result, tmp.join("omniroute"));
        assert!(result.exists());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn normalize_router_dir_keeps_correct_subdir() {
        let tmp = std::env::temp_dir().join("cr_test_correct");
        let _ = fs::remove_dir_all(&tmp);
        let target = tmp.join("omniroute");
        fs::create_dir_all(&target).unwrap();
        let result = normalize_router_dir(target.to_str().unwrap(), RouterId::OmniRoute).unwrap();
        assert_eq!(result, target);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn normalize_router_dir_case_insensitive() {
        let tmp = std::env::temp_dir().join("cr_test_case");
        let _ = fs::remove_dir_all(&tmp);
        let target = tmp.join("OMNIROUTE");
        fs::create_dir_all(&target).unwrap();
        let result = normalize_router_dir(target.to_str().unwrap(), RouterId::OmniRoute).unwrap();
        assert_eq!(result, target);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn normalize_router_dir_empty_path_errors() {
        assert!(normalize_router_dir("", RouterId::OmniRoute).is_err());
        assert!(normalize_router_dir("   ", RouterId::OmniRoute).is_err());
    }

    #[test]
    fn server_script_relative_paths() {
        assert_eq!(RouterId::NineRouter.server_script_relative(), "app/custom-server.js");
        assert_eq!(RouterId::ExtremeRouter.server_script_relative(), "app/custom-server.js");
        assert_eq!(RouterId::OmniRoute.server_script_relative(), "dist/server.js");
    }
}
