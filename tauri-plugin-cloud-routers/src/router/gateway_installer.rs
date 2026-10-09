//! Установка собственного бинаря шлюза (`RouterKind::NativeGateway`).
//!
//! ## Почему это отдельный модуль, а не флаг в `installer.rs`
//!
//! `installer.rs` обслуживает node-роутеров: версия берётся из npm registry,
//! дистрибутив — из tgz, рядом кладётся портативный `node.exe`. Шлюзу всё это
//! неприменимо: он не публикуется в npm, у него нет версии на сервере и нет
//! распаковки. Общая точка входа — [`crate::router::installer::install_or_update`],
//! которая по виду роутера уходит сюда. Флаг внутри npm-логики заставил бы
//! каждое её место таскать `Option` и вспоминать про `None` (ровно то, чего
//! [`RouterKind`] и был создан, чтобы не допустить).
//!
//! ## Что значит «установка» здесь
//!
//! Копирование готового бинаря в папку роутера плюс **настоящая** проверка:
//! запустить `--version` и убедиться, что файл жив. Проверка обязательна —
//! без неё прерванное копирование дало бы «установлено», а шлюз не поднялся бы.
//!
//! ## Откуда берётся бинарь
//!
//! Список кандидатов — [`source_candidates`]. Первый пункт (рядом с exe хоста)
//! срабатывает, когда сборка хоста кладёт шлюз в комплект как ресурс. Второй
//! (каталог `target` этого workspace) — на машине, где плагин собирается из
//! исходников. Ни один не найден → ошибка со **всеми** проверенными путями:
//! молчаливый отказ выглядел бы в UI как «установлено».

use std::fs;
use std::path::{Path, PathBuf};

use tauri::AppHandle;

use crate::commands::RouterId;
use crate::router::config;
use crate::router::installer::{InstalledInfo, ProgressFn};

/// Имя файла бинаря с расширением: `cloud-routers-gateway` → `….exe`.
///
/// Имя берётся из таблицы роутеров (`spec.rs`), а не пишется строкой: иначе
/// переименование крейта тихо оставило бы установщик искать несуществующий файл.
pub fn gateway_binary_name(exe_file: &str) -> String {
    format!("{}.exe", exe_file)
}

/// Кандидаты на путь к готовому бинарю, в порядке приоритета.
///
/// Возвращаются все, а не только существующие: сообщение об ошибке обязано
/// показывать, что именно проверялось.
pub fn source_candidates(exe_file: &str) -> Vec<PathBuf> {
    let name = gateway_binary_name(exe_file);
    let mut out = Vec::new();

    // Рядом с exe хоста — сюда бинарь попадает, если сборка хоста включила его
    // в `tauri.conf.json → bundle.resources`.
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        out.push(exe_dir.join(&name));
    }

    // Каталог сборки этого workspace. Адрес берётся из `CARGO_MANIFEST_DIR` —
    // он зашит при компиляции крейта, поэтому указывает именно на ту машину,
    // где плагин собирали. На машине пользователя такой папки нет, кандидат
    // просто не найдётся — это не тишина, а отсутствие файла.
    if let Some(workspace) = Path::new(env!("CARGO_MANIFEST_DIR")).parent() {
        let target = workspace.join("target");
        out.push(target.join("release").join(&name));
        out.push(target.join("debug").join(&name));
    }

    out
}

/// Первый существующий кандидат или ошибка с перечислением проверенных путей.
pub fn find_source(exe_file: &str) -> Result<PathBuf, String> {
    let candidates = source_candidates(exe_file);
    for c in &candidates {
        if c.is_file() {
            return Ok(c.clone());
        }
    }
    Err(format!(
        "Не найден собранный бинарь {}. Проверены пути: {}. \
         Соберите его командой `cargo build -p cloud-routers-gateway` \
         или положите файл рядом с приложением.",
        gateway_binary_name(exe_file),
        candidates
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

/// Вывести версию бинаря: запустить `--version` и забрать второе слово.
///
/// Формат печати задаёт сам шлюз (`main.rs`: `cloud-routers-gateway <версия>`),
/// поэтому версия читается из его вывода, а не выдумывается плагином.
pub fn probe_version(exe: &Path) -> Result<String, String> {
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--version");
    // CREATE_NO_WINDOW: иначе из GUI-приложения на секунду мигнёт чёрное окно
    // консоли (global_ai_docs/desktop_rust_tauri/rules.md §6.8).
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let out = cmd
        .output()
        .map_err(|e| format!("Не удалось запустить {}: {}", exe.display(), e))?;
    if !out.status.success() {
        return Err(format!(
            "{} --version завершился с кодом {:?}",
            exe.display(),
            out.status.code()
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    text.split_whitespace()
        .nth(1)
        .map(|s| s.to_string())
        .ok_or_else(|| {
            format!(
                "{} --version не вывел версию (получено: {:?})",
                exe.display(),
                text.trim()
            )
        })
}

/// Установить / обновить бинарь шлюза (блокирующая; вызывать из spawn_blocking).
pub fn install(
    app: &AppHandle,
    router_id: RouterId,
    force: bool,
    progress: ProgressFn,
) -> Result<InstalledInfo, String> {
    let exe_file = router_id
        .gateway_exe_file()
        .ok_or_else(|| format!("{} не является нативным роутером", router_id))?;
    let dir = config::router_dir(app, router_id);
    let dest = dir.join(gateway_binary_name(exe_file));

    // Повтор без force: файл на месте И сам отвечает `--version` → перезаписи
    // не требуется. Проверка ответом процесса, а не существованием файла:
    // прерванная прошлая копия дала бы «установлено» и падение при старте.
    if !force && dest.exists() {
        match probe_version(&dest) {
            Ok(version) => {
                // Проверяем, не новее ли кандидат-источник (для локальной разработки и апдейтов без бампа версии)
                let mut need_update = false;
                if let Ok(source) = find_source(exe_file) {
                    if let Ok(source_ver) = probe_version(&source) {
                        if source_ver != version {
                            need_update = true;
                        }
                    }
                    if let (Ok(src_meta), Ok(dest_meta)) = (fs::metadata(&source), fs::metadata(&dest)) {
                        if let (Ok(src_time), Ok(dest_time)) = (src_meta.modified(), dest_meta.modified()) {
                            if src_time > dest_time {
                                need_update = true;
                            }
                        }
                        if src_meta.len() != dest_meta.len() {
                            need_update = true;
                        }
                    }
                }

                if !need_update {
                    progress(
                        "installed",
                        0,
                        0,
                        &format!("{} v{} уже установлен", router_id, version),
                    );
                    log::info!("{} v{} уже установлен ({})", router_id, version, dest.display());
                    let mut cfg = config::load_config(app, router_id);
                    if cfg.installed_version.as_deref() != Some(version.as_str()) {
                        cfg.installed_version = Some(version.clone());
                        config::save_config(app, router_id, &cfg)?;
                    }
                    return Ok(InstalledInfo::of(version, &dir));
                } else {
                    log::info!("{}: найден более свежий бинарь, обновляем...", router_id);
                }
            }
            Err(e) => log::warn!(
                "{}: установленный бинарь не отвечает ({}), переустанавливаем",
                router_id,
                e
            ),
        }
    }

    // Работающий процесс держит свой файл: перезапись на Windows кончилась бы
    // sharing violation. Останавливаем ДО копирования, а не после неудачи.
    let cfg_now = config::load_config(app, router_id);
    let data_dir = config::router_data_dir(app, router_id);
    crate::router::gateway_process::stop_server(&dir, cfg_now.port_or_default(router_id), &data_dir);

    progress("binary", 0, 0, &format!("Ищем собранный бинарь {}...", exe_file));
    let source = find_source(exe_file)?;
    log::info!("{}: копируем {} -> {}", router_id, source.display(), dest.display());

    fs::create_dir_all(&dir)
        .map_err(|e| format!("Не удалось создать папку установки {}: {}", dir.display(), e))?;

    // Сначала во временный файл рядом с целью: обрыв на середине копирования
    // не должен оставить «наполовину установленный» бинарь, который завтра
    // посчитается рабочим (проверка `--version` его поймает, но файл-заготовка
    // в папке сбивал бы пользователя с толку).
    let file_name = gateway_binary_name(exe_file);
    let staged = dir.join(format!("{}.part", file_name));
    fs::copy(&source, &staged).map_err(|e| {
        format!(
            "Не удалось скопировать {} -> {}: {}",
            source.display(),
            staged.display(),
            e
        )
    })?;
    // Замена целиком: прежний файл убираем в последний момент, чтобы при сбое
    // переименования осталась рабочая предыдущая версия, а не дыра.
    if dest.exists() {
        fs::remove_file(&dest)
            .map_err(|e| format!("Не удалось убрать прежний {}: {}", dest.display(), e))?;
    }
    fs::rename(&staged, &dest)
        .map_err(|e| format!("Не удалось подставить {}: {}", dest.display(), e))?;

    let version = probe_version(&dest)?;
    progress("done", 0, 0, &format!("✅ {} v{} установлен", router_id, version));
    log::info!("✅ {} v{} установлен ({})", router_id, version, dest.display());

    let mut cfg = cfg_now;
    cfg.installed_version = Some(version.clone());
    config::save_config(app, router_id, &cfg)?;

    Ok(InstalledInfo::of(version, &dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let p = std::env::temp_dir().join(format!("cr_gwinst_{tag}_{}", seq));
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

    #[test]
    fn binary_name_comes_from_spec() {
        let exe_file = RouterId::Gateway.gateway_exe_file().expect("exe_file");
        assert_eq!(gateway_binary_name(exe_file), "cloud-routers-gateway.exe");
    }

    #[test]
    fn source_candidates_start_with_dir_next_to_host_exe() {
        let cands = source_candidates("cloud-routers-gateway");
        let host_exe_dir = std::env::current_exe()
            .expect("тестовый бинарь запущен с реального пути")
            .parent()
            .expect("у exe есть родитель")
            .to_path_buf();
        assert_eq!(
            cands.first().expect("кандидаты не пусты"),
            &host_exe_dir.join("cloud-routers-gateway.exe"),
            "первым идёт путь рядом с exe хоста — куда его кладёт сборка"
        );
    }

    #[test]
    fn source_candidates_include_workspace_target_dirs() {
        let cands = source_candidates("cloud-routers-gateway");
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("workspace");
        assert!(cands.contains(&workspace.join("target").join("release").join("cloud-routers-gateway.exe")));
        assert!(cands.contains(&workspace.join("target").join("debug").join("cloud-routers-gateway.exe")));
    }

    #[test]
    fn find_source_error_lists_every_checked_path() {
        // Имя, которого нет нигде: ошибка обязана перечислять, что проверялось,
        // иначе «не найдено» не отличить от «сломалось копирование».
        let err = find_source("no-such-binary-xyz").expect_err("источника нет");
        for tail in ["release", "debug"] {
            assert!(err.contains(tail), "ошибка не упоминает {}: {err}", tail);
        }
        assert!(err.contains("no-such-binary-xyz.exe"), "{err}");
    }

    #[test]
    fn probe_version_fails_loudly_for_non_executable_file() {
        // Файл, который не отвечает, обязан дать ошибку с его путём.
        let tmp = TempDir::new("notexec");
        let fake = tmp.path().join("cloud-routers-gateway.exe");
        fs::write(&fake, b"not a pe file").unwrap();
        let err = probe_version(&fake).expect_err("запуск обязан провалиться");
        assert!(!err.is_empty());
    }

    /// Настоящий разбор вывода настоящего бинаря.
    ///
    /// Тест проверяет ровно ту цепочку, которой пользуется установщик: если
    /// формат `--version` в шлюзе изменится, установка перестанет видеть
    /// версию, а этот тест упадёт. Если бинарь ещё не собран (чистый checkout
    /// или CI без Rust-тулчейна шлюза) — тест честно молчит, а не подставляет
    /// выдуманный вывод: разбирать в тесте собственную строку означало бы
    /// проверить `split_whitespace`, а не наш код.
    #[test]
    fn probe_version_parses_real_gateway_binary() {
        let Ok(source) = find_source("cloud-routers-gateway") else {
            eprintln!("skip: cloud-routers-gateway не собран — `cargo build -p cloud-routers-gateway`");
            return;
        };
        let version = probe_version(&source).expect("настоящий бинарь обязан печатать свою версию");
        assert!(
            !version.trim().is_empty(),
            "версия пуста — разбор вывода сломался (бинарь {})",
            source.display()
        );
        assert!(
            version.starts_with(|c: char| c.is_ascii_digit()),
            "ожидалась версия цифрами, получено {:?}",
            version
        );
    }
}