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
    // консоли (global_ai_docs/desktop_rust_tauri_rules.md §6.8).
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

/// Единая формула «установленному бинарю нужен свежий»: версия источника
/// отличается (включая «установленная версия вообще не записана»), источник
/// новее по времени или файлы различаются размером.
///
/// Одна формула для двух точек, которые решают одно и то же:
/// `commands::check_router_update` (бейдж «Доступно обновление») и
/// `install` (нужна ли перезапись при force=false). Раньше правило было
/// скопировано в обоих местах и могло разъехаться: проверка говорила бы
/// «актуально», а установка при следующем запуске молча перезаписывала
/// бинарь (или наоборот).
///
/// Нечитаемые метаданные (`None` для source или dest) признак НЕ дают:
/// отсутствие данных — не «обновление есть». «Dest отсутствует» — отдельное
/// условие вызывающего (у check это плюс-критерий, у install файл к этому
/// моменту уже существует).
pub fn binary_differs(
    source_version: &str,
    installed_version: Option<&str>,
    source_meta: Option<&fs::Metadata>,
    dest_meta: Option<&fs::Metadata>,
) -> bool {
    if installed_version != Some(source_version) {
        return true;
    }
    let (Some(src), Some(dst)) = (source_meta, dest_meta) else {
        return false;
    };
    let (Ok(src_time), Ok(dst_time)) = (src.modified(), dst.modified()) else {
        return false;
    };
    if src_time > dst_time {
        return true;
    }
    src.len() != dst.len()
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
                // Нужна ли перезапись: ниже-уровня формула с check_router_update
                // (`binary_differs`): версия источника != установленной, источник
                // свежее или размер различается. Для локальной разработки и апдейтов
                // без бампа версии.
                let mut need_update = false;
                if let Ok(source) = find_source(exe_file) {
                    if let Ok(source_ver) = probe_version(&source) {
                        if binary_differs(
                            &source_ver,
                            Some(&version),
                            fs::metadata(&source).ok().as_ref(),
                            fs::metadata(&dest).ok().as_ref(),
                        ) {
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
    // Неудача — честная ошибка установки: дальше был бы sharing violation,
    // который выглядел бы чужой ошибкой копирования.
    let cfg_now = config::load_config(app, router_id);
    let data_dir = config::router_data_dir(app, router_id);
    crate::router::gateway_process::stop_server(&dir, cfg_now.port_or_default(router_id), &data_dir)?;

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
    // Замена целиком через промежуточное имя, а не «удалить → переименовать»:
// между удалением и переименованием есть окно, в котором рабочего бинаря
// нет вообще. Переименование в `.old` → переименование нового на место →
// удаление `.old` такого окна не создаёт, а при сбое возвращает прежний файл.
    let backup = dir.join(format!("{}.old", file_name));
    if dest.exists() {
        let _ = fs::remove_file(&backup);
        fs::rename(&dest, &backup).map_err(|e| {
            format!(
                "Не удалось освободить место под новый бинарь ({} -> {}): {}. \
                 Прежний бинарь не тронут.",
                dest.display(),
                backup.display(),
                e
            )
        })?;
    }
    if let Err(e) = fs::rename(&staged, &dest) {
        let restore = if !dest.exists() && backup.exists() {
            fs::rename(&backup, &dest).err()
        } else {
            None
        };
        return Err(match restore {
            Some(r) => format!(
                "Не удалось подставить новый бинарь ({}): {}. Откат не удался: {}. \
                 Прежний бинарь остался в {}.",
                staged.display(),
                e,
                r,
                backup.display()
            ),
            None => format!(
                "Не удалось подставить новый бинарь ({}): {}. Прежний бинарь \
                 восстановлен и продолжает работать.",
                staged.display(),
                e
            ),
        });
    }
    if backup.exists() {
        if let Err(e) = fs::remove_file(&backup) {
            log::warn!(
                "{}: прежний бинарь {} не удалён: {}",
                router_id,
                backup.display(),
                e
            );
        }
    }

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
            let p = crate::router::test_scratch::scratch_dir(&format!("cr_gwinst_{}_{}", tag, seq));
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

    /// Три признака формулы `binary_differs`: различие версий, источник новее
    /// и различие размеров. Не читаемые метаданные признак НЕ дают (22.10.26).
    #[test]
    fn binary_differs_flags_version_change() {
        let tmp = TempDir::new("diffver");
        let file = tmp.path().join("same.meta");
        fs::write(&file, b"identical content").unwrap();
        let meta = fs::metadata(&file).expect("meta");

        // Установленная версия не записана (нет запись в конфиге) — обновление предлагаем.
        assert!(binary_differs("0.1.2", None, Some(&meta), Some(&meta)));
        // Версии различаются — простейший признак (бамп Cargo-версии гейтвея).
        assert!(binary_differs("0.1.2", Some("0.1.1"), Some(&meta), Some(&meta)));
        // Полное совпадение — актуально, включать «обновление» было бы ложью.
        assert!(!binary_differs("0.1.1", Some("0.1.1"), Some(&meta), Some(&meta)));
    }

    #[test]
    fn binary_differs_flags_newer_source_mtime() {
        let tmp = TempDir::new("diffmtime");
        let dst = tmp.path().join("dest.bin");
        let src = tmp.path().join("src.bin");
        fs::write(&dst, b"AAAABBBB").unwrap();
        // Гарантированный интервал между записями файлов: mtime-разрешение
        // NTFS (~100 нс), но под нагрузкой планировщика sleep всё же надёжней.
        std::thread::sleep(std::time::Duration::from_millis(60));
        fs::write(&src, b"AAAABBBB").unwrap();

        let dst_meta = fs::metadata(&dst).expect("dst meta");
        let src_meta = fs::metadata(&src).expect("src meta");
        let version = "0.1.2";

        // Источник свежее установленного файла — обновление обязано светиться.
        assert!(binary_differs(version, Some(version), Some(&src_meta), Some(&dst_meta)));
        // Обратный порядок (установленный новее источника) — не признак:
        // так бьётся кейс «свежий бинарь скопировали, источник заменяли позже».
        assert!(!binary_differs(version, Some(version), Some(&dst_meta), Some(&src_meta)));
    }

    #[test]
    fn binary_differs_flags_size_mismatch() {
        let tmp = TempDir::new("diffsize");
        let src = tmp.path().join("src.bin");
        let dst = tmp.path().join("dst.bin");
        fs::write(&src, b"x").unwrap();
        fs::write(&dst, b"yy").unwrap();
        let src_meta = fs::metadata(&src).expect("src meta");
        let dst_meta = fs::metadata(&dst).expect("dst meta");
        assert!(binary_differs("0.1.2", Some("0.1.2"), Some(&src_meta), Some(&dst_meta)));
        // Прерванное копирование (артефакт `.part` тупикового размера) ловится
        // тем же признаком — иначе «установлено» и падение при старте.
        assert!(binary_differs("0.1.2", Some("0.1.2"), Some(&dst_meta), Some(&src_meta)));
        // Равные размеры и версии — актуально.
        assert!(!binary_differs("0.1.2", Some("0.1.2"), Some(&src_meta), Some(&src_meta)));
    }

    #[test]
    fn binary_differs_none_metadata_means_no_signal() {
        // Нечитаемые метаданные НЕ признак: отсутствие данных ≠ «обновление есть»
        // (запрет ложных срабатываний — иначе бинарь перезаписывали бы заново
        // при каждом запуске, когда файл занят и мету прочесть нельзя).
        assert!(!binary_differs("0.1.1", Some("0.1.1"), None, None));
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