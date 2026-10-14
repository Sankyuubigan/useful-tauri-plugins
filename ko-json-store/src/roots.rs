//! Выбор корня записи: **где** приложению (хосту и плагинам) разрешено писать.
//!
//! Существует по одной причине: раньше путь передавался в API как аксиома
//! (`app.path().app_data_dir()`, `std::env::temp_dir()`, `%APPDATA%`), и
//! приложение не знало, записывает оно вообще или нет. Отказ записи был
//! неотличим от успеха, пока данные уже не потеряны.
//!
//! Правило выбора — **настоящая проба**, а не `exists()`/`access()`: каталог
//! может существовать и быть закрыт для записи (уровень целостности процесса
//! ниже уровня объекта, квота, антивирус). Проба повторяет последовательность
//! реальной записи — создать, записать, `sync_all`, `rename`, удалить.
//!
//! Каталоги именуются метками, а не путями: путь выбирается один раз на
//! процесс, кэшируется под меткой и выдаётся всем потребителям одинаково.
//! Хост и плагины получают один и тот же ответ — расхождение между ними и было
//! источником «приложение пишет, а плагин молчит».
//!
//! Метки:
//! - [`LABEL_APP_DATA`] — хост: сессии, `app_config.json`, логи промптов
//!   (`%APPDATA%/<identifier>`);
//! - [`LABEL_APP_SHARED`] — плагины (`%APPDATA%/<плагин>`), общий для всех
//!   проектов на машине;
//! - [`LABEL_SCRATCH`] — временные файлы; выбор сразу переопределяет
//!   `TMP`/`TEMP`/`TMPDIR` для процесса, поэтому дочерние процессы
//!   (llama-server, node, npm, git) наследуют рабочий каталог автоматически;
//! - [`LABEL_DATA`] — крупное хранилище `KingOrchData` (движки, модели).
//!
//! Переопределение переменной окружения [`DATA_ROOT_ENV`] имеет приоритет у всех
//! меток кроме [`LABEL_APP_SHARED`]: это единственный способ указать внешний
//! каталог (портативный запуск, отладка на заведомо исправном диске).

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Каталог приложения: сессии, конфиг, логи промптов.
pub const LABEL_APP_DATA: &str = "app-data";
/// Общий каталог плагинов (`%APPDATA%/<плагин>`), одинаковый для всех хостов.
pub const LABEL_APP_SHARED: &str = "app-shared";
/// Временные файлы процесса.
pub const LABEL_SCRATCH: &str = "scratch";
/// Крупное хранилище `KingOrchData`: движки, модели, бандлы.
pub const LABEL_DATA: &str = "king-orch-data";

/// Переопределение корня переменной окружения — высший приоритет в лестнице.
pub const DATA_ROOT_ENV: &str = "KING_ORCH_DATA_ROOT";

/// Один кандидат лестницы: путь и откуда он взят (для лога и диагностики).
#[derive(Debug, Clone)]
pub struct Candidate {
    pub path: PathBuf,
    pub source: String,
}

impl Candidate {
    pub fn new(path: impl Into<PathBuf>, source: impl Into<String>) -> Self {
        let path = path.into();
        if path.as_os_str().is_empty() {
            return Self {
                path: PathBuf::from("."),
                source: source.into(),
            };
        }
        Self {
            path,
            source: source.into(),
        }
    }
}

/// Результат выбора: куда писать, откуда взялось и почему отвергли остальных.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RootChoice {
    pub label: String,
    pub path: PathBuf,
    pub source: String,
    /// `false` — не нашлось ни одного записываемого каталога; `path` тогда
    /// остаётся последним кандидатом и **не является** рабочим.
    pub writable: bool,
    /// Причины отказа по отвергнутым кандидатам (пусто = первый подошёл).
    pub rejected: Vec<String>,
}

/// Решения по меткам: считаются один раз за процесс и переживают
/// инвалидацию (смена пути хранилища из настроек).
static CHOICES: OnceLock<Mutex<std::collections::BTreeMap<String, RootChoice>>> = OnceLock::new();

fn table() -> &'static Mutex<std::collections::BTreeMap<String, RootChoice>> {
    CHOICES.get_or_init(|| Mutex::new(std::collections::BTreeMap::new()))
}

/// Прочитать уже принятое решение по метке. `None` — метка ещё не выбиралась.
pub fn choice(label: &str) -> Option<RootChoice> {
    table()
        .lock()
        .ok()
        .and_then(|guard| guard.get(label).cloned())
}

/// Выбрать корень для метки: первый записываемый кандидат. Повторный вызов
/// возвращает принятое решение — проба не повторяется на каждый вызов.
pub fn choose(label: &str, candidates: Vec<Candidate>) -> RootChoice {
    if let Some(cached) = choice(label) {
        return cached;
    }
    let picked = probe_ladder(label, &candidates);
    if let Ok(mut guard) = table().lock() {
        guard.insert(label.to_string(), picked.clone());
    }
    picked
}

/// Сбросить решение по метке — после смены пути хранилища пользователем.
pub fn invalidate(label: &str) {
    if let Ok(mut guard) = table().lock() {
        guard.remove(label);
    }
}

/// Текущий корень метки. `None` — метка не выбиралась: вызывать без
/// предшествующего [`choose`] нельзя, иначе потребитель молча писал бы
/// относительным путём.
pub fn root(label: &str) -> Option<PathBuf> {
    choice(label).map(|c| c.path)
}

/// Настоящая проба каждого кандидата по очереди.
fn probe_ladder(label: &str, candidates: &[Candidate]) -> RootChoice {
    let mut rejected = Vec::new();
    for candidate in candidates {
        match crate::probe_dir(&candidate.path) {
            Ok(path) => {
                let choice = RootChoice {
                    label: label.to_string(),
                    path,
                    source: candidate.source.clone(),
                    writable: true,
                    rejected: rejected.clone(),
                };
                log_choice(&choice, true);
                if label == LABEL_SCRATCH {
                    export_temp_env(&choice.path);
                }
                return choice;
            }
            Err(reason) => rejected.push(format!("[{}] {}", candidate.source, reason)),
        }
    }

    let fallback = candidates
        .last()
        .map(|c| c.path.clone())
        .unwrap_or_else(|| PathBuf::from("."));
    let choice = RootChoice {
        label: label.to_string(),
        path: fallback,
        source: "запасной (не проверен)".to_string(),
        writable: false,
        rejected,
    };
    log_choice(&choice, false);
    choice
}

/// Итог одной разведки в лог. Отказ — всегда с перечислением причин: иначе
/// «ничего не записывается» неотличимо от «не смотрели».
fn log_choice(choice: &RootChoice, writable: bool) {
    let rejected = if choice.rejected.is_empty() {
        String::new()
    } else {
        format!("; отклонено: {}", choice.rejected.join(" | "))
    };
    if writable {
        log::info!(
            "[ROOT] {} → {} ({}){}",
            choice.label,
            choice.path.display(),
            choice.source,
            rejected
        );
    } else {
        log::error!(
            "[ROOT] {}: НИ ОДИН кандидат не доступен для записи. Взят запасной {} \
             БЕЗ гарантии ({}){}",
            choice.label,
            choice.path.display(),
            choice.source,
            rejected
        );
    }
}

/// Переопределить `TMP`/`TEMP`/`TMPDIR` для всего процесса.
///
/// Вызывается ровно один раз — вместе с первым выбором [`LABEL_SCRATCH`],
/// поэтому повторного подклеивания пути быть не может. На Windows
/// `set_var` sound и в многопоточном процессе (документация std, «Safety»),
/// а дочерние процессы наследуют переменные автоматически.
fn export_temp_env(dir: &Path) {
    for key in ["TMP", "TEMP", "TMPDIR"] {
        // SAFETY: см. doc-комментарий функции — Windows, single/multi-thread OK.
        unsafe { std::env::set_var(key, dir) };
    }
    log::info!("[ROOT] временные файлы процесса → {}", dir.display());
}

/// Кандидат из переменной окружения [`DATA_ROOT_ENV`], если она задана.
/// Пустое значение игнорируется: «переменная объявлена, но не выбрана» — это
/// не выбор.
pub fn env_candidate(source: &str) -> Option<Candidate> {
    let raw = std::env::var_os(DATA_ROOT_ENV)?;
    if raw.is_empty() {
        return None;
    }
    Some(Candidate::new(PathBuf::from(raw), source))
}

/// Папка рядом с исполняемым файлом — запасной кандидат, существующий всегда.
fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Базовый пользовательский каталог (`APPDATA`) — без `AppHandle`, поэтому
/// доступен и плагину, и хосту одинаково.
pub fn user_base() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn shared_ladder() -> Vec<Candidate> {
    let mut candidates = Vec::new();
    candidates.extend(env_candidate("переменная окружения"));
    candidates.push(Candidate::new(user_base(), "APPDATA"));
    candidates.push(Candidate::new(exe_dir().join("data"), "папка приложения"));
    candidates
}

fn app_data_ladder(identifier: &str) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    candidates.extend(env_candidate("переменная окружения"));
    candidates.push(Candidate::new(user_base().join(identifier), "APPDATA"));
    candidates.push(Candidate::new(exe_dir().join("data"), "папка приложения"));
    candidates
}

/// Каталог данных **плагина**: общий корень плюс имя плагина.
///
/// Общий, а не per-app: процесс шлюза один на машину, и второй хост обязан
/// видеть тот же каталог, иначе «свой» путь окажется у каждого. Решение
/// принимается здесь же, если хост ещё не выбрал корень, — лестница одна и та
/// же, поэтому хост и плагин приходят к одному ответу независимо от порядка.
pub fn plugin_dir(name: &str) -> PathBuf {
    choose(LABEL_APP_SHARED, shared_ladder());
    let base = root(LABEL_APP_SHARED).unwrap_or_else(user_base);
    stay_or_adopt(LABEL_APP_SHARED, &base, &user_base().join(name), name)
}

/// Каталог данных приложения-хоста плюс `name` — для плагинов, которые по
/// договорённости пишут в каталог хоста (например, image-engine читает
/// `app_config.json` King Orch).
pub fn app_dir(identifier: &str, name: &str) -> PathBuf {
    let legacy = user_base().join(identifier);
    choose(LABEL_APP_DATA, app_data_ladder(identifier));
    let base = root(LABEL_APP_DATA).unwrap_or_else(|| legacy.clone());
    stay_or_adopt(LABEL_APP_DATA, &base, &legacy, name)
}

/// Корень не изменился — возвращаем прежний путь без переноса. Изменился —
/// переносим данные в новый. Ядро обеих функций выше, вынесено отдельно,
/// чтобы правило проверялось без обращения к настоящему `%APPDATA%`.
fn stay_or_adopt(label: &str, base: &Path, legacy: &Path, subdir: &str) -> PathBuf {
    if base == legacy || (subdir.is_empty() && base == legacy) {
        return legacy.join(subdir);
    }
    adopt(label, legacy, subdir)
}

// ────────────────────────────── разовое перенаселение ──────────────────────────────

/// Перенести данные из прежнего каталога в выбранный корневой — один раз.
///
/// Без этого «починил» означал бы «старые сессии исчезли»: файлы остались бы
/// в старом корне, который приложение больше не читает. Перенос
/// **не перезаписывает** уже существующие файлы и **не удаляет** источник —
/// вторая копия не мешает, зато откат остаётся возможным.
///
/// `subdir` — подкаталог внутри выбранного корня (у плагинов это имя плагина).
pub fn adopt(label: &str, legacy: &Path, subdir: &str) -> PathBuf {
    let Some(base) = root(label) else {
        log::error!(
            "[ROOT] {} не выбран — перенос из {} невозможен, данные останутся в старом месте",
            label,
            legacy.display()
        );
        return legacy.to_path_buf();
    };
    let destination = base.join(subdir);

    if legacy == destination {
        return destination;
    }
    if !legacy.is_dir() {
        return destination;
    }
    if already_adopted(&destination, legacy) {
        // Молча. `plugin_dir()` зовётся на каждый запрос, и без этой проверки
        // лог засорялся десятками одинаковых строк за запуск.
        return destination;
    }

    log::warn!(
        "[ROOT] {}: корень изменён на {} — переношу данные из {}. Копия в старом \
         месте НЕ удаляется.",
        label,
        destination.display(),
        legacy.display()
    );
    match copy_tree(legacy, &destination) {
        Ok(stats) => {
            if stats.skipped > 0 {
                // Тихий пропуск — потенциальная потеря данных: файл с тем же
                // именем лежит и там, и там, и какой из них свежий, неизвестно.
                log::warn!(
                    "[ROOT] {}: в {} уже было {} файлов с теми же именами — они НЕ \
                     перезаписаны. Если это остатки неудачного переноса, сверьтесь \
                     вручную с {}.",
                    label,
                    destination.display(),
                    stats.skipped,
                    legacy.display()
                );
            }
            write_marker(&destination, legacy);
            log::info!(
                "[ROOT] {}: перенесено {} файлов {} → {} (пропущено: {})",
                label,
                stats.copied,
                legacy.display(),
                destination.display(),
                stats.skipped
            );
        }
        Err(e) => log::error!(
            "[ROOT] {}: ПЕРЕНОС НЕ ЗАВЕРШЁН {} → {} ({}). Часть данных осталась в старом месте.",
            label,
            legacy.display(),
            destination.display(),
            e
        ),
    }
    destination
}

/// Имя файла-метки переноса; внутри — путь, из которого в этот каталог
/// переносили данные.
const ADOPT_MARKER: &str = ".ko-adopted-from";

/// Перенос из этого источника в этот каталог уже выполнялся.
///
/// Проверяется **метка, а не «каталог не пуст»**: каталог могли заполнить другие
/// потребители. `app-data` и `app-shared` в запасном режиме указывали на одну
/// папку рядом с exe, и плагины переезжали первыми — проверка «не пусто»
/// объявляла данные хоста уже перенесёнными, хотя там лежали только каталоги
/// плагинов. Именно так потерялись вкладки и история сессий.
fn already_adopted(destination: &Path, legacy: &Path) -> bool {
    std::fs::read_to_string(destination.join(ADOPT_MARKER))
        .map(|marker| marker.trim() == legacy.to_string_lossy())
        .unwrap_or(false)
}

/// Записать метку переноса. Без неё каталог, заполненный кем-то другим,
/// выглядел бы «уже перенесённым» — см. [`already_adopted`].
fn write_marker(destination: &Path, legacy: &Path) {
    if let Err(e) =
        std::fs::write(destination.join(ADOPT_MARKER), legacy.to_string_lossy().as_bytes())
    {
        log::warn!(
            "[ROOT] метка переноса в {} не записана ({}) — перенос повторится при следующем запуске",
            destination.display(),
            e
        );
    }
}

struct CopyStats {
    copied: usize,
    skipped: usize,
}

/// Рекурсивное копирование без перезаписи. Ошибка отдельного файла не роняет
/// перенос целиком: потеря одного файла лучше, чем отказ всей миграции из-за
/// одного занятого — но каждая такая потеря попадает в лог.
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<CopyStats> {
    let mut stats = CopyStats {
        copied: 0,
        skipped: 0,
    };
    std::fs::create_dir_all(to)?;
    let entries = std::fs::read_dir(from)?;
    for entry in entries.flatten() {
        let source = entry.path();
        let destination = to.join(entry.file_name());
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            let nested = copy_tree(&source, &destination)?;
            stats.copied += nested.copied;
            stats.skipped += nested.skipped;
        } else if destination.exists() {
            stats.skipped += 1;
        } else {
            match std::fs::copy(&source, &destination) {
                Ok(_) => stats.copied += 1,
                Err(e) => {
                    stats.skipped += 1;
                    log::warn!(
                        "[ROOT] перенос: {} не скопирован в {} ({})",
                        source.display(),
                        destination.display(),
                        e
                    );
                }
            }
        }
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("ko_roots_{}_{}", tag, nanos));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn choose_picks_first_writable_and_caches() {
        let dir = scratch("ladder");
        let blocked = dir.join("blocked");
        std::fs::write(&blocked, b"x").unwrap();
        let good = dir.join("good");

        let picked = choose(
            "test-ladder",
            vec![
                Candidate::new(blocked, "первый"),
                Candidate::new(good.clone(), "второй"),
            ],
        );
        assert!(picked.writable);
        assert_eq!(picked.path, good);
        assert_eq!(picked.rejected.len(), 1);

        // Повторный вызов обязан вернуть кэш, а не пробовать заново: если бы он
        // пробил, лестница отработала бы второй раз и дала бы тот же ответ
        // случайно, а после смены пути — разный.
        let again = choose("test-ladder", vec![Candidate::new(dir.join("другой"), "третий")]);
        assert_eq!(again.path, good, "решение обязано кэшироваться по метке");
        assert_eq!(again.source, "второй");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalidate_forces_redecision() {
        let dir = scratch("invalidate");
        let first = dir.join("first");
        choose("test-invalidate", vec![Candidate::new(first.clone(), "первый")]);
        assert_eq!(root("test-invalidate"), Some(first.clone()));

        invalidate("test-invalidate");
        assert_eq!(root("test-invalidate"), None);

        let second = dir.join("second");
        let picked = choose("test-invalidate", vec![Candidate::new(second.clone(), "второй")]);
        assert_eq!(picked.path, second);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unwritable_ladder_is_reported_not_hidden() {
        let dir = scratch("allbad");
        let a = dir.join("a");
        let b = dir.join("b");
        std::fs::write(&a, b"x").unwrap();
        std::fs::write(&b, b"x").unwrap();

        let picked = choose("test-allbad", vec![Candidate::new(a, "a"), Candidate::new(b, "b")]);
        assert!(!picked.writable, "ложная «доступность» недопустима");
        assert_eq!(picked.rejected.len(), 2, "причины отказа обязаны сохраняться");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn adopt_copies_once_and_never_overwrites() {
        let dir = scratch("adopt");
        let legacy = dir.join("legacy");
        std::fs::create_dir_all(legacy.join("sessions")).unwrap();
        std::fs::write(legacy.join("sessions").join("s1.json"), "{\"a\":1}").unwrap();
        std::fs::write(legacy.join("app_config.json"), "{\"b\":2}").unwrap();

        let base = dir.join("base");
        choose("test-adopt", vec![Candidate::new(base.clone(), "base")]);
        let target = adopt("test-adopt", &legacy, "");
        assert_eq!(target, base);
        assert!(base.join("sessions").join("s1.json").is_file());
        assert!(base.join("app_config.json").is_file());

        // Новый файл в старом месте не должен перезаписывать уже перенесённое.
        std::fs::write(base.join("app_config.json"), "{\"b\":999}").unwrap();
        std::fs::write(legacy.join("app_config.json"), "{\"b\":777}").unwrap();
        adopt("test-adopt", &legacy, "");
        assert_eq!(
            std::fs::read_to_string(base.join("app_config.json")).unwrap(),
            "{\"b\":999}",
            "существующий файл переноса не перезаписывается"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn adopt_keeps_legacy_when_nothing_to_move() {
        let dir = scratch("adopt_empty");
        let legacy = dir.join("legacy");
        std::fs::create_dir_all(&legacy).unwrap();
        let base = dir.join("base");
        choose("test-adopt-empty", vec![Candidate::new(base.clone(), "base")]);

        let target = adopt("test-adopt-empty", &legacy, "sessions");
        assert_eq!(target, base.join("sessions"));
        assert!(legacy.is_dir(), "источник не удаляется");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn adopt_into_missing_label_returns_legacy_instead_of_relative_path() {
        let dir = scratch("adopt_nolabel");
        let legacy = dir.join("legacy");
        std::fs::create_dir_all(&legacy).unwrap();
        let target = adopt("test-never-chosen", &legacy, "");
        assert_eq!(target, legacy, "без решения по метке писать некуда");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_env_value_is_not_a_candidate() {
        // SAFETY: тест меняет переменную процесса и восстанавливает её.
        unsafe {
            std::env::set_var(DATA_ROOT_ENV, "");
            assert!(env_candidate("override").is_none());
            std::env::remove_var(DATA_ROOT_ENV);
            assert!(env_candidate("override").is_none());
        }
    }

#[test]
    fn stay_or_adopt_does_nothing_when_base_unchanged() {
        let dir = scratch("stay");
        let base = dir.join("appdata");
        fs::create_dir_all(base.join("cloud-routers")).unwrap();
        fs::write(base.join("cloud-routers").join("app_config.json"), "{}").unwrap();

        let result = stay_or_adopt("test-stay", &base, &base, "cloud-routers");
        assert_eq!(result, base.join("cloud-routers"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn stay_or_adopt_moves_data_when_base_changed() {
        let dir = scratch("move");
        let legacy = dir.join("legacy");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("app_config.json"), r#"{"ok":1}"#).unwrap();
        let base = dir.join("exe_data");
        choose("test-move", vec![Candidate::new(base.clone(), "папка приложения")]);

        let result = stay_or_adopt("test-move", &base, &legacy, "cloud-routers");
        assert_eq!(result, base.join("cloud-routers"));
        assert!(base.join("cloud-routers").join("app_config.json").is_file());
        let _ = fs::remove_dir_all(&dir);
    }

/// Регрессия 14.10: вкладки и история сессий потерялись, потому что
    /// «каталог не пуст» считался признаком «перенос уже был».
    ///
    /// `app-data` и `app-shared` в запасном режиме указывают на одну папку рядом
    /// с exe. Плагины переехали первыми и заполнили её своими каталогами, после
    /// чего перенос данных хоста был объявлен ненужным — и не состоялся вовсе.
    #[test]
    fn adopt_runs_even_when_another_consumer_filled_the_destination() {
        let dir = scratch("adopt_shared");
        let legacy = dir.join("legacy");
        fs::create_dir_all(legacy.join("sessions")).unwrap();
        fs::write(legacy.join("sessions").join("s1.json"), r#"{"вкладки":4}"#).unwrap();
        fs::write(legacy.join("app_config.json"), r#"{"tabs":[1,2,3,4]}"#).unwrap();

        let base = dir.join("base");
        // Каталог заполнен ЧУЖИМ потребителем — плагином, а не хостом.
        fs::create_dir_all(base.join("9router")).unwrap();
        fs::write(base.join("9router").join("server.json"), "{}").unwrap();
        choose("test-adopt-shared", vec![Candidate::new(base.clone(), "папка приложения")]);

        adopt("test-adopt-shared", &legacy, "");

        assert!(
            base.join("app_config.json").is_file(),
            "данные хоста обязаны переехать, даже если рядом лежат каталоги плагинов"
        );
        assert!(base.join("sessions").join("s1.json").is_file());
        assert!(base.join("9router").join("server.json").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn adopt_writes_marker_and_second_run_does_not_copy_again() {
        let dir = scratch("adopt_marker");
        let legacy = dir.join("legacy");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("a.json"), r#"{"v":1}"#).unwrap();
        let base = dir.join("base");
        choose("test-adopt-marker", vec![Candidate::new(base.clone(), "base")]);

        adopt("test-adopt-marker", &legacy, "");
        assert!(base.join(ADOPT_MARKER).is_file(), "метка обязана быть записана");
        assert!(already_adopted(&base, &legacy));

        // Файл появился в источнике уже ПОСЛЕ переноса — повторный запуск не
        // должен его подтягивать, иначе «перенос» превратился бы в синхронизацию
        // без правил слияния.
        fs::write(legacy.join("b.json"), r#"{"v":2}"#).unwrap();
        adopt("test-adopt-marker", &legacy, "");
        assert!(
            !base.join("b.json").exists(),
            "после метки перенос считается выполненным и не повторяется"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn adopt_does_not_overwrite_existing_file_but_warns_by_count() {
        let dir = scratch("adopt_keep");
        let legacy = dir.join("legacy");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("a.json"), r#"{"из_старого":1}"#).unwrap();
        let base = dir.join("base");
        fs::create_dir_all(&base).unwrap();
        fs::write(base.join("a.json"), r#"{"уже_тут":2}"#).unwrap();
        choose("test-adopt-keep", vec![Candidate::new(base.clone(), "base")]);

        adopt("test-adopt-keep", &legacy, "");
        assert_eq!(
            fs::read_to_string(base.join("a.json")).unwrap(),
            r#"{"уже_тут":2}"#,
            "существующий файл не перезаписывается — свежесть неизвестна"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}