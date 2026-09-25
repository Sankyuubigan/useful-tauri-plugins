//! Tauri-команды плагина 9router. Тонкий слой: вся логика — в `crate::router`.

use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter};

use crate::router::{
    client::{self, ChatMessage, ChatRequest},
    config::{self, dist_dir, node_exe, router_dir, server_script},
    installer, process,
};

/// Состояние шлюза 9router для UI (индикатор + кнопки).
#[derive(Serialize, Clone)]
pub struct NineRouterStatus {
    /// Установлены и node.exe, и бандл 9router.
    pub installed: bool,
    /// Порт отвечает (сервер жив).
    pub running: bool,
    /// Версия 9router (из npm-тега, зафиксированная при установке).
    pub version: Option<String>,
    /// Версия портативного Node.js.
    pub node_version: Option<String>,
    pub port: u16,
    pub base_url: String,
    /// Папка установки (по умолчанию <exe>/9router).
    pub path: String,
    pub node_present: bool,
    pub server_present: bool,
    /// Человеко-читаемое сообщение для UI.
    pub message: String,
}

fn build_status(app: &AppHandle) -> NineRouterStatus {
    let cfg = config::load_config(app);
    let dir = router_dir(app);
    let node_present = node_exe(&dir).exists();
    let server_present = server_script(&dist_dir(&dir)).exists();
    let installed = node_present && server_present;
    let port = cfg.port_or_default();
    // «Запущен» — только если порт держит НАШ портативный node.exe. Чужой
    // процесс на порту (глобальный npm 9Router и т.п.) сервером приложения НЕ
    // является: иначе панель врут (инцидент: внешний 9Router на порту 20128
    // выглядел «работающим» при ненастроенном бандле).
    let gw = process::gateway_state(port, &dir);
    let running = gw == process::GatewayState::OursRunning;

    let message = if !installed {
        "9Router не установлен. Нажмите «Установить».".to_string()
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
        // pid=0 — владелец не определён (listener_pid=None): не врём про «чужой»
        // процесс, если health-check уже прошёл выше; иначе честно про порт.
        if pid != 0 {
            format!(
                "Порт {} занят чужим процессом (pid {}). Остановите внешний 9Router или смените порт.",
                port, pid
            )
        } else {
            format!(
                "Порт {} открыт, но 9Router не отвечает. Возможно, порт занят другим сервисом — смените порт в настройках.",
                port
            )
        }
    } else {
        "Установлен, не запущен".to_string()
    };

    NineRouterStatus {
        installed,
        running,
        version: cfg.installed_version.clone(),
        node_version: cfg.node_version.clone(),
        port,
        base_url: cfg.base_url(),
        path: dir.to_string_lossy().to_string(),
        node_present,
        server_present,
        message,
    }
}

/// Статус шлюза (для индикатора в UI). Не запускает сервер.
#[tauri::command]
pub async fn get_status(app: AppHandle) -> Result<NineRouterStatus, String> {
    tauri::async_runtime::spawn_blocking(move || build_status(&app))
        .await
        .map_err(|e| format!("status task join error: {}", e))
}

/// Установить / обновить 9router (портативный Node.js + npm-бандл).
/// `force=true` — переустановить, даже если версия совпадает.
/// Прогресс шлётся событием `9router-progress` (`{stage, done, total, text}`).
#[tauri::command]
pub async fn install_or_update(app: AppHandle, force: Option<bool>) -> Result<NineRouterStatus, String> {
    let app_evt = app.clone();
    let force = force.unwrap_or(false);
    let app_work = app.clone();
    let info = tauri::async_runtime::spawn_blocking(move || {
        let progress: installer::ProgressFn = Box::new(move |stage: &str, done: u64, total: u64, text: &str| {
            let _ = app_evt.emit(
                "9router-progress",
                json!({ "stage": stage, "done": done, "total": total, "text": text }),
            );
        });
        installer::install_or_update(&app_work, force, progress)
    })
    .await
    .map_err(|e| format!("install task join error: {}", e))??;

    log::info!("9router установлен: v{} (Node {})", info.version, info.node_version);
    Ok(build_status(&app))
}

/// Ленивый автозапуск по требованию: если сервер не запущен и установлен —
/// поднять; если уже жив — no-op. Ошибка, если не установлен.
#[tauri::command]
pub async fn ensure_started(app: AppHandle) -> Result<NineRouterStatus, String> {
    let status = build_status(&app);
    if !status.installed {
        return Err("9Router не установлен. Откройте Настройки → 9Router и нажмите «Установить».".to_string());
    }
    if status.running {
        return Ok(status);
    }
    let app_work = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = config::load_config(&app_work);
        process::start_server(&app_work, &cfg)
    })
    .await
    .map_err(|e| format!("start task join error: {}", e))??;

    Ok(build_status(&app))
}

/// Остановить сервер 9router.
#[tauri::command]
pub fn stop(app: AppHandle) -> NineRouterStatus {
    process::stop_server();
    build_status(&app)
}

/// Сменить папку установки 9router (по умолчанию `<exe>/9router`).
///
/// Сохраняет путь в `nine_router.dir` и возвращает статус под новый путь:
/// `installed` пересчитывается по фактическому наличию `node.exe` и серверного
/// скрипта в выбранной папке (как `set_engine_dir` у llama-engine). Работающий
/// из старой папки сервер останавливается, чтобы статус не показывал
/// «running» по серверу, которого в новом пути нет.
#[tauri::command]
pub fn set_router_dir(app: AppHandle, path: String) -> Result<NineRouterStatus, String> {
    if path.trim().is_empty() {
        return Err("Путь установки не может быть пустым".to_string());
    }
    process::stop_server();
    let mut cfg = config::load_config(&app);
    cfg.dir = Some(path);
    config::save_config(&app, &cfg);
    log::info!(
        "9router: папка установки изменена: {}",
        cfg.dir.as_deref().unwrap_or("")
    );
    Ok(build_status(&app))
}

/// Список комбо 9router (только LLM). Если сервер не запущен и установлен —
/// стартует лениво (открытие 9router-группы в дропдауне = спрос).
#[tauri::command]
pub async fn get_combos(app: AppHandle) -> Result<Vec<client::ComboInfo>, String> {
    let status = build_status(&app);
    if !status.installed {
        return Err("9Router не установлен.".to_string());
    }
    if !status.running {
        let app_work = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let cfg = config::load_config(&app_work);
            process::start_server(&app_work, &cfg)
        })
        .await
        .map_err(|e| format!("start task join error: {}", e))??;
    }

    let cfg = config::load_config(&app);
    let base = cfg.base_url();
    let api_key = cfg.api_key.clone();
    tauri::async_runtime::spawn_blocking(move || client::get_combos(&base, api_key.as_deref()))
        .await
        .map_err(|e| format!("combos task join error: {}", e))?
}

/// Открыть веб-дашборд 9router в браузере по умолчанию.
#[tauri::command]
pub async fn open_dashboard(app: AppHandle) -> Result<(), String> {
    let cfg = config::load_config(&app);
    let base_url = cfg.base_url();
    let health_url = base_url.clone();
    let healthy = tauri::async_runtime::spawn_blocking(move || client::is_healthy(&health_url))
        .await
        .map_err(|e| format!("health task join error: {}", e))?;
    if !healthy {
        let status = build_status(&app);
        if !status.installed {
            return Err("9Router не установлен.".to_string());
        }
        if !status.running {
            let app_work = app.clone();
            tauri::async_runtime::spawn_blocking(move || {
                let cfg = config::load_config(&app_work);
                process::start_server(&app_work, &cfg)
            })
            .await
            .map_err(|e| format!("start task join error: {}", e))??;
        }
    }
    let url = format!("{}/dashboard", base_url);
    open_in_browser(&url);
    Ok(())
}

/// Проверить наличие обновления 9router (версия npm latest).
/// Возвращает Some(новая_версия) или None, если установленная версия актуальна.
#[tauri::command]
pub async fn check_router_update(app: AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| format!("HTTP client: {}", e))?;
        let latest = installer::latest_npm_version(&client)?;
        let cfg = config::load_config(&app);
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

/// Чат через 9router (OpenAI-совместимый, стриминг).
/// Каждая порция текста шлётся событием `9router-chunk` (`{text, author, kind}`),
/// полный ответ также возвращается из команды.
#[tauri::command]
pub async fn chat_completion(
    app: AppHandle,
    model: String,
    messages: Vec<ChatMessage>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    author: Option<String>,
) -> Result<String, String> {
    let cfg = config::load_config(&app);
    if !build_status(&app).installed {
        return Err("9Router не установлен.".to_string());
    }
    let api_key = cfg.api_key.clone();
    let app_work = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let cfg_inner = config::load_config(&app_work);
        process::start_server(&app_work, &cfg_inner)
    })
    .await
    .map_err(|e| format!("start task join error: {}", e))??;

    let base = cfg.base_url();
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
                "9router-chunk",
                json!({ "text": delta, "author": author, "kind": "message" }),
            );
        })
    })
    .await
    .map_err(|e| format!("chat task join error: {}", e))??;

    Ok(full)
}

/// Сохранить API-ключ 9router (для `/v1/chat/completions`). Пустая строка —
/// очистить сохранённый ключ. Возвращает статус шлюза.
#[tauri::command]
pub fn set_api_key(app: AppHandle, key: Option<String>) -> NineRouterStatus {
    let mut cfg = config::load_config(&app);
    cfg.api_key = key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty());
    config::save_config(&app, &cfg);
    if cfg.api_key.is_some() {
        log::info!("9router: API-ключ сохранён");
    } else {
        log::info!("9router: API-ключ очищен");
    }
    build_status(&app)
}

/// Открыть URL в браузере по умолчанию (без лишних крейтов).
fn open_in_browser(url: &str) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = std::process::Command::new("cmd");
        cmd.args(["/C", "start", "", url]);
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
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