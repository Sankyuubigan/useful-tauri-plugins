//! Подготовка установщика релиза: разбор URL, скачивание, запуск NSIS.
//!
//! Выделено из команды `install_release` отдельным модулем, чтобы логика
//! подготовки была покрыта тестами, а команда оставалась тонкой оболочкой
//! (бэкап → подготовка → запуск инсталлера → выход).
//!
//! ## Почему установщик запускается напрямую, а НЕ через `cmd /c`
//!
//! Прежняя реализация собирала строку `start "" /wait "<exe>" /S & start "" "<app>"` и
//! передавала её в `cmd /c`. Это ломалось: `std::process::Command` экранирует кавычки по
//! правилам C-runtime (`\"`), а `cmd.exe` по этим правилам не разбирает — `\` становится
//! частью имени файла, путь вида `\\...` трактуется как UNC и Windows выдаёт
//! `ERROR_BAD_NETPATH` («Не найден сетевой путь»). Оболочка здесь не нужна.
//!
//! ## Что NSIS делает сам (шаблон `installer.nsi` Tauri, начиная с 2.0.0)
//!
//! - `/P` — passive: окно прогресса, авто-убийство ещё живой копии приложения,
//!   видимые ошибки (в `/S` неудача заканчивается тихим `Abort`);
//! - `/UPDATE` — установка поверх: без uninstall-диалога и без удаления данных
//!   приложения;
//! - `/R` — функция `.onInstSuccess` запускает `nsis_tauri_utils::RunAsUser
//!   "$INSTDIR\<main>.exe"`, то есть приложение перезапускает сам инсталлер.
//!
//! Эталон — официальный `tauri-plugin-updater`: `ShellExecuteW(installer, "/P /UPDATE /R")`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use tauri::Runtime;
use tauri_plugin_downloader::DownloadOptions;

use crate::install_report::{self, InstallKind};

/// Аргументы запуска NSIS-инсталлера. Единственный источник правды (SSOT):
/// и плагин, и хост запускают установщик одинаково.
const NSIS_ARGS: [&str; 3] = ["/P", "/UPDATE", "/R"];

/// Имя файла установщика из URL ассета GitHub Releases.
/// Пустой путь и завершающий слэш считаются некорректным URL.
pub fn installer_file_name(download_url: &str) -> Result<String, String> {
    download_url
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("Некорректный URL установщика: {}", download_url))
}

/// Путь установщика в системном temp. Имя берётся из URL — единственный
/// источник правды, чтобы не рассчитывать его дважды.
pub fn installer_path(download_url: &str) -> Result<(String, PathBuf), String> {
    let file_name = installer_file_name(download_url)?;
    let path = std::env::temp_dir().join(&file_name);
    Ok((file_name, path))
}

/// План запуска внешней программы: исполняемый файл + аргументы.
/// Отдельная структура, чтобы план можно было проверить в тестах, не запуская
/// ничего, и чтобы нельзя было случайно протащить в запуск оболочку.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

/// План запуска NSIS-инсталлера в пассивном режиме с перезапуском приложения.
pub fn nsis_launch_plan(installer: &Path) -> LaunchPlan {
    LaunchPlan {
        program: installer.to_path_buf(),
        args: NSIS_ARGS.iter().map(OsString::from).collect(),
    }
}

/// Скачивает установщик выбранного релиза в temp.
/// Возвращает (имя файла, путь к скачанному файлу).
/// Логи идёт через единый движок `tauri-plugin-downloader`.
pub async fn download_installer(download_url: &str) -> Result<(String, PathBuf), String> {
    let (file_name, installer_path) = installer_path(download_url)?;
    log::info!(
        "[about-updates] скачивание установщика: {} -> {}",
        download_url,
        installer_path.display()
    );

    tauri_plugin_downloader::download(
        download_url,
        &installer_path,
        DownloadOptions {
            label: format!("Установщик {}", file_name),
            kind: "app".into(),
            ..Default::default()
        },
        None,
    )
    .await
    .map_err(|e| {
        log::error!(
            "[about-updates] не удалось скачать установщик {}: {}",
            download_url,
            e
        );
        format!("Ошибка загрузки установщика: {}", e)
    })?;

    log::info!("[about-updates] установщик скачан: {}", installer_path.display());
    Ok((file_name, installer_path))
}

/// Запустить подготовленный план. Windows-only: NSIS-установщик существует
/// только для Windows, мёртвый `sh -c "*.exe"` на других ОС удалён.
#[cfg(windows)]
pub fn spawn_installer(plan: &LaunchPlan) -> Result<u32, String> {
    use std::os::windows::process::CommandExt;

    // Правило §6.8 global_ai_docs: любой Command на Windows задаёт CREATE_NO_WINDOW.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    log::info!(
        "[about-updates] запуск установщика: {} {:?}",
        plan.program.display(),
        plan.args
    );

    let child = std::process::Command::new(&plan.program)
        .args(&plan.args)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| {
            log::error!(
                "[about-updates] не удалось запустить установщик {}: {}",
                plan.program.display(),
                e
            );
            format!("Не удалось запустить установщик: {}", e)
        })?;

    let pid = child.id();
    log::info!("[about-updates] установщик запущен, PID {}", pid);
    Ok(pid)
}

#[cfg(not(windows))]
pub fn spawn_installer(_plan: &LaunchPlan) -> Result<u32, String> {
    let err = "Запуск NSIS-инсталлера поддерживается только на Windows".to_string();
    log::error!("[about-updates] {}", err);
    Err(err)
}

/// Полный цикл установки релиза: скачать → зафиксировать намерение в отчёте →
/// запустить инсталлер → закрыть приложение.
///
/// Единственная точка, которой пользуются и откат (плагин), и резервное
/// обновление (хост) — иначе эти пути разъезжаются по флагам и логированию.
///
/// Перезапуск приложения после установки выполняет сам NSIS (`/R`), поэтому
/// здесь нужен только выход: он освобождает exe приложения для замены.
pub async fn run_install<R: Runtime>(
    app: &tauri::AppHandle<R>,
    kind: InstallKind,
    version: &str,
    download_url: &str,
) -> Result<(), String> {
    log::info!(
        "[about-updates] установка релиза {} ({}) из {}",
        version,
        kind.as_str(),
        download_url
    );

    let (_file_name, installer_path) = download_installer(download_url).await?;

    // Отчёт пишется ДО запуска: после выхода приложения записать в лог уже нечем.
    // Вердикт («состоялось / не состоялось») выносится на старт следующей сессии
    // и определяется сверкой фактически установленной версии.
    install_report::begin(app, kind, version, &installer_path)?;

    let plan = nsis_launch_plan(&installer_path);
    if let Err(e) = spawn_installer(&plan) {
        install_report::fail(app, &e);
        return Err(e);
    }

    log::info!(
        "[about-updates] приложение закрывается; перезапуск выполнит NSIS (/R), отчёт: {}",
        install_report::report_path(app)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|e| format!("<недоступен: {}>", e))
    );

    // Штатный выход: срабатывает RunEvent::ExitRequested — движки убиваются,
    // отчёты об ошибках флашатся, в лог попадает «Приложение закрыто».
    app.exit(0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_name_taken_from_url() {
        let (name, path) = installer_path(
            "https://github.com/o/r/releases/download/v1.2.3/App_1.2.3_x64-setup.exe",
        )
        .unwrap();
        assert_eq!(name, "App_1.2.3_x64-setup.exe");
        assert!(path.ends_with("App_1.2.3_x64-setup.exe"));
        assert_eq!(path.parent().unwrap(), std::env::temp_dir());
    }

    #[test]
    fn trailing_slash_is_invalid_url() {
        assert!(installer_file_name("https://github.com/o/r/releases/download/").is_err());
    }

    /// План установки = сам установщик + ровно три флага NSIS.
    /// Никаких `cmd`, `/c`, `start`, `&` и экранирования кавычек: оболочка
    /// не участвует, а значит не может испортить команду (баг «Не найден сетевой путь»).
    #[test]
    fn nsis_plan_is_installer_plus_three_flags() {
        let plan = nsis_launch_plan(Path::new(r"C:\Temp\App_1.0_x64-setup.exe"));
        assert_eq!(plan.program, Path::new(r"C:\Temp\App_1.0_x64-setup.exe"));
        assert_eq!(plan.args, vec![OsString::from("/P"), OsString::from("/UPDATE"), OsString::from("/R")]);

        let rendered: Vec<String> = plan.args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
        for arg in rendered {
            assert!(!arg.contains('\\'), "аргумент не должен содержать экранирование: {}", arg);
            assert!(!arg.contains('"'), "аргумент не должен содержать кавычки: {}", arg);
        }
    }

    /// Строгая защита от возврата к `cmd /c ... start ...` (регрессия бага).
    #[test]
    fn nsis_plan_never_shells_out() {
        let plan = nsis_launch_plan(Path::new(r"C:\Temp\App_1.0_x64-setup.exe"));
        let program = plan.program.to_string_lossy().to_lowercase();
        for forbidden in ["cmd", "sh", "powershell", "start"] {
            assert!(
                !program.contains(forbidden),
                "исполняемый файл не должен быть оболочкой: {}",
                program
            );
        }
    }
}
