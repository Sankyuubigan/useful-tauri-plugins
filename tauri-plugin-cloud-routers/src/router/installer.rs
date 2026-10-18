//! Установка / обновление облачных роутеров БЕЗ системных зависимостей.
//!
//! Всё скачивается и распаковывается в папку `<router_dir>` (по умолчанию
//! `APPDATA/cloud-routers/<id>` — каталог плагина, общий для всех проектов),
//! терминал и админ-права не нужны:
//!
//! ```text
//! APPDATA/cloud-routers/<id>/
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
use crate::spec::RouterKind;

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
    // Размер берётся из таблицы роутеров, а не из литерала. Литерал «тройка»
    // перестал бы соответствовать реальности при добавлении четвёртого
    // роутера — и `install_guard` упал бы с «index out of bounds» уже в
    // рантайме, то есть у пользователя в момент установки.
    INSTALL_LOCKS.get_or_init(|| {
        (0..crate::spec::RouterId::ALL.len())
            .map(|_| Mutex::new(()))
            .collect()
    })
}

/// Мьютекс установки роутера. Держать на всё время установки и на всё время
/// запуска сервера (обе операции — внутри `spawn_blocking`, без `.await`).
pub fn install_guard(router_id: RouterId) -> MutexGuard<'static, ()> {
    let lock = &install_locks()[router_id.index()];
    match lock.lock() {
        Ok(g) => g,
        // Отравленный мьютекс = паника в другом потоке; восстановление
        // безопасно: внутри критической секции нет инвариантов, только «жди».
        Err(poisoned) => poisoned.into_inner(),
    }
}

// ───────────────────────── Папка сбора дочерних процессов ─────────────────────
//
// `npm install` и `postinstall` работают через системный `os.tmpdir()`.
// Отдавать им разделяемую системную %TEMP% нельзя: там уже воспроизводился
// отказ удаления у чужого инструмента на этой же связке
// (esbuild.exe: `remove ...\esbuild-<hash>: Access is denied`), и такой отказ
// выглядел бы как «непонятная ошибка npm», хотя причина — в чужой папке.
//
// Инвариант тот же, что в сборке (tauri-build-toolkit/lib/scratch.cjs): своя
// папка, которую мы создаём и ПРОВЕРЯЕМ. Молчаливый откат на %TEMP% запрещён —
// вместо него падение с точным сообщением на старте установки.
//
// Папка НЕ лежит внутри `dist`: при обновлении `dist` подменяется целиком, и
// временные файлы не должны ни переезжать вместе с ним, ни выглядеть как
// часть установленного пакета. Соседняя папка — тот же том, поэтому атомарная
// подмена `dist` не теряет кросс-томной семантики.
pub fn scratch_dir(router_dir: &Path) -> PathBuf {
    router_dir.join(".scratch")
}

/// Создать папку сбора и убедиться, что в неё можно записать и что записанное
/// удаляется. Второе — не паранойя: именно отказ удаления был классом проблемы.
fn ensure_scratch(router_dir: &Path, router_id: RouterId) -> Result<PathBuf, String> {
    let dir = scratch_dir(router_dir);
    fs::create_dir_all(&dir).map_err(|e| {
        format!("{}: не удалось создать папку сбора {}: {}", router_id, dir.display(), e)
    })?;
    // Пробник уникален: два роутера могут ставиться одновременно в разных
    // потоках одного процесса, и общий по PID файл приводил бы к тому, что
    // один удалит пробник другого и получит ложный отказ.
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let probe = dir.join(format!("probe-{}-{}.tmp", std::process::id(), unique));
    fs::write(&probe, b"probe").map_err(|e| {
        format!("{}: папка сбора {} не принимает запись: {}", router_id, dir.display(), e)
    })?;
    fs::remove_file(&probe).map_err(|e| {
        format!(
            "{}: папка сбора {} не отдаёт файлы обратно ({}). \
             Установка остановлена: без этого npm будет падать тем же отказом, \
             что и в системной %TEMP%.",
            router_id,
            dir.display(),
            e
        )
    })?;
    Ok(dir)
}

/// Переменные окружения дочернего процесса. Единый список на npm и postinstall:
/// разъезжающиеся списки означали бы, что один из шагов снова пойдёт в %TEMP%.
fn child_env(scratch: &Path) -> [(&'static str, PathBuf); 3] {
    [
        ("TMP", scratch.to_path_buf()),
        ("TEMP", scratch.to_path_buf()),
        ("TMPDIR", scratch.to_path_buf()),
    ]
}

fn apply_child_env(cmd: &mut std::process::Command, scratch: &Path) {
    for (key, value) in child_env(scratch) {
        cmd.env(key, value);
    }
}

// ───────────────── Файловая блокировка установки (между хостами) ───────────────
//
// Мьютекс выше живёт в ПРОЦЕССЕ хоста, а папка установки общая для всех
// приложений (см. config::choose_router_dir). Второй хост про мьютекс ничего не
// знает и может удалить `dist` под работающим node первого — ровно тот сценарий,
// из-за которого обновление должно быть транзакционным.
//
// Лок — файл, созданный эксклюзивно (`create_new` — атомарно на NTFS), внутри
// папки установки: он виден всем процессам и всем хостам. Новых зависимостей
// не требуется. Зависший лок (процесс убили) снимается по проверке живости PID.

const INSTALL_LOCK_FILE: &str = ".install.lock";

#[derive(Debug)]
struct InstallLock {
    path: PathBuf,
}

impl InstallLock {
    fn path_for(router_dir: &Path) -> PathBuf {
        router_dir.join(INSTALL_LOCK_FILE)
    }

    /// Взять блокировку. Занята живым процессом — ошибка (пользователю честно
    /// показывают «установка уже идёт», а не «папка занята»). Осталась от
    /// упавшего процесса — снимается и берётся заново.
    ///
    /// ## Почему нельзя падать на невозможности создать файл
    ///
    /// Блокировка — страховка от параллельной установки ДВУМЯ хостами, а не
    /// условие самой установки. На этой машине запись во временные файлы и
    /// папки периодически отказала с `Access is denied` (os error 5) —
    /// 12.10 из-за этого упало обновление OmniRoute. Если такой отказ
    /// прерывал бы установку, единственная неисправность в файловой системе
    /// запрещала бы пользователю обновляться вообще, а страховка всё равно
    /// не защитила бы — защищать нечем, файла нет.
    ///
    /// Поэтому: не смогли создать файл → пишем предупреждение и работаем без
    /// межхостовой блокировки. Мьютекс в памяти процесса (install_guard) при
    /// этом остаётся, а рабочую версию в любом случае защищает транзакционная
    /// подмена: худший исход — два хоста соберут версии независимо, и
    /// последний подменит папку.
    fn acquire(router_dir: &Path, router_id: RouterId) -> Result<Option<Self>, String> {
        let path = Self::path_for(router_dir);
        for attempt in 0..2 {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut f) => {
                    use std::io::Write;
                    let _ = writeln!(f, "{}", std::process::id());
                    return Ok(Some(InstallLock { path }));
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if attempt == 1 || !Self::break_if_stale(&path, router_id) {
                        let owner = std::fs::read_to_string(&path).unwrap_or_default();
                        return Err(format!(
                            "{}: установка уже выполняется другим окном ({}). Дождитесь \
                             окончания или закройте то окно — прерывать установку нельзя, \
                             папка роутера останется в промежуточном состоянии.",
                            router_id,
                            owner.trim()
                        ));
                    }
                }
                Err(e) => {
                    log::warn!(
                        "{}: не удалось создать файл блокировки {} ({}). Продолжаем без \
                         межхостовой блокировки: в этой машине запись в файлы периодически \
                         отказывает, и запрещать установку из-за служебного файла нельзя.",
                        router_id,
                        path.display(),
                        e
                    );
                    return Ok(None);
                }
            }
        }
        unreachable!("цикл acquire имеет ровно две итерации и возвращает в обеих")
    }

    /// Снять оставшийся лок, только если его владелец мёртв. Живой владелец —
    /// не трогаем: это чуж активный апдейт.
    fn break_if_stale(path: &Path, router_id: RouterId) -> bool {
        let raw = std::fs::read_to_string(path).unwrap_or_default();
        let pid: u32 = raw.trim().parse().unwrap_or(0);
        if pid != 0 && crate::router::process::pid_alive(pid) {
            return false;
        }
        log::warn!(
            "{}: снимаем оставшийся файл блокировки (владелец pid {})",
            router_id,
            pid
        );
        std::fs::remove_file(path).is_ok()
    }
}

impl Drop for InstallLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
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

/// `package.json` пакета как объект. Единственное место чтения манифеста:
/// и `package_script`, и проверка версии staging берут его отсюда, иначе
/// два места разошлись бы в том, что считать «прочитанным пакетом».
fn read_package_json(manifest: &Path) -> Option<serde_json::Value> {
    let raw = fs::read_to_string(manifest).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Скрипт жизненного цикла из `package.json` пакета (SSOT — сам пакет, не наш хардкод).
/// `dist/package.json` распакован из tgz, поэтому читаем его, а не гадаем о путях.
pub fn package_script(dist: &Path, name: &str) -> Option<String> {
    let json = read_package_json(&dist.join("package.json"))?;
    let script = json.get("scripts")?.get(name)?.as_str()?.trim();
    if script.is_empty() {
        None
    } else {
        Some(script.to_string())
    }
}

/// Что делать с попыткой установки: пропустить или собрать.
///
/// Отдельная функция, а не условие внутри `install_version`, потому что это
/// ровно тот гейт, который раньше врал: `force` (кнопка «Обновить сейчас»)
/// игнорировался, и обновление молча превращалось в полную переустановку с
/// повторным скачиванием Node и сносом `dist`.
pub fn install_action(complete: bool, force: bool) -> InstallAction {
    if complete && !force {
        InstallAction::Skip
    } else {
        InstallAction::Build
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum InstallAction {
    /// Всё уже на месте — работу не делаем.
    Skip,
    /// Собираем новую версию в staging.
    Build,
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
///
/// Роутера без npm-пакета (наш шлюз) эта функция обслуживать не может:
/// обращение по выдуманному имени молча проверяло бы несуществующий адрес и
/// возвращало «обновлений нет» — то есть врало бы в обе стороны.
pub fn latest_npm_version(client: &Client, router_id: RouterId) -> Result<String, String> {
    let pkg = router_id.npm_package().ok_or_else(|| {
        format!("{} не публикуется в npm — проверять обновления негде", router_id)
    })?;
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
/// ## Почему новая версия собирается в staging, а не в `dist`
///
/// Раньше порядок был «удалить `dist` → распаковать → `npm install`». Любой
/// сбой на этих шагах оставлял роутера без `dist`: он не запускался, а
/// `installed_version` оставался от предыдущей (уже удалённой) версии. Ровно
/// так и выглядит «обновил — перестало работать».
///
/// Теперь новая версия собирается в отдельной папке и подменяет `dist` только
/// после проверки. Сбой на любом шаге = удалённый staging, нетронутый рабочий
/// `dist`, прежняя версия продолжает работать.
pub fn install_or_update(
    app: &AppHandle,
    router_id: RouterId,
    force: bool,
    progress: ProgressFn,
) -> Result<InstalledInfo, String> {
    // Шлюз не в npm: у него нет ни версии на сервере, ни tgz для распаковки.
    // Общая точка входа уходит в отдельный модуль по ВИДУ роутера, а не по
    // флагу внутри npm-логики — иначе каждое её место таскало бы `Option`
    // (ровно то, чего `RouterKind` и создан, чтобы не допустить).
    if let RouterKind::NativeGateway { .. } = router_id.kind() {
        return crate::router::gateway_installer::install(app, router_id, force, progress);
    }

    let dir = crate::router::config::router_dir(app, router_id);
    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP client: {}", e))?;

    // Целевая версия резолвится ДО попытки установки: без неё нечего записать
    // в `failed_update`, а пользователю именно «не удалось обновиться до X»
    // и есть то, что он должен увидеть.
    let npm_ver = latest_npm_version(&client, router_id)?;

    let outcome = install_version(app, router_id, &client, &dir, &npm_ver, force, progress);
    record_update_outcome(app, router_id, &npm_ver, &outcome);
    outcome
}

/// Исход попытки обновления сохраняется в конфиг.
///
/// Без этого UI после неудачного обновления показывал ровно то же состояние,
/// что до него: «установлено, не запущено» + рабочая версия. Пользователь не
/// мог отличить «обновление не получилось» от «обновлений не было» — а именно
/// это и приводило к «обновил и сломал». Теперь причина видна в панели, и она
/// не стирается перезапуском приложения.
fn record_update_outcome(
    app: &AppHandle,
    router_id: RouterId,
    target_version: &str,
    outcome: &Result<InstalledInfo, String>,
) {
    let mut cfg = crate::router::config::load_config(app, router_id);
    let before = cfg.failed_update.clone();
    match outcome {
        Ok(_) => cfg.failed_update = None,
        Err(reason) => {
            cfg.failed_update = Some(crate::router::config::FailedUpdate {
                target_version: target_version.to_string(),
                at_unix: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
                reason: shorten(reason, 600),
            });
        }
    }
    // Сравниваем содержимое, а не «было/стало»: повторный сбой с ДРУГОЙ
    // причиной обязан обновить запись, иначе панель показывала бы причину
    // прошлой неудачи — то есть враньё.
    if cfg.failed_update != before {
        if let Err(e) = crate::router::config::save_config(app, router_id, &cfg) {
            log::warn!("{}: не удалось сохранить исход обновления: {}", router_id, e);
        }
    }
}

/// Обрезать сообщение до `max` символов с многоточием — реестр ошибок должен
/// оставаться читаемым в конфиге и в панели.
fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let skip = text.chars().count() - max;
    format!("…{}", text.chars().skip(skip).collect::<String>().trim_start())
}

/// Установить/обновить node-роутер до конкретной версии.
fn install_version(
    app: &AppHandle,
    router_id: RouterId,
    client: &Client,
    dir: &Path,
    npm_ver: &str,
    force: bool,
    progress: ProgressFn,
) -> Result<InstalledInfo, String> {
    let mut cfg = crate::router::config::load_config(app, router_id);
    let npm_ver = npm_ver.to_string();

    let complete = install_is_complete(
        cfg.installed_version.as_deref(),
        &npm_ver,
        node_exe(dir).exists(),
        server_script_exists(dir, router_id),
        deps_present(dir, router_id),
    );
    // `force` — это «переустановить заново» (кнопка «Установить»). Раньше
    // условие игнорировало force, и кнопка «Обновить сейчас» (она зовёт
    // installOrUpdate с force=true) молча делала полную переустановку вместо
    // обновления: заново качала Node, сносила dist и ставила npm заново.
    if install_action(complete, force) == InstallAction::Skip {
        progress("installed", 0, 0, &format!("{} v{} уже установлен", router_id, npm_ver));
        log::info!("{} v{} уже установлен", router_id, npm_ver);
        return Ok(InstalledInfo {
            version: npm_ver,
            node_version: cfg.node_version.clone().unwrap_or_default(),
            path: dir.to_string_lossy().to_string(),
        });
    }
    if cfg.installed_version.is_some() {
        log::warn!(
            "{}: предыдущая установка неполная (node={}, server={}, deps={}) — переустанавливаем",
            router_id,
            node_exe(dir).exists(),
            server_script_exists(dir, router_id),
            deps_present(dir, router_id),
        );
    }

    fs::create_dir_all(dir.join("runtime"))
        .map_err(|e| format!("Не удалось создать папку установки: {}", e))?;

    // Блокировка берётся ДО сетевых запросов и распаковки: папка установки
    // общая для всех приложений, и два хоста не должны собирать версию
    // одновременно (см. InstallLock).
    let _lock = InstallLock::acquire(dir, router_id)?;
    let scratch = ensure_scratch(dir, router_id)?;

    let staging = staging_dir_for(dir, &npm_ver);
    // Годится ли уже распакованное дерево? Решаем ДО любой загрузки:
    // пригодное дерево не требует ни tgz, ни `npm install`, а негодное
    // должно быть снесено, иначе «продолжим» поверх мусора.
    //
    // `force` — это кнопка «Установить заново»: она обязана пересобирать
    // всё, иначе «переустановить» молча превратится в «достроить».
    let reuse = if force {
        log::info!("{}: переустановка по запросу — дерево не переиспользуем", router_id);
        false
    } else {
        match staging_reusable(&staging, &npm_ver, router_id) {
            StagingState::Reusable => true,
            StagingState::Rebuild(why) => {
                log::info!("{}: дерево пересобирается заново — {}", router_id, why);
                false
            }
        }
    };

    // Мусор прошлых попыток — только наш: активной чужой попытки не
    // возможно, лок держит удаление. Папку текущей версии сохраняем, если
    // с неё продолжаем.
    cleanup_staging(dir, router_id, if reuse { Some(&staging) } else { None });

    // RAII: staging переживает отказ (см. `StagingGuard`) и исчезает только
    // при явном commit() после подмены. Через `?` полураспакованная папка
    // не утекает мусором и не теряет оплаченную работу.
    let mut staging_guard = StagingGuard::new(staging.clone(), router_id);

    let node_ver = resolve_node_version(client)?;
    let node = node_exe(dir);
    let runtime_dir = dir.join("runtime");
    if !node.exists() || !npm_runtime_intact(&runtime_dir) || force {
        log::info!("{}: скачиваем портативный Node.js {} (npm цел: {})", router_id, node_ver, npm_runtime_intact(&runtime_dir));
        progress("node", 0, 0, "Скачиваем портативный Node.js...");
        let zip_bytes = download_bytes(client, &node_zip_url(&node_ver))?;
        // Node перезаписывается только когда сервер его не держит: старый
        // процесс мог пережить kill и продолжить держать файл.
        let cfg_now = crate::router::config::load_config(app, router_id);
        crate::router::process::stop_server(
            router_id,
            dir,
            cfg_now.port_or_default(router_id),
            &crate::router::config::router_data_dir(app, router_id),
        )?;
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

    // Точка решения «качать или переиспользовать» — единственная: и tgz, и
    // распаковка происходят только когда продолжать не с чего.
    let cached_tgz = tgz_cache_path(dir, &npm_ver, router_id);
    if reuse {
        log::info!(
            "{}: продолжаем с готового дерева {}, загрузка tgz и npm install не нужны",
            router_id,
            staging.display()
        );
        // `total = 0` — «процент неизвестен», а не «всё сделано». См. пояснение в
        // `fetch_tgz`: значение `done = total` здесь означало бы для
        // фронтенда «установка завершена» посреди установки.
        progress(
            "router",
            0,
            0,
            &format!("{} v{} уже распакован — продолжаем установку", router_id, npm_ver),
        );
    } else {
        progress("router", 0, 0, &format!("Скачиваем {} v{}...", router_id, npm_ver));
        let tgz = fetch_tgz(client, dir, &npm_ver, router_id, &progress)?;
        progress(
            "unpack",
            0,
            0,
            &format!("Распаковываем {} v{}...", router_id, npm_ver),
        );
        extract_tgz_strip_package(&tgz, &staging)?;
    }

    ensure_server_script(&staging, router_id)?;

    if router_id.needs_npm_deps() {
        if !reuse {
            progress("deps", 0, 0, "Устанавливаем зависимости OmniRoute (npm install)...");
        }
        let runtime_npm_cli = runtime_dir.join("node_modules").join("npm").join("bin").join("npm-cli.js");
        install_omniroute_deps(
            &staging,
            &node,
            &runtime_npm_cli,
            &scratch,
            router_id,
            reuse,
            &progress,
        )?;
        if !deps_marker(&staging).exists() {
            return Err(format!(
                "{}: зависимости не отмечены как установленные ({}). Установка неполная.",
                router_id,
                deps_marker(&staging).display()
            ));
        }
    }

    // Всё собрано и проверено: останавливаем сервер и подменяем папку версии.
    progress(
        "swap",
        0,
        0,
        &format!("Переключаем {} на собранную версию...", router_id),
    );
    swap_in(app, router_id, dir, &staging)?;
    staging_guard.commit();
    // Кэш держим только для текущей версии: повторная попытка идёт за ней же,
    // а версии старые занимали бы по 131 МБ каждая.
    prune_tgz_cache(dir, &cached_tgz, router_id);

    cfg.installed_version = Some(npm_ver.clone());
    cfg.node_version = Some(node_ver.clone());
    crate::router::config::save_config(app, router_id, &cfg)?;

    progress("done", 0, 0, &format!("✅ {} v{} установлен (Node {})", router_id, npm_ver, node_ver));
    log::info!("✅ {} v{} установлен (Node {})", router_id, npm_ver, node_ver);

    Ok(InstalledInfo {
        version: npm_ver,
        node_version: node_ver,
        path: dir.to_string_lossy().to_string(),
    })
}

// ─────────────────────────── Транзакционная подмена ──────────────────────────
//
// Три состояния, из которых складывается установка:
//   `dist`          — рабочая версия, на неё смотрит пользователь;
//   `dist_staging_*` — собираемая версия, её никто не видит;
//   `dist_old_*`    — предыдущая копия `dist` на время подмены.
//
// Подмена — переименования в пределах ОДНОГО тома. Пока `dist` цел, отказ
// невозможен: сбой до `staging → dist` оставляет систему в рабочем состоянии.

const STAGING_PREFIX: &str = "dist_staging_";
const OLD_PREFIX: &str = "dist_old_";
const CACHE_DIR_NAME: &str = ".cache";

/// Имя staging выводится ИЗ ЦЕЛЕВОЙ ВЕРСИИ, а не из времени и PID.
///
/// ## Зачем
///
/// Отказ на `postinstall` стоил 90 секунд `npm install` и 131 МБ загрузки
/// tgz: staging сносился, и следующая попытка начиналась с нуля. Сохранять
/// распакованное дерево можно только так, чтобы следующая попытка **нашла**
/// его — значит имя обязано быть предсказуемым.
///
/// ## Почему не отдельный файл-маркер `.source-version` в папке
///
/// Имя папки и её содержимое были бы двумя независимыми источниками версии.
/// Они расходятся при переименовании, копировании или восстановлении из
/// бэкапа — и тогда тихо установилась бы чужая версия. Имя, выведенное из
/// версии, разойтись не может по построению. Проверка «та ли версия внутри»
/// тоже не по имени, а по `package.json` самого пакета (см.
/// [`staging_reusable`]) — то есть по источнику, который написал автор
/// пакета, а не мы.
fn staging_dir_for(router_dir: &Path, version: &str) -> PathBuf {
    router_dir.join(format!("{}{}", STAGING_PREFIX, sanitize_for_filename(version)))
}

/// Версия (или имя пакета) в имени файла: только безопасные символы.
///
/// Имя папки строится из сетевого ответа npm, поэтому фильтр обязателен:
/// без него значение вида `../../..` увело бы запись мимо папки роутера.
fn sanitize_for_filename(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('.').to_string();
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed
    }
}

/// Чем закончилась проверка готовности staging.
#[derive(Debug, PartialEq, Eq)]
pub enum StagingState {
    /// Дерево этой версии собрано и пригодно — продолжаем с него.
    Reusable,
    /// Годится только пересборка с нуля; строка — причина для лога.
    Rebuild(String),
}

/// Можно ли продолжить установку с уже распакованного staging.
///
/// Проверка **по содержимому**, а не по имени папки: версия читается из
/// `package.json` пакета, готовность зависимостей — из маркера npm
/// (`deps_marker`), серверный скрипт ищется тем же кодом, что и при первой
/// сборке. Любое несовпадение означает пересборку, а не «наверное, подойдёт».
pub fn staging_reusable(staging: &Path, version: &str, router_id: RouterId) -> StagingState {
    let manifest = staging.join("package.json");
    if !manifest.is_file() {
        return StagingState::Rebuild("распакованного пакета нет".to_string());
    }
    match staged_package_version(staging) {
        Some(v) if v == version => {}
        Some(v) => {
            return StagingState::Rebuild(format!(
                "в папке версия {}, а устанавливается {}",
                v, version
            ))
        }
        None => return StagingState::Rebuild("в пакете нет читаемой версии".to_string()),
    }
    if router_id.needs_npm_deps() && !deps_marker(staging).exists() {
        return StagingState::Rebuild("дерево зависимостей не доустановлено".to_string());
    }
    if find_server_script(staging, router_id).is_none() {
        return StagingState::Rebuild("серверный скрипт не найден".to_string());
    }
    StagingState::Reusable
}

/// `version` из `package.json` пакета — SSOT версии для проверки staging.
fn staged_package_version(staging: &Path) -> Option<String> {
    read_package_json(&staging.join("package.json"))
        .and_then(|j| j.get("version")?.as_str().map(|s| s.trim().to_string()))
}

fn new_old_dir(router_dir: &Path) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    router_dir.join(format!("{}{}", OLD_PREFIX, stamp))
}

/// Снести огрызки staging и прошлые подмены. Вызывается под файловой
/// блокировкой: активной чужой попытки быть не может.
///
/// `keep` — папка текущей версии, которую продолжаем. Её снос означал бы
/// «начать заново», то есть ровно то уничтожение оплаченной работы,
/// ради которого шаг `npm install` и повторная загрузка tgz и существуют.
fn cleanup_staging(router_dir: &Path, router_id: RouterId, keep: Option<&Path>) {
    let Ok(entries) = fs::read_dir(router_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(STAGING_PREFIX) || name.starts_with(OLD_PREFIX) {
            let path = entry.path();
            if keep.map(|k| k == &path).unwrap_or(false) {
                log::info!(
                    "{}: промежуточная папка {} сохранена — с неё продолжаем",
                    router_id,
                    name
                );
                continue;
            }
            let res = if path.is_dir() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
            match res {
                Ok(()) => log::info!("{}: убран мусор прошлой установки {}", router_id, name),
                Err(e) => log::warn!("{}: не удалось убрать {}: {}", router_id, name, e),
            }
        }
    }
}

/// Сохраняет staging при отказе — решение, обратное прежнему.
///
/// ## Почему снос был не «транзакционностью», а потерей работы
///
/// Рабочая версия лежит в `dist`, а не в staging, и её снос отказ не
/// затрагивал ни при каких условиях. Единственное, что давал снос staging —
/// «чистое» состояние папки роутера ценой 90 секунд `npm install` и 131 МБ
/// загрузки на КАЖДУЮ попытку. Это не защита, это плата за отсутствие
/// защиты.
///
/// ## Что остаётся честным
///
/// Отказ по-прежнему виден пользователю как отказ, `dist` по-прежнему цел,
/// и следующая попытка проверяет пригодность staging по содержимому
/// ([`staging_reusable`]) — негодное дерево будет пересобрано, а не
/// достроено поверх.
struct StagingGuard {
    path: PathBuf,
    router_id: RouterId,
    committed: bool,
}

impl StagingGuard {
    fn new(path: PathBuf, router_id: RouterId) -> Self {
        StagingGuard {
            path,
            router_id,
            committed: false,
        }
    }

    fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for StagingGuard {
    fn drop(&mut self) {
        if self.committed || !self.path.exists() {
            return;
        }
        log::info!(
            "{}: промежуточная папка {} сохранена для повторной попытки, \
             действующая версия не тронута",
            self.router_id,
            self.path.display()
        );
    }
}

// ───────────────────────────── Кэш распаковки tgz ─────────────────────────────
//
// Повторная попытка обновления не должна заново качать 131 МБ. Кэш — по
// версии, в `<router_dir>/.cache`, рядом со staging и в том же томе.
//
// Кэш тут **ускорение, а не источник истины**: содержимое проходит те же
// проверки, что и только что скачанный файл (распаковка, `staging_reusable`,
// контракт артефактов), поэтому подмена или битый файл в кэше не приводят к
// тихой установке мусора.

fn cache_dir(router_dir: &Path) -> PathBuf {
    router_dir.join(CACHE_DIR_NAME)
}

fn tgz_cache_path(router_dir: &Path, version: &str, router_id: RouterId) -> PathBuf {
    let pkg = router_id.npm_package().unwrap_or("package");
    cache_dir(router_dir).join(format!(
        "{}-{}.tgz",
        sanitize_for_filename(pkg),
        sanitize_for_filename(version)
    ))
}

/// Прочитать tgz из кэша. Пустой или нечитаемый файл кэшем не считается —
/// иначе битая запись превращалась бы в тихую поломку установки.
fn read_cached_tgz(path: &Path) -> Option<Vec<u8>> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() == 0 {
        return None;
    }
    match fs::read(path) {
        Ok(b) if !b.is_empty() => Some(b),
        _ => None,
    }
}

/// Положить tgz в кэш. Запись идёт во временный файл и переименовывается:
/// прерванная запись иначе оставила бы файл вроде валидного, но обрезанный.
///
/// Ошибка записи **не** роняет установку — кэш не условие её успеха.
fn write_cached_tgz(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or("у кэша нет папки")?;
    fs::create_dir_all(dir).map_err(|e| format!("Не создать {}: {}", dir.display(), e))?;
    let tmp = dir.join(format!(
        "{}.{}.part",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("tgz"),
        std::process::id()
    ));
    fs::write(&tmp, bytes).map_err(|e| format!("Запись {}: {}", tmp.display(), e))?;
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            // Хвост не должен остаться мусором — но уже после неудачи.
            let _ = fs::remove_file(&tmp);
            Err(format!("Перенос {} -> {}: {}", tmp.display(), path.display(), e))
        }
    }
}

/// Оставить в кэше только tgz текущей версии.
///
/// Без ограничения кэш растёт на 131 МБ за версию, а пользы от старых версий
/// нет: повторная попытка всегда идёт за одной и той же (текущей).
fn prune_tgz_cache(router_dir: &Path, keep: &Path, router_id: RouterId) {
    let dir = cache_dir(router_dir);
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == keep {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("tgz") {
            continue;
        }
        match fs::remove_file(&path) {
            Ok(()) => log::info!(
                "{}: из кэша убран {} (осталась только текущая версия)",
                router_id,
                path.file_name().and_then(|n| n.to_str()).unwrap_or("?")
            ),
            Err(e) => log::warn!("{}: не удалось убрать {}: {}", router_id, path.display(), e),
        }
    }
}

/// Подменить `dist` на собранный staging: остановить сервер и выполнить
/// переименования. Сама файловая часть — в [`swap_dirs`], чтобы её можно было
/// проверить тестом без `AppHandle`.
///
/// Остановка — через единый `process::stop_server` (тот же контракт, что у
/// кнопки «Остановить»: kill + sweep + верификация). Отдельного слабого пути
/// здесь больше нет: раньше `stop_and_wait` дублировал остановку со своим
/// таймаутом, и два пути расходились.
fn swap_in(
    app: &AppHandle,
    router_id: RouterId,
    dir: &Path,
    staging: &Path,
) -> Result<(), String> {
    let cfg = crate::router::config::load_config(app, router_id);
    crate::router::process::stop_server(
        router_id,
        dir,
        cfg.port_or_default(router_id),
        &crate::router::config::router_data_dir(app, router_id),
    )?;
    swap_dirs(dir, staging)
}

/// Файловая часть подмены: `dist → dist_old_<ts>`, `staging → dist`.
///
/// Отказ на любом шаге оставляет рабочую версию на месте: пока `dist` цел,
/// сбой невозможен. Если `staging → dist` не удалась, `dist_old` возвращается
/// обратно, и оба пути попадают в текст ошибки — иначе пользователь ищет
/// папку, которой нет.
fn swap_dirs(router_dir: &Path, staging: &Path) -> Result<(), String> {
    let dist = dist_dir(router_dir);
    let old = new_old_dir(router_dir);
    if dist.exists() {
        fs::rename(&dist, &old).map_err(|e| {
            format!(
                "Не удалось освободить место под новую версию ({} -> {}): {}. \
                 Прежняя версия не тронута, установка прервана.",
                dist.display(),
                old.display(),
                e
            )
        })?;
    }
    if let Err(e) = fs::rename(staging, &dist) {
        let restore = if !dist.exists() && old.exists() {
            fs::rename(&old, &dist).err()
        } else {
            None
        };
        return Err(match restore {
            Some(r) => format!(
                "Не удалось подменить папку версии ({}): {}. Откат тоже не удался: {}. \
                 Прежняя версия осталась в {}.",
                staging.display(),
                e,
                r,
                old.display()
            ),
            None => format!(
                "Не удалось подменить папку версии ({}): {}. Прежняя версия \
                 восстановлена на место и продолжает работать.",
                staging.display(),
                e
            ),
        });
    }
    if old.exists() {
        if let Err(e) = fs::remove_dir_all(&old) {
            // Не критично: новая версия уже на месте, мусор уберёт следующий
            // запуск (cleanup_staging).
            log::warn!(
                "предыдущая версия {} не удалена: {}",
                old.display(),
                e
            );
        }
    }
    Ok(())
}

/// Результат установки.
#[derive(serde::Serialize, Clone, Default)]
pub struct InstalledInfo {
    pub version: String,
    pub node_version: String,
    pub path: String,
}

impl InstalledInfo {
    /// Папка роутера указывается явно, а не вычисляется внутри.
    ///
    /// Раньше здесь стоял литерал `RouterId::NineRouter`, и `path` для ЛЮБОГО
    /// роутера, включая шлюз, отдавал папку 9router. Это ложь в UI (core rules
    /// §2.2): пользователь нажимал «Изменить путь» и получал чужую папку.
    pub fn of(version: String, dir: &std::path::Path) -> Self {
        Self {
            version,
            // У шлюза нет Node: пустое значение честнее выдуманной версии.
            node_version: String::new(),
            path: dir.to_string_lossy().to_string(),
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
    // У шлюза серверного скрипта нет by design — он не распаковывается из tgz.
    let rel = router_id.server_script_relative()?;
    let expected = dist.join(rel);
    if expected.exists() {
        return Some(expected);
    }
    let filename = Path::new(rel).file_name()?;
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

/// tgz из кэша или из сети — единственная точка загрузки распаковываемого
/// пакета, чтобы «сначала кэш, потом сеть» нельзя было обойти мимо.
///
/// Содержимое кэша проходит те же проверки, что и свежескачанный файл
/// (распаковка + `staging_reusable` + контракт артефактов), поэтому кэш —
/// ускорение, а не источник истины.
fn fetch_tgz(
    client: &Client,
    router_dir: &Path,
    version: &str,
    router_id: RouterId,
    progress: &ProgressFn,
) -> Result<Vec<u8>, String> {
    let cached = tgz_cache_path(router_dir, version, router_id);
    if let Some(bytes) = read_cached_tgz(&cached) {
        log::info!(
            "{}: tgz v{} взят из кэша ({} Б) — загрузка не требуется",
            router_id,
            version,
            bytes.len()
        );
        // `total = 0` — «процент неизвестен», и НЕ «скачано всё». Раньше здесь
        // стояло `done = total = len(байт)`, а фронтенд по `done >= total`
        // объявлял установку завершённой и обновлял панель — в середине
        // установки, до `npm install`. Пользователь видел «готово» при
        // продолжавшейся работе.
        progress(
            "router",
            0,
            0,
            &format!("{} v{} уже скачан — распаковываем", router_id, version),
        );
        return Ok(bytes);
    }
    let bytes = download_bytes(client, &npm_tgz_url(version, router_id))?;
    if let Err(e) = write_cached_tgz(&cached, &bytes) {
        // Отсутствие кэша не является отказом установки: следующая попытка
        // просто скачает файл заново. Молчать об этом нельзя (правило §2.2).
        log::warn!(
            "{}: tgz v{} не сохранён в кэш ({}): {}",
            router_id,
            version,
            cached.display(),
            e
        );
    } else {
        log::info!("{}: tgz v{} сохранён в кэш {}", router_id, version, cached.display());
    }
    Ok(bytes)
}

/// Серверный скрипт обязан лежать в распакованном дереве — иначе пакет
/// распакован не полностью, и поднимать сервер будет нечем.
fn ensure_server_script(staging: &Path, router_id: RouterId) -> Result<(), String> {
    match find_server_script(staging, router_id) {
        Some(s) => {
            log::info!("{}: серверный скрипт найден: {}", router_id, s.display());
            Ok(())
        }
        None => {
            let expected = server_script(staging, router_id);
            Err(format!("В tgz не найден сервер бандла: {}", expected.display()))
        }
    }
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
///
/// ## Почему `reuse_deps` не выключает `postinstall`
///
/// Шаги пакета идемпотентны (`ensureSwcHelpers` выходит раньше, если цель
/// уже есть; копирование standalone-пакетов идёт с `force`), поэтому повтор
/// безопасен и стоит секунды. А вот `npm install` на 648 пакетов стоит
/// десятки секунд — и именно его повторять нечего, дерево уже готово.
fn install_omniroute_deps(
    dist: &Path,
    node: &Path,
    npm_cli: &Path,
    scratch: &Path,
    router_id: RouterId,
    reuse_deps: bool,
    progress: &ProgressFn,
) -> Result<(), String> {
    if reuse_deps {
        log::info!(
            "{}: дерево зависимостей уже размечено как установленное — npm install пропущен",
            router_id
        );
    } else {
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
        apply_child_env(&mut cmd, scratch);
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
                "{}: npm install не завершился (код {:?}){}",
                router_id,
                output.status.code(),
                failure_tail(&output),
            ));
        }
        log::info!(
            "{}: дерево зависимостей установлено за {:.1}c ({} пакетов)",
            router_id,
            elapsed,
            fs::read_dir(dist.join("node_modules")).map(|it| it.count()).unwrap_or(0),
        );
    }

    run_package_postinstall(dist, node, scratch, router_id, progress)
}

/// Выполнить `postinstall` пакета и вынести вердикт по **контракту артефактов**,
/// а не по коду возврата.
///
/// ## Почему код возврата — не вердикт
///
/// Код `0xC0000005` от внешней программы не говорит, какая часть работы
/// осталась недоделанной. Раньше он был единственным критерием, поэтому падение
/// на необязательном шаге отменяло семь уже выполненных и 90 секунд работы.
/// Теперь вердикт выносит проверка того, что postinstall обязана была
/// оставить (см. [`crate::router::postinstall_contract`]).
///
/// ## Что при этом НЕ делается
///
/// Не делается «упал, но файлы появились → успех». Контракт сверяет каждый
/// артефакт с источником по `package.json` и по полноте дерева, поэтому
/// прерванное на середине копирование даёт провал контракта, а не успех.
/// Причину же самого падения мы не «лечим» и не прячем: полный вывод с argv,
/// cwd и окружением остаётся в логе из `run_captured`.
fn run_package_postinstall(
    dist: &Path,
    node: &Path,
    scratch: &Path,
    router_id: RouterId,
    progress: &ProgressFn,
) -> Result<(), String> {
    let Some(script) = package_script(dist, "postinstall") else {
        log::info!("{}: у пакета нет postinstall — пропускаем", router_id);
        return Ok(());
    };
    log::info!("{}: выполняем postinstall пакета: {}", router_id, script);
    // Отдельный этап: postinstall раскладывает нативные модули роутера.
    // Без этого объявления UI молчал с момента «Устанавливаем зависимости»
    // до самого конца установки.
    progress(
        "postinstall",
        0,
        0,
        &format!("{}: раскладываем нативные модули (postinstall)...", router_id),
    );
    let mut cmd = package_script_command(node, dist, &script, scratch);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let started = Instant::now();
    let output = run_captured(&mut cmd, "postinstall")?;
    let elapsed = started.elapsed().as_secs_f64();
    let exit_ok = output.status.success();

    progress(
        "verify",
        0,
        0,
        &format!("{}: проверяем готовность роутера...", router_id),
    );
    let report = crate::router::postinstall_contract::verify(dist);
    match (exit_ok, report.holds()) {
        (true, true) => log::info!(
            "{}: postinstall выполнен за {:.1}c — контракт выполнен ({})",
            router_id,
            elapsed,
            report.describe()
        ),
        (false, true) => log::warn!(
            "{}: postinstall завершился с кодом {:?}, но контракт выполнен ({}). \
             Отказ пришёлся на шаг, не оставляющий артефактов роутера, — установка продолжается. \
             Причина падения остаётся в логе выше.",
            router_id,
            output.status.code(),
            report.describe()
        ),
        (_, false) => {
            let zero_code_lie = if exit_ok {
                " Код возврата при этом 0: шаг отработал, но обязательную работу не сделал — \
                 вердикт вынесен по артефактам, а не по коду."
            } else {
                ""
            };
            return Err(format!(
                "{}: postinstall не оставил обязательные артефакты ({}).{}{}",
                router_id,
                report.describe(),
                zero_code_lie,
                failure_tail(&output),
            ));
        }
    }
    Ok(())
}

/// Причина сбоя доходит до пользователя, а не только до лога.
///
/// Раньше полный вывод npm/postinstall уходил только в лог, а в UI уезжало
/// «не завершился (код 1) — подробности в логе приложения». Пользователь без
/// открытого лога не мог понять, что делать, и обычно решал, что «обновление
/// сломало роутер». Берём последние строки вывода — строка ошибки всегда в
/// конце. Полный вывод по-прежнему целиком в логе (`run_captured`).
fn failure_tail(output: &std::process::Output) -> String {
    const MAX_LINES: usize = 12;
    const MAX_CHARS: usize = 900;
    fn clean(text: &str) -> Vec<&str> {
        text.lines()
            .map(|l| l.trim_end())
            .filter(|l| !l.trim().is_empty())
            .collect()
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    // stderr важнее stdout: npm кладёт в stderr строку ошибки, а в stdout —
    // прогресс реестра, который при неудаче идёт последним и вытеснял причину
    // из хвоста (поймано тестом: E404 терялся среди строк прогресса).
    let stderr_lines = clean(&stderr);
    let source = if stderr_lines.is_empty() {
        clean(&stdout)
    } else {
        stderr_lines
    };
    if source.is_empty() {
        return "\nИнструмент не вывел ни одной строки — причина в логе приложения.".to_string();
    }

    let tail = if source.len() > MAX_LINES {
        source[source.len() - MAX_LINES..].join("\n")
    } else {
        source.join("\n")
    };
    let tail = if tail.chars().count() > MAX_CHARS {
        let skip = tail.chars().count() - MAX_CHARS;
        format!("…{}", tail.chars().skip(skip).collect::<String>().trim_start())
    } else {
        tail
    };
    format!("\nПоследние строки вывода:\n{}", tail)
}

/// Символы, при которых строка перестаёт быть простым списком аргументов и
/// требует оболочки: перенаправление, цепочки, подстановка переменных,
/// группировка, экранирование. Наличие ЛЮБОГО из них означает `cmd`/`sh`.
const SHELL_METACHARACTERS: &[char] =
    &['&', '|', '<', '>', '^', '%', '!', '"', '\'', '(', ')', '{', '}', ';', '`', '\n', '\r', '\t'];

/// Разобрать npm-скрипт в исполняемый файл и аргументы — **без оболочки**.
///
/// Скрипты вида `node scripts/build/postinstall.mjs` оболочкой не пользуются:
/// у них нет ни перенаправлений, ни цепочек, ни подстановок. Запуск такого
/// скрипта через `cmd /c` добавляет посередине процесс, у которого правила
/// разбора аргументов другие, чем у `CreateProcess`: имя программы, кавычки и
/// `\` экранируются C-runtime при прямом запуске и по правилам `cmd` при
/// запуске через оболочку (тот же класс граблей, что с NSIS-инсталлером).
///
/// ## Чего это изменение НЕ доказывает
///
/// Раньше здесь стояло обоснование «измеренный факт 14.10: `npm install`
/// проходит, `postinstall` через `cmd` — нет». Этот факт **опровергнут**:
/// падение `0xC0000005` повторилось и при прямом запуске без `cmd.exe`.
/// Изменение остаётся как упрощение цепочки процесса, а не как лечение бага.
///
/// ## Что важно помнить про имя программы
///
/// `argv[0]` здесь остаётся голым именем (`node`), а не абсолютным путём, и
/// Windows разрешает его через PATH. Поэтому порядок PATH в
/// `package_script_command` — не косметика: он определяет, на каком Node
/// выполнится скрипт пакета.
///
/// `None` означает «нужна оболочка»: скрипт содержит метасимвол, кавычки или
/// не разбирается на программу и аргументы. Тогда вызывающий код честно идёт
/// через `cmd`/`sh` — но не молча.
pub fn direct_invocation(script: &str) -> Option<Vec<String>> {
    if script.chars().any(|c| SHELL_METACHARACTERS.contains(&c)) {
        return None;
    }
    let mut parts = script.split_whitespace();
    let program = parts.next()?.to_string();
    if program.is_empty() {
        return None;
    }
    let mut argv = vec![program];
    argv.extend(parts.map(str::to_string));
    Some(argv)
}

/// Команда запуска npm-скрипта пакета: напрямую, если оболочка не нужна,
/// иначе через `cmd /c` / `sh -c`.
fn package_script_command(
    node: &Path,
    dist: &Path,
    script: &str,
    scratch: &Path,
) -> std::process::Command {
    let direct = direct_invocation(script);
    let mut cmd = match &direct {
        Some(argv) => {
            log::debug!("cloud-routers: скрипт без оболочки: {}", script);
            let mut c = std::process::Command::new(&argv[0]);
            c.args(&argv[1..]);
            c
        }
        None if cfg!(windows) => {
            log::info!(
                "cloud-routers: скрипт требует оболочки, запуск через cmd: {}",
                script
            );
            let mut c = std::process::Command::new("cmd");
            c.arg("/d").arg("/s").arg("/c").arg(script);
            c
        }
        None => {
            log::info!(
                "cloud-routers: скрипт требует оболочки, запуск через sh: {}",
                script
            );
            let mut c = std::process::Command::new("sh");
            c.arg("-c").arg(script);
            c
        }
    };
// Скрипт пакета (`postinstall`) обязан идти на ТОМ ЖЕ node, на котором
    // потом запускается сервер роутера, и тем же, каким ставится `npm install`.
    // Для этого папка с нашим `node.exe` должна быть в PATH **первой**.
    //
    // Порядок был обратным: унаследованный PATH шёл первым, наш `node_dir`
    // последним. При унаследованном `C:\Program Files\nodejs` в PATH имя `node`
    // разрешалось в СИСТЕМНЫЙ Node, и postinstall собирался на нём, тогда как
    // `npm install` (запускается по полному пути) и сервер шли на нашем.
    // Три разных Node в одной установке — это не «мелочь», а расхождение
    // окружений между шагами одной операции.
    let node_dir = node.parent().unwrap_or(node);
    let npm_bin = node_dir.join("node_modules").join("npm").join("bin");
    let pkg_bin = dist.join("node_modules").join(".bin");
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    // PATH — это порядок поиска: наш node, потом npm-каталоги, потом унаследованный.
    let new_path = std::iter::once(node_dir.to_path_buf())
        .chain([npm_bin.clone(), pkg_bin])
        .chain(std::env::split_paths(&inherited));
    cmd.current_dir(dist)
        .env("PATH", std::env::join_paths(new_path).unwrap_or(inherited))
        .env("NODE_PATH", dist.join("node_modules"))
        .env("npm_execpath", npm_bin.join("npm-cli.js"));
    // Та же папка сбора, что и у npm: скрипт пакета тоже кладёт файлы во
    // временные, и его отказ выглядел бы так же, как отказ npm.
    apply_child_env(&mut cmd, scratch);
    cmd
}

/// Запустить команду и залогировать полный вывод. Ошибка → Err с контекстом,
/// успех → `Output` (вызывающий сам решает, что значит ненулевой код).
///
/// Отказ любого внешнего шага пишет в лог не только вывод процесса, но и
/// **условия его запуска**: точную argv, cwd и снимок окружения. Без них
/// отказ внешней программы недиагностичен — код возврата ничего не говорит о
/// том, с каким окружением она стартовала (об этом, в частности, был
/// неразрешимый вопрос по 0xC0000005 у `postinstall`).
fn run_captured(
    cmd: &mut std::process::Command,
    what: &str,
) -> Result<std::process::Output, String> {
    // Снимок берётся ДО `output()`: тот требует `&mut`, а отчёт — `&`.
    let report = crate::router::child_env::command_report(cmd);
    let started = Instant::now();
    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            log::error!(
                "{} не запустился ({}).\n{}",
                what,
                e,
                report
            );
            return Err(format!("Не удалось запустить {}: {}", what, e));
        }
    };
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Полный вывод — в лог (десятки КБ), в UI — короткая причина.
        log::error!(
            "{} упал за {:.1}c (code {:?})\n{}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            what,
            started.elapsed().as_secs_f64(),
            output.status.code(),
            report,
            stdout.trim(),
            stderr.trim(),
        );
    }
    Ok(output)
}

fn npm_tgz_url(version: &str, router_id: RouterId) -> String {
    let pkg = router_id.npm_package().unwrap_or_default();
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
            let p = crate::router::test_scratch::scratch_dir(&format!("cr_installer_{}", tag));
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

    /// Число мьютексов установки обязано совпадать с числом роутеров.
///
/// Индекс берётся из таблицы роутеров, а мьютексы — из массива, размер которого
/// задавался литералом. Расхождение давало панику «index out of bounds» в
/// момент установки у пользователя, а не ошибку компиляции. Тест закрывает
/// класс ошибок целиком.
#[test]
fn install_locks_count_matches_router_table() {
    assert_eq!(
        install_locks().len(),
        crate::spec::RouterId::ALL.len(),
        "мьютексов установки меньше, чем роутеров: install_guard упадёт в рантайме"
    );
    for router in crate::spec::RouterId::ALL.iter().copied() {
        assert!(
            router.index() < install_locks().len(),
            "индекс роутера {} вне массива мьютексов",
            router.as_str()
        );
    }
}

/// Мьютекс установки взаимно исключает запуск сервера: пока установка
    /// держит lock, второй поток БЛОКИРУЕТСЯ — значит сервер не поднимется
    /// поверх полураспакованного dist (корень бага «Cannot find module 'next'»).
    #[test]
    fn install_guard_blocks_start_until_install_finished() {
        use std::sync::mpsc;

        let router = RouterId::OmniRoute;
        let install_guard_held = install_locks()[router.index()].lock().unwrap();

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

    // ───────────────────── Транзакционность обновления ─────────────────────

    /// ГЛАВНОЕ свойство: пока новая версия не собрана, рабочая папка не
    /// тронута. Именно это отличает «обновление упало, роутер работает» от
    /// «обновление упало, роутер сломан».
    #[test]
    fn staging_failure_leaves_working_dist_untouched() {
        let root = crate::router::test_scratch::scratch_dir("swap_rollback");
        let dist = dist_dir(&root);
        fs::create_dir_all(dist.join("dist")).unwrap();
        fs::write(dist.join("dist").join("server.js"), "// v1").unwrap();
        fs::write(dist.join("version.txt"), "1.0.0").unwrap();

        let staging = staging_dir_for(&root, "2.0.0");
        {
            let guard = StagingGuard::new(staging.clone(), RouterId::OmniRoute);
            fs::create_dir_all(staging.join("dist")).unwrap();
            // Попытка «упала»: выходим из области без commit().
            drop(guard);
        }

        assert!(dist.join("dist").join("server.js").exists(), "рабочая папка пропала");
        assert_eq!(fs::read_to_string(dist.join("version.txt")).unwrap(), "1.0.0");
        assert!(
            staging.exists(),
            "промежуточная папка обязана пережить отказ — иначе повтор платит за npm install заново"
        );
    }

    /// Успешная подмена: рабочая папка становится новой, старая удаляется,
    /// мусора не остаётся.
    #[test]
    fn swap_dirs_installs_staging_and_removes_previous_copy() {
        let root = crate::router::test_scratch::scratch_dir("swap_ok");
        let dist = dist_dir(&root);
        fs::create_dir_all(&dist).unwrap();
        fs::write(dist.join("version.txt"), "1.0.0").unwrap();

        let staging = staging_dir_for(&root, "2.0.0");
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("version.txt"), "2.0.0").unwrap();

        let mut guard = StagingGuard::new(staging.clone(), RouterId::OmniRoute);
        swap_dirs(&root, &staging).expect("подмена");
        guard.commit();

        assert_eq!(
            fs::read_to_string(dist.join("version.txt")).unwrap(),
            "2.0.0",
            "после подмены должна работать новая версия"
        );
        assert!(!staging.exists(), "staging переехал на место dist");
        let leftovers: Vec<String> = fs::read_dir(&root)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with(OLD_PREFIX) || n.starts_with(STAGING_PREFIX))
            .collect();
        assert!(leftovers.is_empty(), "остался мусор: {:?}", leftovers);
    }

    /// Провал подмены возвращает рабочую версию на место. Сценарий:
    /// `dist` уже переименован, но `staging → dist` не удалось.
    #[test]
    fn swap_dirs_rolls_back_when_staging_cannot_be_moved() {
        let root = crate::router::test_scratch::scratch_dir("swap_fail");
        let dist = dist_dir(&root);
        fs::create_dir_all(&dist).unwrap();
        fs::write(dist.join("version.txt"), "1.0.0").unwrap();

        // staging не существует → переименование гарантированно падает.
        let missing = staging_dir_for(&root, "2.0.0");
        let err = swap_dirs(&root, &missing).expect_err("подмена без папки обязана упасть");

        assert!(
            err.contains("Прежняя версия"),
            "ошибка обязана говорить, что прежняя версия на месте: {}",
            err
        );
        assert!(dist.exists(), "рабочая папка должна быть восстановлена");
        assert_eq!(fs::read_to_string(dist.join("version.txt")).unwrap(), "1.0.0");
    }

    /// Мусор от прошлых попыток убирается, рабочая `dist` — нет.
    #[test]
    fn cleanup_removes_only_staging_and_old_folders() {
        let root = crate::router::test_scratch::scratch_dir("cleanup");
        let dist = dist_dir(&root);
        fs::create_dir_all(&dist).unwrap();
        let staging = staging_dir_for(&root, "2.0.0");
        let old = new_old_dir(&root);
        fs::create_dir_all(&staging).unwrap();
        fs::create_dir_all(&old).unwrap();

        cleanup_staging(&root, RouterId::OmniRoute, None);

        assert!(dist.exists(), "рабочая папка неприкосновенна");
        assert!(!staging.exists(), "staging прошлой попытки должен быть снесён");
        assert!(!old.exists(), "прежняя копия должна быть снесена");
    }

    // ───────────────── Неразрушающий staging и кэш tgz ─────────────────

    /// Гейт G2: папка текущей версии переживает уборку мусора, чужие версии —
    /// нет. Снос своей папки означал бы «начать заново», то есть ровно ту
    /// потерю 90 секунд и 131 МБ, ради устранения которой всё затевалось.
    #[test]
    fn cleanup_keeps_staging_of_current_version() {
        let root = crate::router::test_scratch::scratch_dir("cleanup_keep");
        let mine = staging_dir_for(&root, "3.8.51");
        let foreign = staging_dir_for(&root, "3.8.50");
        fs::create_dir_all(&mine).unwrap();
        fs::create_dir_all(&foreign).unwrap();

        cleanup_staging(&root, RouterId::OmniRoute, Some(&mine));

        assert!(mine.exists(), "свою папку версии снесли — работа потеряна");
        assert!(!foreign.exists(), "папка чужой версии — мусор, её сносим");
    }

    /// Имя staging выводится из версии, поэтому два разных ответа реестра
    /// никогда не сольются в одну папку.
    #[test]
    fn staging_name_is_derived_from_version() {
        let root = crate::router::test_scratch::scratch_dir("staging_name");
        assert_ne!(
            staging_dir_for(&root, "3.8.50"),
            staging_dir_for(&root, "3.8.51")
        );
        assert!(staging_dir_for(&root, "3.8.51").ends_with("dist_staging_3.8.51"));
    }

    /// Имя строится из сетевого ответа npm, поэтому разделители пути не должны
    /// в него попадать: иначе запись ушла бы мимо папки роутера.
    #[test]
    fn sanitize_strips_path_separators() {
        assert_eq!(sanitize_for_filename("../../evil"), "_.._evil");
        assert!(!sanitize_for_filename("..").contains(".."));
        assert_eq!(sanitize_for_filename("..."), "unknown");
        assert_eq!(sanitize_for_filename("3.8.51"), "3.8.51");
    }

    /// Готовое дерево признаётся пригодным только когда совпадает ВСЁ:
    /// версия пакета, маркер завершённого `npm install` и серверный скрипт.
    fn make_staging(root: &Path, version: &str) -> PathBuf {
        let staging = staging_dir_for(root, version);
        fs::create_dir_all(staging.join("dist")).unwrap();
        fs::write(
            staging.join("package.json"),
            format!(r#"{{"version":"{}"}}"#, version),
        )
        .unwrap();
        fs::write(staging.join("dist").join("server.js"), "//").unwrap();
        fs::create_dir_all(deps_marker(&staging).parent().unwrap()).unwrap();
        fs::write(deps_marker(&staging), b"{}").unwrap();
        staging
    }

    #[test]
    fn staging_reusable_requires_matching_version() {
        let root = crate::router::test_scratch::scratch_dir("reuse_ok");
        let staging = make_staging(&root, "3.8.51");
        assert_eq!(
            staging_reusable(&staging, "3.8.51", RouterId::OmniRoute),
            StagingState::Reusable
        );
        assert!(
            matches!(
                staging_reusable(&staging, "3.8.52", RouterId::OmniRoute),
                StagingState::Rebuild(_)
            ),
            "чужая версия в папке обязана приводить к пересборке"
        );
    }

    #[test]
    fn staging_reusable_requires_completed_npm_tree() {
        let root = crate::router::test_scratch::scratch_dir("reuse_deps");
        let staging = make_staging(&root, "3.8.51");
        fs::remove_file(deps_marker(&staging)).unwrap();
        assert!(
            matches!(
                staging_reusable(&staging, "3.8.51", RouterId::OmniRoute),
                StagingState::Rebuild(_)
            ),
            "недоустановленное дерево обязано приводить к пересборке"
        );
    }

    #[test]
    fn staging_reusable_requires_server_script() {
        let root = crate::router::test_scratch::scratch_dir("reuse_server");
        let staging = make_staging(&root, "3.8.51");
        fs::remove_file(staging.join("dist").join("server.js")).unwrap();
        assert!(matches!(
            staging_reusable(&staging, "3.8.51", RouterId::OmniRoute),
            StagingState::Rebuild(_)
        ));
    }

    #[test]
    fn staging_reusable_rejects_empty_folder() {
        let root = crate::router::test_scratch::scratch_dir("reuse_empty");
        let staging = staging_dir_for(&root, "3.8.51");
        fs::create_dir_all(&staging).unwrap();
        assert!(matches!(
            staging_reusable(&staging, "3.8.51", RouterId::OmniRoute),
            StagingState::Rebuild(_)
        ));
    }

    /// Гейт G1 (часть 1): tgz переживает попытку и читается из кэша без сети.
    #[test]
    fn tgz_cache_roundtrip() {
        let root = crate::router::test_scratch::scratch_dir("tgz_cache");
        let path = tgz_cache_path(&root, "3.8.51", RouterId::OmniRoute);
        assert!(read_cached_tgz(&path).is_none(), "кэша ещё нет");

        let payload = b"tgz-bytes".to_vec();
        write_cached_tgz(&path, &payload).expect("запись кэша");
        assert_eq!(read_cached_tgz(&path).as_deref(), Some(&payload[..]));
        assert!(
            !path.with_extension("tgz.0.part").exists(),
            "хвост записи не должен оставаться рядом с кэшем"
        );
    }

    /// Битая запись кэша не должна превращаться в тихую поломку: пустой файл
    /// кэшем не считается.
    #[test]
    fn empty_cache_file_is_not_a_cache_hit() {
        let root = crate::router::test_scratch::scratch_dir("tgz_cache_empty");
        let path = tgz_cache_path(&root, "3.8.51", RouterId::OmniRoute);
        write_cached_tgz(&path, b"").unwrap();
        assert!(read_cached_tgz(&path).is_none());
    }

    /// Кэш не должен расти на 131 МБ за версию.
    #[test]
    fn prune_keeps_only_current_version() {
        let root = crate::router::test_scratch::scratch_dir("tgz_prune");
        let current = tgz_cache_path(&root, "3.8.51", RouterId::OmniRoute);
        let old = tgz_cache_path(&root, "3.8.50", RouterId::OmniRoute);
        write_cached_tgz(&current, b"new").unwrap();
        write_cached_tgz(&old, b"old").unwrap();

        prune_tgz_cache(&root, &current, RouterId::OmniRoute);

        assert!(current.exists(), "кэш текущей версии должен остаться");
        assert!(!old.exists(), "кэш прошлой версии должен быть убран");
    }

    /// Гейт G4: отказ внешнего шага обязан оставить в логе argv, cwd и снимок
    /// окружения. Проверяем сам отчёт — до него руки были только у кода.
    #[test]
    fn command_report_contains_argv_cwd_and_env() {
        let mut cmd = std::process::Command::new("node");
        cmd.arg("scripts/build/postinstall.mjs");
        cmd.current_dir("D:\\stage");
        cmd.env("NODE_PATH", "D:\\stage\\node_modules");
        let report = crate::router::child_env::command_report(&cmd);
        assert!(report.contains("argv:"), "нет argv:\n{}", report);
        assert!(report.contains("postinstall.mjs"), "нет argv:\n{}", report);
        assert!(report.contains("D:\\stage"), "нет cwd:\n{}", report);
        assert!(report.contains("NODE_PATH="), "нет снимка окружения:\n{}", report);
    }

    /// Гейт «уже установлен»: без force работа пропускается, с force —
    /// выполняется. Раньше force игнорировался, и «Обновить сейчас» делал
    /// полную переустановку вместо обновления.
    #[test]
    fn install_action_respects_force_flag() {
        assert_eq!(install_action(true, false), InstallAction::Skip);
        assert_eq!(install_action(true, true), InstallAction::Build);
        assert_eq!(install_action(false, false), InstallAction::Build);
        assert_eq!(install_action(false, true), InstallAction::Build);
    }

    // ───────────────────────── Папка сбора дочерних ─────────────────────────

    /// Папка сбора обязана существовать, принимать запись и отдавать файл
    /// обратно: отказ удаления — тот самый класс проблемы, ради которого всё
    /// затевалось.
    #[test]
    fn scratch_dir_is_created_and_usable() {
        let root = crate::router::test_scratch::scratch_dir("scratch");
        let scratch = ensure_scratch(&root, RouterId::OmniRoute).expect("папка сбора");
        assert!(scratch.exists());
        assert_eq!(scratch, scratch_dir(&root), "путь обязан быть детерминированным");
        let probe = scratch.join("t.txt");
        fs::write(&probe, "x").unwrap();
        fs::remove_file(&probe).unwrap();
        assert!(!probe.exists(), "файл должен удаляться");
    }

    /// npm и postinstall обязаны получать ОДНИ И ТЕ ЖЕ переменные. Разъезд
    /// означал бы, что один из шагов снова ушёл в системную %TEMP%.
    #[test]
    fn child_env_covers_tmp_temp_and_tmpdir() {
        let env = child_env(Path::new("C:/scratch"));
        let keys: Vec<&str> = env.iter().map(|(k, _)| *k).collect();
        assert!(keys.contains(&"TMP"));
        assert!(keys.contains(&"TEMP"));
        assert!(keys.contains(&"TMPDIR"));
        assert!(
            env.iter().all(|(_, v)| v == Path::new("C:/scratch")),
            "все переменные указывают на одну папку сбора"
        );
    }

    /// Регрессия 14.10: `postinstall` (`node scripts/build/postinstall.mjs`) шёл
    /// через `cmd /d /s /c` и стабильно падал с `0xC0000005`, отдав обрезанный
    /// stdout и пустой stderr, при работающем `npm install` с тем же node и тем
    /// же деревом. Между ними была ровно одна разница — лишний процесс `cmd`.
    #[test]
    fn plain_js_script_does_not_go_through_the_shell() {
        assert_eq!(
            direct_invocation("node scripts/build/postinstall.mjs"),
            Some(vec![
                "node".to_string(),
                "scripts/build/postinstall.mjs".to_string()
            ])
        );
        assert_eq!(
            direct_invocation("  node   a.js  b.js  "),
            Some(vec![
                "node".to_string(),
                "a.js".to_string(),
                "b.js".to_string()
            ]),
            "лишние пробелы не должны ломать разбор"
        );
    }

    #[test]
    fn script_with_shell_syntax_still_goes_through_the_shell() {
        // Оболочка тут НЕ лишняя: `&&`, перенаправление и переменные без неё
        // не выполняются. Молча запустить такое напрямую значило бы тихо
        // выполнить не то.
        for script in [
            "node a.js && node b.js",
            "node a.js > out.txt",
            "node a.js 2>&1",
            "node \"%VAR%/a.js\"",
            "npm run build && npm run test",
            "node (a.js)",
        ] {
            assert_eq!(
                direct_invocation(script),
                None,
                "скрипт с синтаксисом оболочки обязан идти через неё: {}",
                script
            );
        }
        assert_eq!(direct_invocation("   "), None, "пустой скрипт не команда");
    }

    /// Скрипт пакета обязан видеть в PATH наш `node.exe`.
    ///
    /// Регрессия 14.10: `postinstall` собирал omniroute на Node из унаследованного
    /// PATH, а сервер потом запускался на портативном. Раньше в PATH клали только
    /// `npm/bin` — там `node.exe` нет, поэтому `cmd /c node` уезжал в
    /// `C:\Program Files\nodejs`. Скрипты пакета рассчитаны на npm-окружение
    /// нашего рантайма, а не на случайный Node с машины.
    #[test]
    fn package_script_command_puts_our_node_on_path() {
        let dist = crate::router::test_scratch::scratch_dir("pkg_cmd_path");
        let node = Path::new("C:/routers/omniroute/runtime/node.exe");
        let cmd = package_script_command(node, &dist, "node scripts/build/postinstall.mjs", Path::new("C:/scratch"));

        let path = cmd
            .get_envs()
            .find(|(k, _)| *k == std::ffi::OsStr::new("PATH"))
            .and_then(|(_, v)| v)
            .expect("PATH задан явно");
        let dirs: Vec<PathBuf> = std::env::split_paths(path).collect();
        let node_dir = node.parent().unwrap();
        // Порядок, а не просто присутствие: PATH — это порядок поиска. Наш node
        // обязан быть ПЕРВЫМ, иначе голое имя `node` разрешается в СИСТЕМНЫЙ
        // Node, а postinstall собирается на другом Node, чем сервер.
        //
        // Тест раньше утверждал обратное (`dirs.last()`) при сообщении
        // «обязан иметь приоритет» — то есть закреплял именно тот баг,
        // который чинится перестановкой.
        assert_eq!(
            dirs.first(),
            Some(&node_dir.to_path_buf()),
            "наш node обязан быть первым в PATH, иначе победит системный: {:?}",
            dirs
        );
        assert!(
            dirs.len() > 1,
            "унаследованный PATH должен сохраняться целиком после наших каталогов"
        );
    }

    // ─────────────────────── Блокировка между хостами ───────────────────────

    /// Живой владелец блокирует: папка установки общая для всех приложений,
    /// и второй хост не имеет права начинать установку поверх первого.
    #[test]
    fn install_lock_blocks_while_owner_is_alive() {
        let root = crate::router::test_scratch::scratch_dir("lock_busy");
        let path = InstallLock::path_for(&root);
        fs::write(&path, std::process::id().to_string()).unwrap();

        let err = InstallLock::acquire(&root, RouterId::OmniRoute)
            .expect_err("лок живого процесса обязан блокировать");
        assert!(err.contains("уже выполняется"), "неясная ошибка: {}", err);
        assert!(path.exists(), "чужой лок сносить нельзя");
    }

    /// Лок упавшего процесса снимается: иначе после сбоя установки роутер
    /// был бы заблокирован навсегда, до перезагрузки.
    #[test]
    fn install_lock_breaks_stale_owner() {
        let root = crate::router::test_scratch::scratch_dir("lock_stale");
        let path = InstallLock::path_for(&root);
        // PID 0 не существует как процесс — имитация упавшего окна.
        fs::write(&path, "0").unwrap();

        let lock = InstallLock::acquire(&root, RouterId::OmniRoute).expect("мёртвый лок снимается");
        drop(lock);
        assert!(!path.exists(), "лок снимается при выходе");
    }

    /// Удерживаемый лок виден второму acquire в этом же процессе (create_new
    /// не зависит от PID) — так же ведёт себя и второй хост.
    #[test]
    fn install_lock_is_exclusive_while_held() {
        let root = crate::router::test_scratch::scratch_dir("lock_hold");
        let _held = InstallLock::acquire(&root, RouterId::OmniRoute).expect("первый лок");
        assert!(
            InstallLock::acquire(&root, RouterId::OmniRoute).is_err(),
            "второй acquire обязан отказать, пока лок удерживается"
        );
    }

    /// Сообщение о сбое доходит до панели: без него пользователь видел только
    /// «код 1 — подробности в логе».
    #[test]
    fn failure_tail_carries_the_reason_to_the_ui() {
        let noise = "x".repeat(5000);
        let out = std::process::Output {
            status: std::process::Command::new("cmd").arg("/c").arg("exit 1").output().unwrap().status,
            stdout: noise.into_bytes(),
            stderr: b"npm ERR! code E404\nnpm ERR! 404 Not Found - GET https://registry.npmjs.org/next".to_vec(),
        };
        let tail = failure_tail(&out);
        assert!(tail.contains("E404"), "строка ошибки обязана попасть в хвост: {}", tail);
        assert!(
            tail.chars().count() < 1200,
            "хвост обязан быть обрезан, а не отдан целиком ({} символов)",
            tail.chars().count()
        );
    }
}
