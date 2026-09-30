use tauri::{AppHandle, Manager, Runtime};

use crate::install_report::{self, InstallKind, InstallReport};
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
        .map_err(|e| {
            log::error!("[about-updates] HTTP-клиент: {}", e);
            format!("Ошибка создания HTTP-клиента: {}", e)
        })?;

    let url = format!("https://api.github.com/repos/{repo}/releases?per_page=100");
    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| {
            log::error!("[about-updates] запрос истории релизов не удался: {}", e);
            format!("Ошибка запроса GitHub: {}", e)
        })?;

    if !resp.status().is_success() {
        log::error!("[about-updates] GitHub API вернул HTTP {}", resp.status());
        return Err(format!("GitHub API error: {}", resp.status()));
    }

    let releases: serde_json::Value = resp.json().await.map_err(|e| {
        log::error!("[about-updates] ответ GitHub API не распознан: {}", e);
        format!("Некорректный ответ GitHub API: {}", e)
    })?;
    let current = app.package_info().version.to_string();

    let mut out: Vec<ReleaseInfo> = Vec::new();
    let arr = releases.as_array().ok_or_else(|| {
        log::error!("[about-updates] ответ GitHub API не массив");
        "Некорректный ответ GitHub API".to_string()
    })?;

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
            log::warn!("[about-updates] у релиза {} нет ассета -setup.exe, пропускаем", version);
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

    log::info!("[about-updates] история релизов: {} шт.", out.len());
    Ok(out)
}

/// Откат к конкретной версии.
///
/// Единственный источник правды — GitHub Releases. Фронтенд передаёт сюда реальный
/// URL установщика (`download_url`, полученный из GitHub API в `get_release_history`)
/// и версию, мы качаем ровно этот ассет и запускаем NSIS-инсталлер
/// (даунгрейд разрешён — `allowDowngrades` включён в `tauri.conf.json` хоста).
///
/// Запуск и перезапуск — на стороне инсталлера (`/P /UPDATE /R`, см. `installer.rs`),
/// поэтому здесь только бэкап данных, скачивание и выход приложения.
#[tauri::command]
pub async fn install_release<R: Runtime>(
    app: AppHandle<R>,
    download_url: String,
    version: String,
) -> Result<(), String> {
    log::info!("[about-updates] откат: запрос на установку {}", download_url);

    // 1. Бэкап данных перед понижением версии. Для отката это обязательно:
    //    после даунгрейда старая версия может не прочитать конфиг/сессии новее.
    crate::updater_rollback::backup_before_rollback(&app).map_err(|e| {
        log::error!("[about-updates] бэкап перед откатом не удался: {}", e);
        format!("Не удалось создать бэкап перед откатом: {}", e)
    })?;

    // 2. Скачивание, отчёт, запуск инсталлера, выход приложения.
    crate::installer::run_install(&app, InstallKind::Rollback, &version, &download_url).await
}

/// Отчёт о последней установке (откат/обновление) с вердиктом.
///
/// Вызывается фронтендом или при диагностике; попутно разрешает «висящий» отчёт,
/// если приложение было запущено вручную после неудачной установки.
#[tauri::command]
pub fn get_install_report<R: Runtime>(app: AppHandle<R>) -> Option<InstallReport> {
    install_report::resolve_pending(&app).or_else(|| install_report::read(&app))
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
