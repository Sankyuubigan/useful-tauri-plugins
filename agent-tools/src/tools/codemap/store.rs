//! Хранение индекса на диске и его инкрементальная пересборка.
//!
//! ## Где лежит индекс
//!
//! `<корень>/.codemap/index.json` — рядом с кодом, который он описывает, иначе
//! инструмент не знал бы, к какому репозиторию относится найденное.
//!
//! ## Почему `.git/info/exclude`, а не `.gitignore`
//!
//! Индекс — машинный артефакт, а не часть проекта. Правка `.gitignore` означала
//! бы, что инструмент меняет отслеживаемый файл чужого репозитория. В
//! `.git/info/exclude` попадает только локальное правило, оно не коммитится и не
//! попадает в чужие клоны. Правило дописывается один раз и идемпотентно.
//!
//! ## Почему запись атомарная
//!
//! Индекс перезаписывается целиком. Обрыв записи посередине оставил бы после
//! себя обрезанный JSON, который при следующем запуске выглядел бы как
//! «индекс есть, но пустой» — то есть агент получил бы молчаливо неполную
//! карту. Запись идёт через `ko-json-store` (tmp → `sync_all` → `rename`).

use std::fs;
use std::path::{Path, PathBuf};

use crate::tools::codemap::model::{FileEntry, Index, FORMAT_VERSION};

use super::flat_lang::FlatChunk;
use super::lang::Lang;
use super::scan;
use super::tags::Tag;

/// Имя каталога индекса внутри корня.
const DIR_NAME: &str = ".codemap";

/// Имя файла индекса.
const FILE_NAME: &str = "index.json";

/// Каталог индекса для корня репозитория.
pub fn index_dir(root: &Path) -> PathBuf {
    root.join(DIR_NAME)
}

/// Путь к файлу индекса.
pub fn index_path(root: &Path) -> PathBuf {
    index_dir(root).join(FILE_NAME)
}

/// Итог пересборки: что произошло, а не «сколько получилось».
///
/// Считается именно потому, что «индекс построен» и «ничего не пришлось
/// разбирать заново» — это разные исходы, и второй важнее: он доказывает, что
/// кеш работает.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BuildStats {
    /// Файлов найдено обходом.
    pub scanned: usize,
    /// Файлов разобрано заново.
    pub parsed: usize,
    /// Файлов взято из кеша без повторного разбора.
    pub reused: usize,
    /// Файлов, которые не удалось разобрать; они ушли в плоский индекс.
    pub failed: usize,
    /// Двоичных файлов, пропущенных без разбора и без ошибки.
    pub binary: usize,
}

/// Сохранить индекс на диск и исключить каталог из Git.
pub fn save(root: &Path, index: &Index) -> Result<(), String> {
    let dir = index_dir(root);
    fs::create_dir_all(&dir).map_err(|e| format!("не создать {}: {e}", dir.display()))?;

    let json = serde_json::to_string(index).map_err(|e| format!("не сериализовать индекс: {e}"))?;
    ko_json_store::write_atomic(&index_path(root), &json)
        .map_err(|e| format!("не записать {}: {e}", index_path(root).display()))?;

    // Ошибка исключения из Git не должна провалить запись индекса: индекс уже
    // на диске и полезен. Но молчать о ней нельзя — иначе пользователь увидит
    // грязный `git status` и не поймёт, откуда взялась папка.
    if let Err(reason) = exclude_from_git(root) {
        log::warn!("codemap: {reason}");
    }
    Ok(())
}

/// Загрузить индекс. Отсутствие файла — не ошибка, а «индекс ещё не построен».
pub fn load(root: &Path) -> Result<Option<Index>, String> {
    let path = index_path(root);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("не прочитать {}: {e}", path.display())),
    };
    let index: Index = serde_json::from_str(&text)
        .map_err(|e| format!("index.json повреждён ({}): {e}", path.display()))?;
    if index.version != FORMAT_VERSION {
        return Ok(None);
    }
    Ok(Some(index))
}

/// Дописать `.codemap/` в `.git/info/exclude`, если правила там ещё нет.
///
/// Возвращает `Ok(false)`, если репозиторий не под git — тогда и исключать
/// нечего, и это не ошибка.
pub fn exclude_from_git(root: &Path) -> Result<bool, String> {
    let git = root.join(".git");
    // В worktree `.git` — файл со ссылкой на настоящий каталог. Он тоже
    // означает git-репозиторий, поэтому проверяем и файл, и каталог.
    if !git.exists() {
        return Ok(false);
    }
    let info = git.join("info");
    fs::create_dir_all(&info).map_err(|e| format!("не создать {}: {e}", info.display()))?;

    let exclude = info.join("exclude");
    let existing = fs::read_to_string(&exclude).unwrap_or_default();
    if existing.lines().any(|line| line.trim() == format!("{DIR_NAME}/")) {
        return Ok(false);
    }

    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(&format!(
        "# добавлено agent-tools: машинный индекс карты проекта\n{DIR_NAME}/\n"
    ));
    ko_json_store::write_atomic(&exclude, &updated)
        .map_err(|e| format!("не дописать {}: {e}", exclude.display()))?;
    Ok(true)
}

/// Собрать индекс по каталогу, переиспользуя кеш для неизменившихся файлов.
///
/// `scan` передаётся снаружи, а не обходится здесь: обход дешёвый, но его
/// результат нужен ещё и для проверки свежести — см. [`refresh`]. Второй обход
/// того же дерева был бы чистой потерей времени.
pub fn build(root: &Path, scan: scan::Scan) -> Result<(Index, BuildStats), String> {
    // Старый индекс читается ОДИН раз. Чтение внутри цикла дало бы
    // O(файлов²) работы и на большом репозитории стало бы главным тормозом
    // всей сборки — при том, что весь смысл кеша в экономии этой работы.
    build_from_old(root, scan, load(root)?)
}

/// Собрать индекс без чтения кеша и без записи на диск.
///
/// Режим `--no-cache` для чужих деревьев (крейты cargo registry, соседние
/// репозитории): их индекс никто не хранит и хранить не должен — на чужом
/// корне запрещено создавать `.codemap/` и трогать его метаданные git.
/// Ответ агенту — отрендеренная карта, а не сохранённый файл, поэтому
/// результат считается в памяти и уходит в stdout напрямую.
pub fn build_without_cache(root: &Path, scan: scan::Scan) -> Result<(Index, BuildStats), String> {
    build_from_old(root, scan, None)
}

/// Тело сборки: что строить, если «старого индекса» задан явно.
///
/// Кеш здесь не читается (его принёс вызывающий) и не пишется: сохранение —
/// отдельное решение [`refresh`]/[`save`], а не побочный эффект сборки.
fn build_from_old(
    root: &Path,
    scan: scan::Scan,
    old: Option<Index>,
) -> Result<(Index, BuildStats), String> {
    let old_files: std::collections::HashMap<String, FileEntry> = old
        .as_ref()
        .map(|index| {
            index
                .files
                .iter()
                .map(|f| (f.rel_path.clone(), f.clone()))
                .collect()
        })
        .unwrap_or_default();
    let old_tags = tags_by_path(old.as_ref());
    let old_chunks = chunks_by_path(old.as_ref());

    let ignored = scan.ignored;
    let mut stats = BuildStats {
        scanned: scan.files.len(),
        ..BuildStats::default()
    };
    let mut files = Vec::with_capacity(scan.files.len());
    let mut tags = Vec::new();
    let mut chunks = Vec::new();

    for file in scan.files {
        if let Some(cached) = old_files.get(&file.rel_path) {
            if cached.mtime_secs == file.mtime_secs {
                stats.reused += 1;
                files.push(cached.clone());
                tags.extend(old_tags.get(&file.rel_path).cloned().unwrap_or_default());
                chunks.extend(old_chunks.get(&file.rel_path).cloned().unwrap_or_default());
                continue;
            }
        }

        // Файл новый или изменился — читаем и разбираем. Здесь и только здесь
        // содержимое попадает в память: неизменившиеся файлы до этого места не
        // доходят вовсе.
        let source = match super::text::read_content(&root.join(&file.rel_path)) {
            super::text::Content::Text(source) => source,
            super::text::Content::Binary => {
                // Двоичный файл не ошибка и не потеря: у него нет символов,
                // которые можно было бы найти. Он просто вне области
                // инструмента, и это должно быть видно в счётчике.
                stats.binary += 1;
                continue;
            }
            super::text::Content::Unreadable(reason) => {
                return Err(format!("не прочитать {}: {reason}", file.rel_path));
            }
        };
        // Размерность считается здесь же: обход по метаданным её не знает,
        // а для неизменившегося файла она пришла из кеша вместе с тегами.
        let file = file.with_content(&source);

        match super::parse::tags_of(&file.rel_path, &source, file.lang) {
            Ok(collected) if !collected.is_empty() => {
                tags.extend(collected);
                // Markdown отдаёт в тегах заголовки — структуру, — но тело
                // документа остаётся прозой: полнотекстовый поиск по нему
                // работает через плоские чанки, как и раньше. Язык и путь
                // читаются до перемещения `file` в список.
                let keeps_text_chunks = file.lang == Lang::Markdown;
                let rel_path = file.rel_path.clone();
                files.push(file.with_parsed(true));
                if keeps_text_chunks {
                    chunks.extend(super::flat_lang::chunks(&rel_path, &source));
                }
            }
            Ok(_) => {
                // Парсера нет либо файл не содержит символов. Он не выпадает,
                // а попадает в плоский индекс и остаётся доступным поиску.
                chunks.extend(super::flat_lang::chunks(&file.rel_path, &source));
                files.push(file.with_parsed(false));
            }
            Err(reason) => {
                // Причина обязана попасть в лог: молча превращённая в плоский
                // карта выглядела бы так, будто в файле нет символов.
                log::warn!("codemap: {reason}");
                chunks.extend(super::flat_lang::chunks(&file.rel_path, &source));
                files.push(file.with_parsed(false));
                stats.failed += 1;
            }
        }
        stats.parsed += 1;
    }

    files.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    tags.sort_by(|a, b| (&a.rel_path, a.line).cmp(&(&b.rel_path, b.line)));
    Ok((
        Index {
            version: FORMAT_VERSION,
            root: root.display().to_string(),
            built_at: super::clock::now(),
            files,
            tags,
            chunks,
            ignored,
        },
        stats,
    ))
}

/// Вернуть индекс, актуальный для текущего состояния каталога.
///
/// Это то, чем пользуются `map`, `search` и `callers` — всё, что читает индекс
/// и обязано видеть реальный проект. Разница с [`build`] в одном действии:
/// индекс сначала **сверяется с диском** и пересобирается, только если
/// расходится.
///
/// Почему сверка нужна и почему она дешёвая: обход берёт только метаданные
/// (см. [`super::scan`]), поэтому проверка «те же файлы, те же даты» стоит
/// одного `read_dir` с `stat` на файл. Без неё индекс, попавший на диск, живёт
/// своей жизнью: удалённый файл навсегда остаётся в карте (и при каждом вызове
/// пишет предупреждение «не удаётся найти файл»), а новый в неё не попадает,
/// пока кто-то вручную не запустит `codemap index`.
pub fn refresh(root: &Path) -> Result<Index, String> {
    let scan = scan::walk(root)?;
    match load(root)? {
        Some(index) if is_fresh(&index, &scan.files) => Ok(index),
        _ => {
            let (index, _) = build(root, scan)?;
            save(root, &index)?;
            Ok(index)
        }
    }
}

/// Вернуть индекс, посчитанный целиком в памяти, не трогая диск для записи.
///
/// `--no-cache`-режим чтения карты: полный scan+parse при каждом вызове, зато
/// ни каталога `.codemap`, ни правок `.git/info/exclude` на чужом корне.
/// Наборы счётчиков те же, что у [`build`], поэтому логи честно показывают,
/// сколько файлов пришлось разобрать заново.
pub fn compute_only(root: &Path) -> Result<Index, String> {
    let scan = scan::walk(root)?;
    let started = std::time::Instant::now();
    let (index, stats) = build_without_cache(root, scan)?;
    log::info!(
        "codemap (no-cache): scanned {} | parsed {} | binary {} | failed {} | {} мс | на диске ничего не записано",
        stats.scanned,
        stats.parsed,
        stats.binary,
        stats.failed,
        started.elapsed().as_millis()
    );
    Ok(index)
}

/// Совпадает ли индекс с тем, что лежит на диске.
///
/// Сравниваются множество путей и `mtime` каждого файла. Одного числа файлов
/// мало: файл мог быть удалён, а другой добавлен, и количество бы совпало —
/// проверка по `mtime` такой случай ловит, потому что нового пути в кеше нет.
fn is_fresh(index: &Index, scanned: &[FileEntry]) -> bool {
    if index.files.len() != scanned.len() {
        return false;
    }
    let cached: std::collections::HashMap<&str, u64> = index
        .files
        .iter()
        .map(|f| (f.rel_path.as_str(), f.mtime_secs))
        .collect();
    scanned
        .iter()
        .all(|f| cached.get(f.rel_path.as_str()) == Some(&f.mtime_secs))
}

/// Теги старого индекса, сгруппированные по файлу.
fn tags_by_path(index: Option<&Index>) -> std::collections::HashMap<String, Vec<Tag>> {
    let mut map: std::collections::HashMap<String, Vec<Tag>> = std::collections::HashMap::new();
    if let Some(index) = index {
        for tag in &index.tags {
            map.entry(tag.rel_path.clone()).or_default().push(tag.clone());
        }
    }
    map
}

/// Чанки старого индекса, сгруппированные по файлу.
fn chunks_by_path(
    index: Option<&Index>,
) -> std::collections::HashMap<String, Vec<FlatChunk>> {
    let mut map: std::collections::HashMap<String, Vec<FlatChunk>> = std::collections::HashMap::new();
    if let Some(index) = index {
        for chunk in &index.chunks {
            map.entry(chunk.rel_path.clone()).or_default().push(chunk.clone());
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("codemap_store_{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("создать временный каталог");
        dir
    }

    fn sample_index() -> Index {
        Index {
            version: FORMAT_VERSION,
            root: "repo".into(),
            built_at: "2026-10-15 00:00:00".into(),
            files: Vec::new(),
            tags: Vec::new(),
            chunks: Vec::new(),
            ignored: Vec::new(),
        }
    }

    /// ГЕЙТ 6, часть 1: индекс переживает перезапуск процесса.
    #[test]
    fn saved_index_is_read_back() {
        let root = temp_root("roundtrip");
        let index = sample_index();
        save(&root, &index).expect("сохранить");
        let loaded = load(&root).expect("загрузить").expect("индекс обязан существовать");
        assert_eq!(loaded.version, index.version);
        assert_eq!(loaded.root, index.root);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_index_is_absent_not_broken() {
        let root = temp_root("missing");
        assert!(
            load(&root).expect("нет файла — не ошибка").is_none(),
            "отсутствие индекса обязано отличаться от повреждения"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn index_of_another_format_version_is_discarded() {
        // Иначе карта молча считалась бы по старым правилам ранжирования.
        let root = temp_root("version");
        let mut stale = sample_index();
        stale.version = FORMAT_VERSION + 1;
        save(&root, &stale).expect("сохранить");
        assert!(
            load(&root).expect("прочитать").is_none(),
            "индекс иной версии обязан быть отброшен, а не прочитан"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// Индекс прошлой версии на диске не обязан быть «повреждённым»: решение о
    /// годности принимает номер версии, поэтому старый файл обязан читаться и
    /// отбрасываться, а не ронять инструмент требованием удалить его руками.
    #[test]
    fn older_index_without_the_field_is_discarded_not_reported_as_corrupt() {
        let root = temp_root("old_shape");
        let index = sample_index();
        let mut value = serde_json::to_value(&index).expect("в json");
        value
            .as_object_mut()
            .expect("объект")
            .remove("ignored");
        value["version"] = serde_json::json!(FORMAT_VERSION - 1);
        fs::create_dir_all(index_dir(&root)).expect("каталог");
        fs::write(
            index_path(&root),
            serde_json::to_string(&value).expect("из json"),
        )
        .expect("записать");
        assert!(
            load(&root).expect("старый индекс обязан читаться, а не падать").is_none(),
            "индекс прошлой версии обязан быть отброшен по номеру версии"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn corrupt_index_is_an_error_not_silence() {
        let root = temp_root("corrupt");
        fs::create_dir_all(index_dir(&root)).expect("каталог");
        fs::write(index_path(&root), "{ не json").expect("испорченный индекс");
        let err = load(&root).expect_err("повреждённый индекс обязан быть ошибкой");
        assert!(err.contains("index.json"), "ошибка обязана называть файл: {err}");
        let _ = fs::remove_dir_all(&root);
    }

    /// Каталог индекса не должен появляться в `git status` чужого проекта.
    #[test]
    fn cache_dir_is_excluded_from_git_once_and_idempotently() {
        let root = temp_root("git");
        fs::create_dir_all(root.join(".git")).expect("имитация репозитория");

        assert_eq!(exclude_from_git(&root).expect("первая запись"), true);
        let after_first = fs::read_to_string(root.join(".git/info/exclude")).expect("exclude");

        assert_eq!(exclude_from_git(&root).expect("повторная запись"), false);
        let after_second = fs::read_to_string(root.join(".git/info/exclude")).expect("exclude");

        assert_eq!(after_first, after_second, "правило обязано дописываться один раз");
        assert!(after_first.contains(".codemap/"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn non_git_directory_is_not_an_error() {
        let root = temp_root("nogit");
        assert_eq!(exclude_from_git(&root).expect("не git"), false);
        let _ = fs::remove_dir_all(&root);
    }

    /// Проекция на настоящий сценарий: файл удалили, а карта продолжала
    /// показывать его — вместе с предупреждением «не удаётся найти файл».
    /// Причина была в том, что `map` брал индекс с диска и не сверял его.
    #[test]
    fn refresh_drops_a_file_that_was_deleted_from_disk() {
        let root = temp_root("deleted");
        fs::create_dir_all(root.join("src")).expect("каталог");
        fs::write(root.join("src/keep.rs"), "pub fn keep() -> u32 { 1 }\n").expect("файл");
        fs::write(root.join("src/gone.rs"), "pub fn gone() -> u32 { 2 }\n").expect("файл");

        let built = refresh(&root).expect("первый refresh");
        assert!(built.files.iter().any(|f| f.rel_path == "src/gone.rs"));

        fs::remove_file(root.join("src/gone.rs")).expect("удалить файл");
        let healed = refresh(&root).expect("второй refresh");
        assert!(
            !healed.files.iter().any(|f| f.rel_path == "src/gone.rs"),
            "удалённый файл обязан исчезнуть из индекса без ручного `codemap index`: {:?}",
            healed.files.iter().map(|f| &f.rel_path).collect::<Vec<_>>()
        );
        assert!(healed.tags.iter().all(|t| t.rel_path != "src/gone.rs"));
        let _ = fs::remove_dir_all(&root);
    }

    /// Обратная сторона того же: новый файл обязан появляться сам.
    #[test]
    fn refresh_picks_up_a_file_created_after_the_index_was_built() {
        let root = temp_root("created");
        fs::create_dir_all(root.join("src")).expect("каталог");
        fs::write(root.join("src/first.rs"), "pub fn first() -> u32 { 1 }\n").expect("файл");
        refresh(&root).expect("первый refresh");

        fs::write(root.join("src/second.rs"), "pub fn second() -> u32 { 2 }\n").expect("файл");
        let healed = refresh(&root).expect("второй refresh");
        assert!(
            healed.files.iter().any(|f| f.rel_path == "src/second.rs"),
            "новый файл обязан попасть в индекс без ручного `codemap index`"
        );
        assert!(
            healed.tags.iter().any(|t| t.rel_path == "src/second.rs" && t.name == "second"),
            "символы нового файла обязаны быть в индексе"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// Изменённый файл перечитывается, а не остаётся с прежними символами.
    #[test]
    fn refresh_reparses_a_file_whose_mtime_changed() {
        let root = temp_root("changed");
        let path = root.join("src/a.rs");
        fs::create_dir_all(root.join("src")).expect("каталог");
        fs::write(&path, "pub fn alpha() -> u32 { 1 }\n").expect("файл");
        let first = refresh(&root).expect("первый refresh");
        assert!(first.tags.iter().any(|t| t.name == "alpha"));
        assert!(first.tags.iter().all(|t| t.name != "beta"));

        // Спим секунду: иначе `mtime` новой записи совпадёт с прежним и файл
        // будет справедливо признан неизменившимся.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        fs::write(&path, "pub fn beta() -> u32 { 2 }\n").expect("файл");
        let healed = refresh(&root).expect("второй refresh");
        assert!(
            healed.tags.iter().any(|t| t.name == "beta"),
            "изменённый файл обязан быть перечитан"
        );
        assert!(
            healed.tags.iter().all(|t| t.name != "alpha"),
            "прежние символы изменённого файла обязаны исчезнуть: {:?}",
            healed.tags
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// Неизменившийся проект не должен перечитываться: ради этого обход и
    /// отделён от чтения содержимого.
    #[test]
    fn refresh_reuses_the_index_untouched_when_nothing_changed() {
        let root = temp_root("stable");
        fs::create_dir_all(root.join("src")).expect("каталог");
        fs::write(root.join("src/a.rs"), "pub fn alpha() -> u32 { 1 }\n").expect("файл");
        refresh(&root).expect("первый refresh");

        let scan = scan::walk(&root).expect("обход");
        let (rebuilt, stats) = build(&root, scan).expect("пересборка");
        assert_eq!(stats.reused, 1, "неизменившийся файл обязан прийти из кеша");
        assert_eq!(stats.parsed, 0, "перечитывать неизменившееся — значит читать впустую");
        assert!(rebuilt.tags.iter().any(|t| t.name == "alpha"));
        let _ = fs::remove_dir_all(&root);
    }

    /// Режим `--no-cache`: ответ считается в памяти, а диск остаётся нетронутым
    /// — ни каталога кеша, ни правок exclude в `.git` чужого корня.
    #[test]
    fn compute_only_answers_without_touching_the_disk() {
        let root = temp_root("compute_only");
        fs::create_dir_all(root.join(".git")).expect("имитация чужого репозитория");
        fs::create_dir_all(root.join("src")).expect("каталог");
        fs::write(root.join("src/main.rs"), "pub fn main_entry() -> u32 { 0 }\n").expect("файл");

        let index = compute_only(&root).expect("вычислить в памяти");
        assert!(
            index.tags.iter().any(|t| t.name == "main_entry"),
            "символы чужого дерева обязаны быть найдены"
        );
        assert_eq!(index.files.len(), 1);
        assert!(
            !index_dir(&root).exists(),
            "no-cache режим обязан не создавать каталог кеша"
        );
        assert!(
            !root.join(".git/info/exclude").exists(),
            "no-cache режим обязан не трогать .git/info/exclude чужого корня"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// Сборка без кеша обязана разобрать ВСЁ: кеша нет, «переиспользовать»
    /// нечего — иначе карта выдала бы пустоту молча.
    #[test]
    fn build_without_cache_parses_everything_anew() {
        let root = temp_root("no_cache_full");
        fs::create_dir_all(root.join("src")).expect("каталог");
        fs::write(root.join("src/a.rs"), "pub fn alpha() -> u32 { 1 }\n").expect("файл");

        let scan = scan::walk(&root).expect("обход");
        let (index, stats) = build_without_cache(&root, scan).expect("сборка без кеша");
        assert_eq!(stats.scanned, 1);
        assert_eq!(stats.parsed, 1, "без кеша файл обязан быть разобран заново");
        assert_eq!(stats.reused, 0, "переиспользовать нечего");
        assert!(index.tags.iter().any(|t| t.name == "alpha"));
        let _ = fs::remove_dir_all(&root);
    }

    /// Markdown разбирается заголовками И остаётся в полнотекстовых чанках:
    /// структура идёт в граф и карту, тело — в поиск. Потерять любую из
    /// двух половин значило бы незаметно обеднить индекс.
    #[test]
    fn markdown_file_gets_both_headings_and_text_chunks() {
        let root = temp_root("markdown");
        fs::write(
            root.join("ARCHITECTURE.md"),
            "# Архитектура\n\nТут описаны слои системы и правила проекта.\n\n## Слой домена\n\nДомен не зависит от инфраструктуры.\n",
        )
        .expect("документ");

        let (index, _) = build(&root, scan::walk(&root).expect("обход")).expect("сборка");
        let file = index
            .files
            .iter()
            .find(|f| f.rel_path == "ARCHITECTURE.md")
            .expect("документ обязан быть в индексе");
        assert!(file.parsed, "markdown имеет структуру — он разобран");

        assert!(
            index.tags.iter().any(|t| t.rel_path == "ARCHITECTURE.md" && t.name == "Архитектура"),
            "заголовок обязан стать определением: {:?}",
            index.tags
        );
        assert!(
            index
                .chunks
                .iter()
                .any(|c| c.rel_path == "ARCHITECTURE.md" && c.text.contains("не зависит от инфраструктуры")),
            "тело документа обязано остаться в полнотекстовом поиске: {:?}",
            index.chunks
        );
    }
}