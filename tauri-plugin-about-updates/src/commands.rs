use std::path::PathBuf;
use std::process::Command;

use tauri::{AppHandle, Manager, Runtime};

use crate::models::ReleaseInfo;
use crate::PluginState;

/// Список доступных релизов через GitHub Releases REST API.
/// Репозиторий берётся из состояния плагина (задано в tauri.conf.json хоста).
#[tauri::command]
pub async fn get_release_history<R: Runtime>(app: AppHandle<R>) -> Result<Vec<ReleaseInfo>, String> {
    let repo = app.state::<PluginState>().repo.clone();

    let client = reqwest::Client::builder()
        .user_agent("tauri-plugin-about-updates")
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .get(format!(
            "https://api.github.com/repos/{repo}/releases?per_page=100"
        ))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("GitHub API error: {}", resp.status()));
    }

    let releases: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let current = app.package_info().version.to_string();

    let mut out: Vec<ReleaseInfo> = Vec::new();
    let arr = releases.as_array().ok_or("Некорректный ответ GitHub API")?;

    for rel in arr {
        let tag = rel
            .get("tag_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let version = tag.trim_start_matches('v').to_string();
        if version.is_empty() {
            continue;
        }

        let pub_date = rel
            .get("published_at")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let notes = rel
            .get("body")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let mut download_url = String::new();
        if let Some(assets) = rel.get("assets").and_then(|v| v.as_array()) {
            if let Some(a) = assets.iter().find(|a| {
                let n = a
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                n.ends_with("-setup.exe") && !n.ends_with(".sig")
            }) {
                download_url = a
                    .get("browser_download_url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
            }
        }
        if download_url.is_empty() {
            continue;
        }

        out.push(ReleaseInfo {
            version: version.clone(),
            pub_date,
            notes,
            download_url,
            is_current: version == current,
        });
    }

    Ok(out)
}

/// Откат к конкретной версии.
///
/// Единственный источник правды — GitHub Releases. Фронтенд передаёт сюда реальный
/// URL установщика (`download_url`, полученный из GitHub API в `get_release_history`),
/// мы качаем ровно этот ассет и запускаем NSIS-инсталлер (тихо, с даунгрейдом —
/// `allowDowngrades` включён в `tauri.conf.json` хоста).
#[tauri::command]
pub async fn install_release<R: Runtime>(app: AppHandle<R>, download_url: String) -> Result<(), String> {
    // 1. Бэкап данных перед понижением версии.
    crate::updater_rollback::backup_before_rollback(&app)?;

    // 2. Имя установщика из URL.
    let file_name = download_url
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Некорректный URL установщика".to_string())?
        .to_string();

    // 3. Скачивание установщика единым движком (прогресс, без лишних окон).
    let installer_path: PathBuf = std::env::temp_dir().join(&file_name);
    tauri_plugin_downloader::download(
        &download_url,
        &installer_path,
        tauri_plugin_downloader::DownloadOptions {
            label: format!("Установщик {}", file_name),
            kind: "app".into(),
            ..Default::default()
        },
        Some(&|msg: String| {
            log::info!("[about-updates] {}", msg);
        }),
    )
    .await
    .map_err(|e| format!("Ошибка загрузки установщика: {}", e))?;

    // 4. Запуск инсталлера в тихом режиме и авто-перезапуск приложения после
    //    переустановки. NSIS в режиме /S НЕ перезапускает приложение сам, поэтому
    //    запускаем отсоединённый cmd, который дожидается завершения инсталлера
    //    (start /wait) и затем сам запускает обновлённый exe (start "" <exe>).
    let app_exe = std::env::current_exe()
        .map_err(|e| format!("Не удалось получить путь к exe: {}", e))?;
    let installer_str = installer_path.display().to_string();
    let app_str = app_exe.display().to_string();

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        let relaunch_script = format!(
            "start \"\" /wait \"{}\" /S & start \"\" \"{}\"",
            installer_str, app_str
        );
        Command::new("cmd")
            .args(["/c", &relaunch_script])
            .creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS)
            .spawn()
            .map_err(|e| format!("Не удалось запланировать перезапуск: {}", e))?;
    }
    #[cfg(not(windows))]
    {
        let relaunch_script = format!("\"{}\" /S; \"{}\"", installer_str, app_str);
        Command::new("sh")
            .args(["-c", &relaunch_script])
            .spawn()
            .map_err(|e| format!("Не удалось запланировать перезапуск: {}", e))?;
    }

    // Завершаем текущий процесс, чтобы он не держал заблокированным свой exe
    // (иначе тихая переустановка не сможет заменить файлы). Перезапуск выполнит
    // отсоединённый релаунчер выше.
    std::process::exit(0);
}

/// Версия хост-приложения (из его Cargo.toml / tauri.conf.json).
#[tauri::command]
pub fn get_app_version<R: Runtime>(app: AppHandle<R>) -> String {
    app.package_info().version.to_string()
}

/// Настроенная ссылка «Поддержать автора» (или null, если не задана).
#[tauri::command]
pub fn get_support_url<R: Runtime>(app: AppHandle<R>) -> Option<String> {
    app.state::<PluginState>().support_url.clone()
}
