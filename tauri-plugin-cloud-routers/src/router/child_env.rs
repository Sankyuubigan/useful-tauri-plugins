//! Диагностика дочерних процессов: точная команда и снимок окружения.
//!
//! ## Зачем это отдельный модуль
//!
//! Отказ внешнего шага (`npm install`, `postinstall`) диагностируется по логу
//! приложения, и лог обязан отвечать на вопрос «с каким argv, cwd и окружением
//! реально стартовал процесс». Собирать это прямо в `installer.rs` означало бы
//! втащить в установщик ещё и форматирование секретов — единственное место,
//! где легче всего допустить утечку ключа в лог. Здесь живут обе вещи рядом и
//! тестируются отдельно.
//!
//! ## Что маскируется, а что — нет (границы, заявленные честно)
//!
//! Маскируются **только** переменные, в ИМЕНИ которых есть маркер секрета.
//! Переменная с невинным именем и секретом в значении (после `npm login` это
//! `npm_config_*`) останется в логе как есть: модуль не знает и не может знать
//! содержимое таких значений. Это ограничение объявлено здесь, а не спрятано —
//! иначе снимок окружения выглядел бы гарантированно безопасным, а это не так.
//!
//! Маскировка меняет значение на `<скрыто: N символов>`: длина остаётся видна,
//! поэтому «переменная есть, но пустая» не притворяется «переменной нет».

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Расширения, которые Windows подставляет к имени программы без расширения.
/// Используется только если `PATHEXT` нет в окружении.
const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

/// Подстроки имён переменных, значение которых в лог не попадает.
/// Сравнение регистронезависимое (в Windows имена переменных нерегистрозависимы).
const SECRET_MARKERS: &[&str] = &[
    "TOKEN",
    "SECRET",
    "KEY",
    "PASS",
    "CREDENTIAL",
    "AUTH",
    "SESSION",
    "COOKIE",
    "SIGNATURE",
    "LICENSE",
    "PRIVATE",
];

/// Имя переменной помечено как секретное?
pub fn is_secret_key(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    SECRET_MARKERS.iter().any(|m| upper.contains(m))
}

/// Значение для лога: у секретного ключа — только длина.
pub fn redact_value(key: &str, value: &str) -> String {
    if is_secret_key(key) {
        format!("<скрыто: {} симв.>", value.chars().count())
    } else {
        value.to_string()
    }
}

/// Фактическое окружение дочернего процесса: унаследованное от хоста плюс
/// явные `env`/`env_remove` из команды.
///
/// Именно «фактическое», а не только явно заданное: `Command::get_envs()`
/// показывает лишь немногое, а диагностическая ценность снимка — в том, что
/// в нём видно, какие переменные ребёнок унаследовал от GUI-хоста. Именно их
/// до сих пор не снимали ни в одном эксперименте.
pub fn effective_env(cmd: &Command) -> BTreeMap<String, String> {
    let mut env: BTreeMap<String, String> = std::env::vars_os()
        .filter_map(|(k, v)| {
            let key = k.to_string_lossy().to_string();
            let val = v.to_string_lossy().to_string();
            Some((key, val))
        })
        .collect();

    for (key, value) in cmd.get_envs() {
        let key = key.to_string_lossy().to_string();
        // Windows сравнивает имена переменных без учёта регистра, поэтому и мы
        // обязаны: иначе `Path=...` добавил бы вторую `PATH` рядом с унаследованной.
        let existing = env
            .keys()
            .find(|k| k.eq_ignore_ascii_case(&key))
            .cloned();
        if let Some(k) = existing {
            env.remove(&k);
        }
        match value {
            Some(v) => {
                env.insert(key, v.to_string_lossy().to_string());
            }
            // `env_remove` — переменной у ребёнка не будет вовсе.
            None => {
                env.remove(&key);
            }
        }
    }
    env
}

/// Полный отчёт о команде для журнала отказа: argv, cwd и снимок окружения.
pub fn command_report(cmd: &Command) -> String {
    let argv = cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    let program = cmd.get_program().to_string_lossy().to_string();
    let all = std::iter::once(program.clone())
        .chain(argv)
        .collect::<Vec<_>>();
    let cwd = cmd
        .get_current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<наследуется от хоста>".to_string());

    let env = effective_env(cmd);

    let mut out = String::new();
    out.push_str(&format!("argv: {}", all.join(" | ")));
    out.push('\n');
    out.push_str(&format!("resolved: {}", describe_resolved(&program, &env)));
    out.push('\n');
    out.push_str(&format!("cwd: {}", cwd));
    out.push('\n');
    out.push_str(&format!("окружение ({} переменн.):", env.len()));
    for (k, v) in &env {
        out.push_str(&format!("\n  {}={}", k, redact_value(k, v)));
    }
    out
}

/// Разрешить имя программы в абсолютный путь так, как это делает поиск по PATH.
///
/// ## Зачем это в отчёте
///
/// Когда программа задана голым именем (`node`), argv этого не показывает, а
/// для разбора отказа различие принципиально: тот же `node` разрешается то в
/// наш runtime, то в системный, и в логе без этой строки оба случая выглядят
/// одинаково.
///
/// ## Границы воспроизведённого
///
/// Воспроизведён ТОЛЬКО поиск по PATH с подстановкой `PATHEXT`. Полный порядок
/// `CreateProcess` (каталог приложения, текущий каталог, System32) здесь не
/// воспроизведён. Для нашего случая — имя `node` и папка с нашим `node.exe`
/// в PATH — этого достаточно, но называть это полным совпадением с
/// `CreateProcess` нельзя.
pub fn resolve_program(program: &str, env: &BTreeMap<String, String>) -> Option<PathBuf> {
    let candidate = Path::new(program);
    // С разделителем пути имя абсолютно или относительно — ищем как есть.
    if program.contains('\\') || program.contains('/') {
        return candidate.is_file().then(|| candidate.to_path_buf());
    }

    let extensions: Vec<String> = env
        .get("PATHEXT")
        .map(|v| v.split(';').map(|s| s.trim().to_string()).collect())
        .unwrap_or_else(|| {
            DEFAULT_PATHEXT
                .split(';')
                .map(|s| s.to_string())
                .collect()
        });

    for dir in std::env::split_paths(
        env.get("PATH")
            .map(PathBuf::from)
            .unwrap_or_default()
            .as_os_str(),
    ) {
        // Сначала имя как есть, затем с расширениями: Windows проверяет так же.
        let names = std::iter::once(String::new())
            .chain(extensions.iter().map(|e| e.to_lowercase()));
        for ext in names {
            let file = dir.join(format!("{}{}", program, ext));
            if file.is_file() {
                return Some(file);
            }
        }
    }
    None
}

/// Строка «resolved» для отчёта: путь плюс признаки файла, по которым в логе
/// видно, КАКОЙ именно экземпляр запущен, даже не открывая его свойства.
fn describe_resolved(program: &str, env: &BTreeMap<String, String>) -> String {
    let Some(path) = resolve_program(program, env) else {
        return format!("{} — НЕ НАЙДЕН в PATH дочернего процесса", program);
    };
    match std::fs::metadata(&path) {
        Ok(meta) => format!(
            "{} -> {} ({} Б, изменён {})",
            program,
            path.display(),
            meta.len(),
            modified_hint(&meta)
        ),
        Err(_) => format!("{} -> {}", program, path.display()),
    }
}

/// Время изменения файла в виде «год-месяц-день час:мин:сек» UTC — без
/// зависимостей и без локали. Это признак экземпляра, а не точная метка.
fn modified_hint(meta: &std::fs::Metadata) -> String {
    let Ok(modified) = meta.modified() else {
        return "?".to_string();
    };
    let Ok(since) = modified.duration_since(std::time::UNIX_EPOCH) else {
        return "?".to_string();
    };
    let secs = since.as_secs();
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}Z",
        secs / 31_622_400 + 1970,
        (secs % 31_622_400) / 2_592_000 + 1,
        (secs % 2_592_000) / 86_400 + 1,
        (secs % 86_400) / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn секретные_имена_определяются_без_учета_регистра() {
        assert!(is_secret_key("NPM_TOKEN"));
        assert!(is_secret_key("anthropic_api_key"));
        assert!(is_secret_key("PGPASSWORD"));
        assert!(!is_secret_key("PATH"));
        assert!(!is_secret_key("TEMP"));
        assert!(!is_secret_key("NODE_PATH"));
    }

    #[test]
    fn секретное_значение_заменяется_но_длина_сохраняется() {
        let out = redact_value("NPM_TOKEN", "abcd");
        assert!(!out.contains("abcd"), "значение утекло в лог: {}", out);
        assert!(out.contains('4'), "длина обязана остаться видимой: {}", out);
    }

    #[test]
    fn несекретное_значение_пишется_как_есть() {
        assert_eq!(redact_value("PATH", r"C:\nodejs"), r"C:\nodejs");
    }

    #[test]
    fn снимок_содержит_явные_переменные_команды() {
        let mut cmd = Command::new("cmd");
        cmd.env("NODE_PATH", r"D:\dist\node_modules");
        let env = effective_env(&cmd);
        assert_eq!(
            env.get("NODE_PATH").map(String::as_str),
            Some(r"D:\dist\node_modules")
        );
    }

    #[test]
    fn env_remove_убирает_унаследованную_переменную() {
        let key = "CR_TEST_REMOVE_ME";
        std::env::set_var(key, "present");
        let mut cmd = Command::new("cmd");
        cmd.env_remove(key);
        let env = effective_env(&cmd);
        assert!(!env.contains_key(key), "env_remove не применился");
        std::env::remove_var(key);
    }

    #[test]
    fn переопределение_не_оставляет_дубль_имени() {
        std::env::set_var("CR_TEST_DUP", "old");
        let mut cmd = Command::new("cmd");
        cmd.env("cr_test_dup", "new");
        let env = effective_env(&cmd);
        let dups = env.keys().filter(|k| k.eq_ignore_ascii_case("CR_TEST_DUP")).count();
        assert_eq!(dups, 1, "в снимке две записи одного имени:\n{:#?}", env);
        assert_eq!(env.get("cr_test_dup").map(String::as_str), Some("new"));
        std::env::remove_var("CR_TEST_DUP");
    }

    #[test]
    fn отчёт_содержит_argv_cwd_и_окружение() {
        let mut cmd = Command::new("node");
        cmd.arg("scripts/build/postinstall.mjs");
        cmd.current_dir("D:\\stage");
        cmd.env("NPM_TOKEN", "super-secret-value");
        let report = command_report(&cmd);
        assert!(report.contains("postinstall.mjs"), "нет argv: {}", report);
        assert!(report.contains("D:\\stage"), "нет cwd: {}", report);
        assert!(report.contains("окружение ("), "нет снимка: {}", report);
        assert!(
            !report.contains("super-secret-value"),
            "секрет утекёл в отчёт:\n{}",
            report
        );
    }

    // ───────────── Разрешение программы: какой бинарь запустится ─────────────
    //
    // Без этой строки в логе падение на системном Node и падение на нашем
    // Node выглядят одинаково — а это ровно то различие, которое проверяется.

    /// Заглушка «нашего» node в отдельной папке, плюс папка с «системным».
    fn two_node_dirs(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = crate::router::test_scratch::scratch_dir(tag);
        let ours = root.join("runtime");
        let theirs = root.join("system_node");
        std::fs::create_dir_all(&ours).unwrap();
        std::fs::create_dir_all(&theirs).unwrap();
        std::fs::write(ours.join("node.exe"), b"our-node").unwrap();
        std::fs::write(theirs.join("node.exe"), b"sys-node").unwrap();
        (root, ours, theirs)
    }

    #[test]
    fn разрешается_первый_совпавший_каталог_в_path() {
        let (root, ours, theirs) = two_node_dirs("resolve_order");
        let mut env = BTreeMap::new();
        env.insert(
            "PATH".to_string(),
            std::env::join_paths([theirs.clone(), ours.clone()])
                .unwrap()
                .to_string_lossy()
                .to_string(),
        );
        let found = resolve_program("node", &env).expect("node.exe должен найтись");
        assert_eq!(found, theirs.join("node.exe"), "победил первый каталог в PATH");
        let _ = root;
    }

    #[test]
    fn порядок_path_меняет_какой_node_победит() {
        let (root, ours, theirs) = two_node_dirs("resolve_order_flipped");
        let mut env = BTreeMap::new();
        env.insert(
            "PATH".to_string(),
            std::env::join_paths([ours.clone(), theirs.clone()])
                .unwrap()
                .to_string_lossy()
                .to_string(),
        );
        let found = resolve_program("node", &env).expect("node.exe должен найтись");
        assert_eq!(found, ours.join("node.exe"), "наш node обязан победить первым");
        let _ = root;
    }

    #[test]
    fn расширение_подставляется_как_у_windows() {
        let (root, ours, _theirs) = two_node_dirs("resolve_pathext");
        let mut env = BTreeMap::new();
        env.insert(
            "PATH".to_string(),
            ours.to_string_lossy().to_string(),
        );
        // Ищем без расширения, а на диске только `node.exe`.
        let found = resolve_program("node", &env).expect("node.exe должен найтись через PATHEXT");
        assert_eq!(found, ours.join("node.exe"));
        let _ = root;
    }

    #[test]
    fn несуществующая_программа_даёт_явное_не_найдено() {
        let (root, ours, _) = two_node_dirs("resolve_missing");
        let mut cmd = Command::new("definitely-not-a-real-program-xyz");
        cmd.env("PATH", &ours);
        let report = command_report(&cmd);
        assert!(
            report.contains("НЕ НАЙДЕН"),
            "отсутствие программы обязано быть видно:\n{}",
            report
        );
        let _ = root;
    }

    #[test]
    fn отчёт_печатает_абсолютный_путь_программы() {
        let (root, ours, theirs) = two_node_dirs("report_resolved");
        let mut cmd = Command::new("node");
        cmd.env("PATH", &theirs);
        let report = command_report(&cmd);
        assert!(
            report.contains(&theirs.join("node.exe").display().to_string()),
            "в отчёте нет фактически запущенного бинаря:\n{}",
            report
        );
        let _ = (root, ours);
    }
}