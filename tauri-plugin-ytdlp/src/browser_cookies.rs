//! Кукисы браузера для yt-dlp (anti-bot YouTube) — перенос механики dpi_gui:
//! крейт rookie 0.5.6, захват по имени браузера ИЛИ по папке User Data
//! (любой Chromium-форк: ключ Local State + базы Default / Profile *).
//!
//! Приватность: кукисы живут ТОЛЬКО в cookies.txt (Netscape) в данных юзера;
//! в лог — количества и источники, НИКОГДА значения.

use rookie::common::enums::Cookie;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

/// Браузеры для выпадающего списка UI (SSOT — функции крейта rookie 0.5.6).
pub const BROWSERS: &[(&str, &str)] = &[
    ("chrome", "Chrome"),
    ("edge", "Edge"),
    ("firefox", "Firefox"),
    ("brave", "Brave"),
    ("opera", "Opera"),
    ("opera_gx", "Opera GX"),
    ("vivaldi", "Vivaldi"),
    ("chromium", "Chromium"),
    ("arc", "Arc"),
    ("librewolf", "LibreWolf"),
    ("zen", "Zen"),
];

/// Домены YouTube-аутентификации: вход — на youtube.com, SAPISID и пр. лежат
/// на .google.com. Фильтр rookie — contains по домену кукисы.
const YT_DOMAINS: &[&str] = &["youtube.com", "google.com"];

/// Результат захвата: сколько свежих кукисов, строка источника, честные ноты.
/// Значения кукисов не входят ни в одно поле.
#[derive(Serialize, Clone)]
pub struct GrabOutcome {
    pub count: usize,
    pub source: String,
    pub notes: Vec<String>,
}

/// Кукиса для cookies.txt: локальная копия полей rookie::Cookie (домен в
/// нижнем регистре). host_only у rookie нет — см. cookies_to_netscape.
#[derive(Clone, Debug)]
struct SavedCookie {
    domain: String,
    path: String,
    name: String,
    value: String,
    expires: Option<u64>,
    secure: bool,
    http_only: bool,
}

/// Путь сохранённого cookies.txt в данных юзера.
pub fn saved_cookies_path(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("ytdlp_cookies.txt")
}

/// Захват + запись cookies.txt. 0 кукисов → Ok с нотами, файл НЕ
/// перезаписывается (чтобы не убить рабочий cookies.txt прошлой попытки).
pub fn grab_and_save(
    app: &AppHandle,
    browser: Option<&str>,
    profile_dir: Option<&str>,
) -> Result<(GrabOutcome, PathBuf), String> {
    let path = saved_cookies_path(app);
    let doms: Vec<String> = YT_DOMAINS.iter().map(|s| (*s).to_string()).collect();
    let (cookies, source, mut notes) = if let Some(dir) =
        profile_dir.filter(|s| !s.trim().is_empty())
    {
        grab_from_dir(dir, &doms)?
    } else {
        grab_from_name(browser.unwrap_or("chrome"), &doms)?
    };
    let count = cookies.len();
    if count == 0 {
        notes.push(
            "Кукисов для youtube.com/google.com не найдено — залогиньтесь в YouTube \
             в выбранном браузере и попробуйте снова."
                .to_string(),
        );
        notes.push(
            "Файл cookies.txt не перезаписан (сохранён предыдущий, если был)."
                .to_string(),
        );
        log::warn!("[YTDLP] Кукисы: 0 шт. (источник: {source}) — cookies.txt не перезаписан");
        return Ok((GrabOutcome { count: 0, source, notes }, path));
    }
    let text = cookies_to_netscape(&cookies);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Не удалось создать директорию: {e}"))?;
    }
    std::fs::write(&path, text).map_err(|e| format!("Не удалось записать cookies.txt: {e}"))?;
    log::info!(
        "[YTDLP] Кукисы: {count} шт. (источник: {source}) — записаны в {}",
        path.display()
    );
    Ok((GrabOutcome { count, source, notes }, path))
}

/// Захват по имени браузера (rookie знает стандартные пути). Ошибка чтения
/// базы — не Err команды: ноты объясняют (ABE, «залогиньтесь»), как в dpi_gui.
fn grab_from_name(
    name: &str,
    doms: &[String],
) -> Result<(Vec<SavedCookie>, String, Vec<String>), String> {
    let label = browser_label(name)?.to_string();
    match rookie_by_name(name, doms) {
        Ok(list) => {
            let cookies: Vec<SavedCookie> = filter_fresh(&list);
            let n = cookies.len();
            let mut notes = Vec::new();
            if n == 0 {
                notes.push(format!(
                    "{label}: кукисов для youtube.com/google.com нет — залогиньтесь \
                     в этом браузере и повторите"
                ));
                if is_chromium_family(name) {
                    notes.push(
                        "Chrome/Edge ≥127 может не отдать ключ шифрования (App-Bound \
                         Encryption): если вход точно есть — возьми Firefox/LibreWolf, \
                         папку User Data другого Chromium-форка или режим «Файл» (cookies.txt)."
                            .to_string(),
                    );
                }
            }
            Ok((cookies, label, notes))
        }
        Err(e) => {
            let mut notes = vec![format!("{label} — не удалось прочитать кукисы: {e}")];
            if is_chromium_family(name) {
                notes.push(
                    "Chrome/Edge ≥127 может не отдать ключ шифрования (App-Bound \
                     Encryption): возьми Firefox/LibreWolf, папку User Data другого \
                     Chromium-форка или режим «Файл» (cookies.txt)."
                        .to_string(),
                );
            }
            Ok((Vec::new(), label, notes))
        }
    }
}

/// Захват из указанной юзером папки User Data — любой Chromium-форк (Thorium,
/// Яндекс и пр.): ключ — Local State, базы — Default и Profile * (старые
/// Cookies и новые Network/Cookies, как в config.json rookie). Захват —
/// rookie::any_browser. Перенос grab_from_dir из dpi_gui.
fn grab_from_dir(
    dir: &str,
    doms: &[String],
) -> Result<(Vec<SavedCookie>, String, Vec<String>), String> {
    let base = PathBuf::from(dir.trim().trim_matches('"'));
    if !base.is_dir() {
        return Err(format!("папка не найдена: {}", base.display()));
    }
    let cookie_files = find_cookie_bases(&base);
    if cookie_files.is_empty() {
        return Err(
            "в папке нет кукис-баз Chromium (ожидаются Default/Cookies, \
             Default/Network/Cookies, Profile */Cookies) — укажи именно папку \
             User Data браузера (Chromium-форк)"
                .to_string(),
        );
    }
    let key_path = base.join("Local State");
    if !key_path.is_file() {
        return Err(
            "в папке нет файла ключей Local State — укажи именно папку User Data \
             Chromium-браузера (Firefox так не поддерживается — выбери «Firefox» \
             в списке браузеров)"
                .to_string(),
        );
    }
    let key = key_path.to_string_lossy().into_owned();
    let mut cookies: Vec<SavedCookie> = Vec::new();
    let mut fails: Vec<String> = Vec::new();
    for cf in &cookie_files {
        match rookie::any_browser(&cf.to_string_lossy(), Some(doms.to_vec()), Some(&key)) {
            Ok(list) => cookies.extend(filter_fresh(&list)),
            Err(e) => fails.push(format!(
                "{}: {e}",
                cf.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            )),
        }
    }
    // Метка источника: имя родительской папки («Thorium», «YandexBrowser») —
    // сама папка обычно называется «User Data»; без родителя — имя папки.
    let label = format!(
        "Папка: {}",
        base.parent()
            .and_then(|p| p.file_name())
            .or_else(|| base.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| base.to_string_lossy().into_owned())
    );
    Ok((cookies, label, fails))
}

/// Cookies-базы Chromium в папке User Data: Default и Profile * (старый
/// <профиль>/Cookies и новый <профиль>/Network/Cookies — как в config.json
/// rookie). Сортировка детерминирована.
fn find_cookie_bases(base: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let d = base.join("Default");
    if d.is_dir() {
        dirs.push(d);
    }
    if let Ok(rd) = std::fs::read_dir(base) {
        for e in rd.filter_map(|e| e.ok()) {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("Profile ") && e.path().is_dir() {
                dirs.push(e.path());
            }
        }
    }
    let mut out = Vec::new();
    for d in dirs {
        for sub in ["Cookies", "Network/Cookies"] {
            let p = d.join(sub);
            if p.is_file() {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Сессионная кукиса (expires: None) — держим: именно они несут логин.
/// Истёкшая (expires <= now) — прочь.
fn not_expired(expires: Option<u64>, now: u64) -> bool {
    expires.map_or(true, |e| e > now)
}

/// Unix-секунды сейчас.
fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// rookie::Cookie → SavedCookie (домен в нижний регистр), истёкшие прочь.
fn filter_fresh(list: &[Cookie]) -> Vec<SavedCookie> {
    let now = unix_now();
    list.iter()
        .filter(|c| not_expired(c.expires, now))
        .map(|c| SavedCookie {
            domain: c.domain.to_lowercase(),
            path: c.path.clone(),
            name: c.name.clone(),
            value: c.value.clone(),
            expires: c.expires,
            secure: c.secure,
            http_only: c.http_only,
        })
        .collect()
}

/// id → функция крейта. any_browser у rookie принимает ПУТИ (не диспетчер по
/// имени), поэтому матчинг здесь.
fn rookie_by_name(name: &str, doms: &[String]) -> Result<Vec<Cookie>, String> {
    let d = Some(doms.to_vec());
    let raw = match name {
        "chrome" => rookie::chrome(d),
        "edge" => rookie::edge(d),
        "firefox" => rookie::firefox(d),
        "brave" => rookie::brave(d),
        "opera" => rookie::opera(d),
        "opera_gx" => rookie::opera_gx(d),
        "vivaldi" => rookie::vivaldi(d),
        "chromium" => rookie::chromium(d),
        "arc" => rookie::arc(d),
        "librewolf" => rookie::librewolf(d),
        "zen" => rookie::zen(d),
        _ => return Err(format!("неизвестный браузер «{name}»")),
    };
    raw.map_err(|e| e.to_string())
}

fn browser_label(id: &str) -> Result<&'static str, String> {
    BROWSERS
        .iter()
        .find(|(bid, _)| *bid == id)
        .map(|(_, l)| *l)
        .ok_or_else(|| format!("неизвестный браузер «{id}»"))
}

/// Chromium-семейство (App-Bound Encryption актуален только для них).
fn is_chromium_family(id: &str) -> bool {
    matches!(
        id,
        "chrome" | "edge" | "brave" | "chromium" | "vivaldi" | "opera" | "opera_gx" | "arc"
    )
}

/// SavedCookie → Netscape cookies.txt (--cookies у yt-dlp). Формат строки:
/// domain \t include_subdomains \t path \t secure \t expiry \t name \t value.
/// host_only у rookie нет — ведущая точка = доменная кукиса (includeSubdomains).
/// Сессионные (expires None) → 2147483647, иначе yt-dlp сессионные отбросит.
/// http_only → префикс #HttpOnly_ (convention Netscape-парсеров).
fn cookies_to_netscape(cookies: &[SavedCookie]) -> String {
    let mut out = String::from("# Netscape HTTP Cookie File\n");
    for c in cookies {
        let domain = if c.domain.starts_with('.') {
            c.domain.clone()
        } else {
            format!(".{}", c.domain)
        };
        let first = if c.http_only {
            format!("#HttpOnly_{domain}")
        } else {
            domain
        };
        let expiry = c.expires.unwrap_or(2_147_483_647);
        out.push_str(&format!(
            "{}\tTRUE\t{}\t{}\t{}\t{}\t{}\n",
            first,
            c.path,
            if c.secure { "TRUE" } else { "FALSE" },
            expiry,
            c.name,
            c.value
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn netscape_conversion() {
        let cookies = vec![
            SavedCookie {
                domain: "youtube.com".to_string(),
                path: "/".to_string(),
                name: "VISITOR_INFO1_LIVE".to_string(),
                value: "v1".to_string(),
                expires: Some(1_800_000_000),
                secure: true,
                http_only: false,
            },
            SavedCookie {
                domain: ".google.com".to_string(),
                path: "/".to_string(),
                name: "SAPISID".to_string(),
                value: "s1".to_string(),
                expires: None,
                secure: true,
                http_only: true,
            },
        ];
        let text = cookies_to_netscape(&cookies);
        let mut lines = text.lines().skip(1);
        assert_eq!(
            lines.next().unwrap(),
            ".youtube.com\tTRUE\t/\tTRUE\t1800000000\tVISITOR_INFO1_LIVE\tv1"
        );
        // host-only домен получает ведущую точку; http_only → #HttpOnly_;
        // сессионная (expires None) → 2147483647.
        assert_eq!(
            lines.next().unwrap(),
            "#HttpOnly_.google.com\tTRUE\t/\tTRUE\t2147483647\tSAPISID\ts1"
        );
    }

    #[test]
    fn not_expired_rules() {
        assert!(not_expired(None, 100));
        assert!(!not_expired(Some(50), 100));
        assert!(not_expired(Some(150), 100));
    }

    #[test]
    fn find_cookie_bases_default_and_profiles() {
        let root = std::env::temp_dir().join(format!("ytdlp_cookietest_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dn = root.join("Default").join("Network");
        std::fs::create_dir_all(&dn).unwrap();
        std::fs::write(dn.join("Cookies"), b"").unwrap();
        let p1 = root.join("Profile 1");
        std::fs::create_dir_all(&p1).unwrap();
        std::fs::write(p1.join("Cookies"), b"").unwrap();
        let mut want = vec![
            root.join("Default").join("Network").join("Cookies"),
            p1.join("Cookies"),
        ];
        want.sort();
        assert_eq!(find_cookie_bases(&root), want);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn browser_label_and_family() {
        assert_eq!(browser_label("chrome").unwrap(), "Chrome");
        assert!(browser_label("nope").is_err());
        assert!(is_chromium_family("edge"));
        assert!(!is_chromium_family("firefox"));
        assert!(!is_chromium_family("zen")); // Zen — форк Firefox, ABE не его
    }
}
