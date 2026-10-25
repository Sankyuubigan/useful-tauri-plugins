//! Пул бандлов: одна общая папка для ВСЕХ наборов (image и video).
//!
//! До 25.10.2026 UI скачивал каждый бандл в подпапку `<base>/<bundle_name>`,
//! хотя имена общих ролей (llm/vae/mmproj) совпадают у всех бандлов каталога.
//! Результат: активная папка принадлежала последней успешной загрузке, и
//! переключение бандла давало «3 файла есть, diffusion — нет» (инцидент
//! юзера 25.10.2026, лог test/logs_2026-10-25_131442.txt, строки 2903+).
//!
//! Теперь пул плоский:
//! - downloader пропускает существующий файл с совпадающим размером
//!   («общие файлы не перекачиваются» — исчезает по построению);
//! - adoption: файл, оставшийся в легаси-подпапке (`<pool>/<bundle>/<file>`
//!   или любой подпапке пула) с совпадающим размером, переносится в пул
//!   (rename — мгновенно на том же томе; fallback — hardlink) вместо
//!   повторной загрузки гигабайтов.

use std::io;
use std::path::{Path, PathBuf};

/// Чем adoption перенёс файл в пул.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdoptHow {
    Moved,
    Linked,
}

/// Размер файла или `None`, если прочитать не удалось (битая ссылка и т.п.).
fn file_size(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|m| m.len())
}

/// Размер совпадает с ожидаемым (None в каталоге = сверить нечем, принимаем).
fn size_matches(actual: Option<u64>, expected: Option<u64>) -> bool {
    match expected {
        Some(exp) => actual == Some(exp),
        None => true,
    }
}

/// Найти файл с совпадающим размером в легаси-расположениях пула:
/// 1) `<pool>/<bundle_name>/<filename>` — каноническое легаси-место;
/// 2) `<pool>/<любая другая подпапка>/<filename>` — соседние per-bundle
///    подпапки (кроме `outputs/` — там артефакты генераций, не веса).
/// Сам `<pool>/<filename>` не проверяется: вызывающий уже установил, что
/// файла в пуле нет (иначе сработал бы skip downloader'а).
pub fn find_adoptable(
    pool: &Path,
    bundle_name: &str,
    filename: &str,
    expected_size: Option<u64>,
) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if !bundle_name.is_empty() {
        candidates.push(pool.join(bundle_name).join(filename));
    }
    if let Ok(entries) = std::fs::read_dir(pool) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == bundle_name || name == "outputs" {
                continue;
            }
            if entry.path().is_dir() {
                candidates.push(entry.path().join(filename));
            }
        }
    }
    candidates
        .into_iter()
        .find(|p| size_matches(file_size(p), expected_size))
}

/// Перенести файл в пул: сначала rename (атомарно и мгновенно на том же
/// томе), при отказе — hardlink (файл останется и в подпапке, но пул
/// консистентен). Ошибка возвращается вызывающему — он честно отдаст её
/// в UI вместо тихой повторной загрузки.
pub fn adopt_into_pool(src: &Path, dest: &Path) -> Result<AdoptHow, io::Error> {
    match std::fs::rename(src, dest) {
        Ok(()) => Ok(AdoptHow::Moved),
        Err(_) => {
            std::fs::hard_link(src, dest)?;
            Ok(AdoptHow::Linked)
        }
    }
}

/// Верификация бандла ПОСЛЕ скачивания (идея юзера 25.10.2026): каждый файл
/// каталога обязан существовать и совпадать размером с `size_bytes`. Без
/// этого «Бандл скачан целиком» был бы ложью при недокачке, а неполный бандл
/// доезжал бы до активации и рогатки generate_image.
/// `files` — пары (имя файла, ожидаемый размер) из каталога.
pub fn verify_files(files: &[(&str, Option<u64>)], dir: &Path) -> Result<(), String> {
    let mut problems: Vec<String> = Vec::new();
    for (filename, expected) in files {
        let path = dir.join(filename);
        match file_size(&path) {
            None => problems.push(format!("{} — файл отсутствует", filename)),
            Some(actual) => {
                if let Some(exp) = expected {
                    if actual != *exp {
                        problems.push(format!(
                            "{} — размер {} Б, ожидается {} Б (файл битый или недокачан)",
                            filename, actual, exp
                        ));
                    }
                }
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Бандл скачан не полностью ({}): {}. Нажмите «Докачать файлы» или повторите скачивание.",
            problems.len(),
            problems.join("; ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Синтетическая папка (без персональных данных; правило core §2.9).
    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("imgeng_pool_{}_{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(path: &Path, len: usize) {
        std::fs::write(path, vec![0u8; len]).unwrap();
    }

    #[test]
    fn adopt_moves_legacy_file_without_download() {
        let pool = tmpdir("move");
        let legacy = pool.join("bundle-a");
        std::fs::create_dir_all(&legacy).unwrap();
        write(&legacy.join("shared.gguf"), 1000);

        let found = find_adoptable(&pool, "bundle-b", "shared.gguf", Some(1000));
        assert_eq!(found, Some(legacy.join("shared.gguf")));

        let dest = pool.join("shared.gguf");
        let how = adopt_into_pool(found.as_ref().unwrap(), &dest).unwrap();
        assert_eq!(how, AdoptHow::Moved);
        assert!(dest.exists());
        assert_eq!(file_size(&dest), Some(1000));
        assert!(!legacy.join("shared.gguf").exists(), "файл перенесён, а не скопирован");
    }

    #[test]
    fn adopt_size_mismatch_is_rejected() {
        let pool = tmpdir("size");
        let legacy = pool.join("bundle-a");
        std::fs::create_dir_all(&legacy).unwrap();
        write(&legacy.join("diffusion.gguf"), 500);

        // Недокачка в легаси-подпапке: размер не совпал — adoption отказан,
        // вызывающий честно пойдёт в загрузчик.
        assert_eq!(
            find_adoptable(&pool, "bundle-b", "diffusion.gguf", Some(900)),
            None
        );
    }

    #[test]
    fn adopt_errors_honestly_when_source_missing() {
        let pool = tmpdir("err");
        let dest = pool.join("x.gguf");
        // Ни rename, ни hardlink не сработают — вызывающий получит Err и
        // честно пойдёт в загрузчик, а не притворится, что файл появился.
        assert!(adopt_into_pool(&pool.join("nope.gguf"), &dest).is_err());
        assert!(!dest.exists());
    }

    #[test]
    fn adopt_scans_sibling_subdirs_but_skips_outputs() {
        let pool = tmpdir("scan");
        std::fs::create_dir_all(pool.join("outputs")).unwrap();
        std::fs::create_dir_all(pool.join("old-quant")).unwrap();
        write(&pool.join("outputs").join("llm.gguf"), 100); // артефакт-папка не источник
        write(&pool.join("old-quant").join("llm.gguf"), 100);

        let found = find_adoptable(&pool, "new-bundle", "llm.gguf", Some(100));
        assert_eq!(found, Some(pool.join("old-quant").join("llm.gguf")));
    }

    #[test]
    fn verify_files_accepts_complete_and_rejects_missing_or_short() {
        let dir = tmpdir("verify");
        write(&dir.join("a.gguf"), 1000);
        write(&dir.join("b.safetensors"), 2000);

        assert!(verify_files(&[("a.gguf", Some(1000)), ("b.safetensors", Some(2000))], &dir).is_ok());
        // Размер не задан в каталоге — проверяется только существование.
        assert!(verify_files(&[("a.gguf", None)], &dir).is_ok());

        let err = verify_files(
            &[("a.gguf", Some(1000)), ("missing.gguf", Some(1))],
            &dir,
        )
        .unwrap_err();
        assert!(err.contains("missing.gguf — файл отсутствует"), "{}", err);
        assert!(err.contains("Докачать файлы"), "{}", err);

        let err2 = verify_files(&[("a.gguf", Some(999))], &dir).unwrap_err();
        assert!(err2.contains("размер 1000 Б, ожидается 999 Б"), "{}", err2);
    }
}
