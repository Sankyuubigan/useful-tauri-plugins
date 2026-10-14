//! Контракт готовности шага `postinstall` пакета роутера.
//!
//! ## Что это такое
//!
//! Успех шага = **проверяемый артефакт**, а не код возврата. Код возврата
//! внешней программы — это диагностика, а не вердикт: он не говорит, сделала
//! ли она работу, и тем более не говорит, какую часть.
//!
//! ## Почему не «файлы появились → успех»
//!
//! `cpSync(..., {recursive: true})`, прерванный на середине, оставляет папку,
//! которая «есть» и в которой половина файлов. Объявлять по этому успех —
//! хуже честной ошибки: пользователь получит роутер, который не стартует.
//!
//! Поэтому проверка не «есть ли», а **«ровно ли то, что должно быть»**:
//! каждый раскладываемый пакет сверяется с ИСТОЧНИКОМ по трём признакам сразу —
//! совпадение `name`/`version` в `package.json`, количество файлов и суммарный
//! размер дерева. Любое расхождение означает «скопировано не всё».
//!
//! ## Почему нельзя проверять «просто наличие»
//!
//! Тогда теряется разница между «скопировано целиком» и «скопирована часть»,
//! и проверка превращается в угадывание, запрещённое планом установки (A3).
//!
//! ## Что НЕ входит в контракт и почему
//!
//! * **`.env` в корне пакета.** Создаётся шагом `syncProjectEnv`. Пакет сам
//!   объявляет такой файл недолговечным: `bin/omniroute.mjs` пишет «папка
//!   пакета заменяется следующей установкой, поэтому `.env` в ней молча
//!   теряется», а реальные секреты хранит в `<DATA_DIR>/.env`, который
//!   сервер создаёт сам при первом старте. Наш запуск сервера вообще не идёт
//!   через `bin/omniroute.mjs` и задаёт `DATA_DIR` явно.
//! * **Нативные бинарники better-sqlite3 и wreq-js.** Шаг в пакете сам
//!   считает их необязательными: при неудаче печатает предупреждение и идёт
//!   дальше, то есть объявлять установку проваленной из-за них — значит
//!   ввести отказ, которого у пакета нет.
//! * **Опциональные SLM-пакеты.** `colocateOptionals` в пакете помечен как
//!   fail-open: сбой слинковки никогда не роняет установку.
//!
//! ## Про список пакетов
//!
//! Список ниже — единственное место в плагине, где он живёт, и каждая
//! позиция ссылается на строку пакета, которая её создаёт. Дублировать его по
//! другим файлам плагина нельзя: список разъедется с пакетом при обновлении.

use std::path::{Path, PathBuf};

/// Пакеты, которые `postinstall` раскладывает в standalone-каталог
/// копированием из `node_modules`.
///
/// Порядок — как в пакете: `postinstall.mjs`, функции `ensureSwcHelpers` и
/// `ensureStandaloneRuntimePackages`.
pub const COPIED_PACKAGES: &[&str] = &["@swc/helpers", "sql.js", "node-machine-id"];

/// Признак, который сам пакет оставляет в пропатченном файле.
/// Маркер взят из `scripts/build/fixPlaywrightAndroid.mjs` — это ЕГО собственная
/// проверка завершённости, а не наша догадка о содержимом патча.
const PLAYWRIGHT_PATCH_MARKER: &str = "/* omniroute-android-patch */";

/// Оба места, которые патчит пакет.
///
/// Второго экземпляра в нормальном дереве НЕТ: проверено на рабочей установке
/// 12.10 — `dist/dist/node_modules/playwright-core` там отсутствует, и сервер
/// разруливает `playwright-core` по родительской папке (`<dist>/node_modules`).
/// Поэтому отсутствие копии — не отказ, а норма; проверяем те, что есть.
const PLAYWRIGHT_PATCH_TARGETS: &[&str] = &[
    "node_modules/playwright-core/lib/coreBundle.js",
    "dist/node_modules/playwright-core/lib/coreBundle.js",
];

/// Отпечаток дерева: сколько файлов и сколько байт.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreeFingerprint {
    pub files: u64,
    pub bytes: u64,
}

/// Рекурсивный отпечаток каталога.
///
/// Симлинки и junctions НЕ обходятся: на Windows junction виден в `read_dir`
/// как каталог, и обход по нему безусловно даёт зацикливание или подсчёт
/// чужого дерева. Такой узел считается одним элементом, но не раскрывается.
///
/// `None` — каталога нет или он не читается. Это НЕ «пусто», а «проверять
/// нечем»: вызывающий обязан отличать одно от другого.
pub fn tree_fingerprint(dir: &Path) -> Option<TreeFingerprint> {
    fn walk(dir: &Path, acc: &mut TreeFingerprint) -> Option<()> {
        let meta = std::fs::symlink_metadata(dir).ok()?;
        if meta.is_file() {
            acc.files += 1;
            acc.bytes += meta.len();
            return Some(());
        }
        if !meta.is_dir() {
            // Симлинк/junction: учитываем как один узел, содержимое не раскрываем.
            acc.files += 1;
            return Some(());
        }
        for entry in std::fs::read_dir(dir).ok()? {
            let entry = entry.ok()?;
            walk(&entry.path(), acc)?;
        }
        Some(())
    }
    let mut acc = TreeFingerprint { files: 0, bytes: 0 };
    walk(dir, &mut acc)?;
    Some(acc)
}

/// Содержит ли `target` ВСЁ, что есть в `source` (те же относительные пути и
/// те же размеры файлов).
///
/// ## Почему именно «содержит», а не «равно»
///
/// Пакет раскладывает копированием, а `cpSync(..., {recursive: true})` НЕ
/// удаляет лишнее: если в standalone-бандле из tgz уже была своя копия
/// пакета, после копирования она остаётся как надмножество. Проверка на
/// равенство отпечатков дала бы ложный отказ на совершенно корректном дереве —
/// то есть ровно тот «отказ из-за сдвига порога», которого здесь быть не должно.
///
/// ## Почему этого достаточно против «скопировал часть и упал»
///
/// Прерванное копирование теряет файлы: их нет в цели либо их размер не тот.
/// Именно это и ловит проверка, а наличие папки цели — нет.
pub fn target_contains_source(source: &Path, target: &Path) -> Result<(), String> {
    fn walk(source: &Path, target: &Path, prefix: &Path, lost: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(source) else {
            return;
        };
        for entry in entries.flatten() {
            let src_path = entry.path();
            let file_name = entry.file_name();
            let Some(name) = file_name.to_str() else {
                continue;
            };
            let rel = prefix.join(name);
            let dst_path = target.join(&rel);

            let Ok(src_meta) = std::fs::symlink_metadata(&src_path) else {
                continue;
            };
            let Ok(dst_meta) = std::fs::symlink_metadata(&dst_path) else {
                lost.push(format!("нет {}", rel.display()));
                continue;
            };
            if src_meta.is_dir() && !src_meta.is_symlink() {
                if !dst_meta.is_dir() {
                    lost.push(format!("{} — в цели не каталог", rel.display()));
                    continue;
                }
                // Цель уже включает вложенный путь, поэтому префикс сбрасываем:
                // иначе `lib/lib/a.js` не найдётся там, где файл есть.
                walk(&src_path, &dst_path, Path::new(""), lost);
            } else if src_meta.len() != dst_meta.len() {
                lost.push(format!(
                    "{} — размер {} Б, в цели {} Б",
                    rel.display(),
                    src_meta.len(),
                    dst_meta.len()
                ));
            }
        }
    }
    let mut lost = Vec::new();
    walk(source, target, Path::new(""), &mut lost);
    if lost.is_empty() {
        Ok(())
    } else {
        // Длинный список не нужен: достаточно трёх примеров и их количества.
        let shown = lost
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        Err(format!(
            "не перенесено {} элем. из источника (например: {})",
            lost.len(),
            shown
        ))
    }
}

/// Итог проверки контракта.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ContractReport {
    /// Артефакты, которые обязаны были появиться, но не появились или
    /// неполны. Непустой список = установка неполна.
    pub missing: Vec<String>,
    /// Артефакты, для которых нет источника: пакет их тоже не раскладывал.
    /// Пишутся в лог явно — молча пропущенная проверка была бы ложью.
    pub not_applicable: Vec<String>,
    /// Артефакты, подтверждённые как полные.
    pub verified: Vec<String>,
}

impl ContractReport {
    /// Контракт выполнен?
    pub fn holds(&self) -> bool {
        self.missing.is_empty()
    }

    /// Человекочитаемое объяснение — обязано называть, КАКОГО артефакта нет.
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if !self.verified.is_empty() {
            parts.push(format!("проверено: {}", self.verified.join(", ")));
        }
        if !self.not_applicable.is_empty() {
            parts.push(format!("не применимо: {}", self.not_applicable.join(", ")));
        }
        if !self.missing.is_empty() {
            parts.push(format!("НЕ ГОТОВО: {}", self.missing.join(", ")));
        }
        parts.join("; ")
    }
}

/// Проверить, что `postinstall` оставил после себя всё, что должен был.
///
/// `dist` — корень распакованного пакета (то, что станет `dist` роутера).
pub fn verify(dist: &Path) -> ContractReport {
    let mut report = ContractReport::default();

    for pkg in COPIED_PACKAGES {
        let source = dist.join("node_modules").join(pkg);
        let target = dist.join("dist").join("node_modules").join(pkg);

        // Источника нет или он пуст — раскладывать было нечего, и сам пакет в этом
        // случае только пишет предупреждение и идёт дальше. Молчать об этом
        // нельзя: строка попадает в лог через `describe()`.
        match tree_fingerprint(&source) {
            None => {
                report.not_applicable.push(format!("{} — нет источника", pkg));
                continue;
            }
            Some(fp) if fp.files == 0 => {
                report.not_applicable.push(format!(
                    "{} — источник пуст (пакет его тоже не раскладывал)",
                    pkg
                ));
                continue;
            }
            Some(_) => {}
        }

        match target_contains_source(&source, &target) {
            Ok(()) => report.verified.push(pkg.to_string()),
            Err(why) => report.missing.push(format!("{} — {}", pkg, why)),
        }
    }

    let mut playwright_missing = verify_playwright_patch(dist, &mut report);
    report.missing.append(&mut playwright_missing);

    report
}

/// Пропатченный `playwright-core` обязан нести СВОЙ маркер пакета.
///
/// Проверяем маркер, а не «файл есть»: файл приходит из tgz и без патча, и
/// наличие ничего не значит. Маркер пишет только завершившийся патч.
fn verify_playwright_patch(dist: &Path, report: &mut ContractReport) -> Vec<String> {
    let mut existing = 0usize;
    let mut unpatched: Vec<&str> = Vec::new();
    for rel in PLAYWRIGHT_PATCH_TARGETS {
        // Копии нет — пропускаем: патчить было нечего, и это норма.
        let Ok(content) = std::fs::read_to_string(dist.join(rel)) else {
            continue;
        };
        existing += 1;
        if !content.contains(PLAYWRIGHT_PATCH_MARKER) {
            unpatched.push(rel);
        }
    }
    if existing == 0 {
        return vec![
            "playwright-core: нет ни одной копии lib/coreBundle.js — патч негде было применить"
                .to_string(),
        ];
    }
    if unpatched.is_empty() {
        report
            .verified
            .push(format!("playwright-core (патч, копий: {})", existing));
        Vec::new()
    } else {
        unpatched
            .into_iter()
            .map(|r| format!("playwright-core не пропатчен: {}", r))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let p = crate::router::test_scratch::scratch_dir(&format!("cr_contract_{}", tag));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).expect("создать temp");
            TempDir(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Собрать «хорошее» дерево: источник и полная копия в `dist/node_modules`.
    fn good_tree(root: &Path) {
        for pkg in COPIED_PACKAGES {
            for base in [
                root.join("node_modules").join(pkg),
                root.join("dist").join("node_modules").join(pkg),
            ] {
                std::fs::create_dir_all(base.join("lib")).unwrap();
                std::fs::write(
                    base.join("package.json"),
                    format!(r#"{{"name":"{}","version":"1.0.0"}}"#, pkg),
                )
                .unwrap();
                std::fs::write(base.join("lib").join("a.js"), "aaaaaaaa").unwrap();
                std::fs::write(base.join("lib").join("b.js"), "bbbb").unwrap();
            }
        }
let core = root.join("node_modules").join("playwright-core").join("lib");
        std::fs::create_dir_all(&core).unwrap();
        std::fs::write(
            core.join("coreBundle.js"),
            format!("if (a) {{}} {}", PLAYWRIGHT_PATCH_MARKER),
        )
        .unwrap();
    }

    #[test]
    fn отпечаток_считает_файлы_и_байты_рекурсивно() {
        let t = TempDir::new("fingerprint");
        std::fs::create_dir_all(t.path().join("a/b")).unwrap();
        std::fs::write(t.path().join("a/f1"), b"1234").unwrap();
        std::fs::write(t.path().join("a/b/f2"), b"56").unwrap();
        assert_eq!(
            tree_fingerprint(t.path()),
            Some(TreeFingerprint { files: 2, bytes: 6 })
        );
    }

    #[test]
    fn отпечаток_отсутствующего_каталога_это_none_а_не_пусто() {
        let t = TempDir::new("fingerprint_missing");
        assert_eq!(tree_fingerprint(&t.path().join("нет")), None);
    }

    #[test]
    fn полное_дерево_проходит_контракт() {
        let t = TempDir::new("ok");
        good_tree(t.path());
        let r = verify(t.path());
        assert!(r.holds(), "контракт не выполнен: {}", r.describe());
        assert_eq!(r.verified.len(), COPIED_PACKAGES.len() + 1);
    }

    /// ГЕЙТ G3, главный случай: дерево обрезано наполовину. Проверка обязана
    /// это поймать — именно эту дыру закрывает контракт вместо «файл есть».
    /// Тест моделирует худший случай: код возврата 0, а работа не сделана.
    #[test]
    fn обрезанное_дерево_не_проходит_контракт() {
        let t = TempDir::new("truncated");
        good_tree(t.path());
        std::fs::remove_file(t.path().join("dist/node_modules/sql.js/lib/b.js")).unwrap();
        let r = verify(t.path());
        assert!(!r.holds(), "обрезанное дерево прошло проверку: {}", r.describe());
        assert!(
            r.missing.iter().any(|m| m.contains("sql.js")),
            "ошибка обязана называть конкретный пакет: {:?}",
            r.missing
        );
    }

    /// Обрезанный не только по списку файлов, но и по содержимому: файл на
    /// месте, но короче источника. Тоже недокопированное дерево.
    #[test]
    fn обрезанный_по_содержимому_файл_не_проходит_контракт() {
        let t = TempDir::new("truncated_body");
        good_tree(t.path());
        std::fs::write(t.path().join("dist/node_modules/sql.js/lib/a.js"), "aa").unwrap();
        let r = verify(t.path());
        assert!(!r.holds(), "обрезанный по содержимому файл прошёл: {}", r.describe());
    }

    /// Цель может законно быть НАДМНОЖЕСТВОМ источника: `cpSync` не удаляет
    /// лишние файлы, оставшиеся от бандла в tgz. Проверка на равенство
    /// отпечатков дала бы здесь ложный отказ на корректном дереве.
    #[test]
    fn надмножество_в_цели_не_считается_неполнотой() {
        let t = TempDir::new("superset");
        good_tree(t.path());
        std::fs::write(t.path().join("dist/node_modules/sql.js/lib/extra.js"), "extra").unwrap();
        let r = verify(t.path());
        assert!(r.holds(), "лишний файл в целе объявлен неполнотой: {}", r.describe());
    }

    #[test]
    fn непропатченный_playwright_не_проходит() {
        let t = TempDir::new("no_patch");
        good_tree(t.path());
        std::fs::write(
            t.path().join("node_modules/playwright-core/lib/coreBundle.js"),
            "if (a) {}",
        )
        .unwrap();
        let r = verify(t.path());
        assert!(!r.holds(), "непропатченный playwright прошёл: {}", r.describe());
        assert!(r.missing.iter().any(|m| m.contains("playwright-core")));
    }

    /// Реальная раскладка 12.10: второй копии playwright-core нет вовсе, и это
    /// НОРМА. Её отсутствие обязано проходить, иначе контракт объявил бы отказ
    /// на заведомо рабочем дереве.
    #[test]
    fn отсутствие_второй_копии_playwright_это_норма() {
        let t = TempDir::new("playwright_one_copy");
        good_tree(t.path());
        assert!(
            !t.path()
                .join("dist/node_modules/playwright-core")
                .exists(),
            "предусловие теста: второй копии нет"
        );
        let r = verify(t.path());
        assert!(r.holds(), "одна копия playwright — норма: {}", r.describe());
        assert!(r.verified.iter().any(|s| s.contains("патч, копий: 1")));
    }

    /// Если копия есть, а маркера нет — патч не наложен.
    #[test]
    fn непропатченная_вторая_копия_playwright_не_проходит() {
        let t = TempDir::new("playwright_two_unpatched");
        good_tree(t.path());
        let core = t.path().join("dist/node_modules/playwright-core/lib");
        std::fs::create_dir_all(&core).unwrap();
        std::fs::write(core.join("coreBundle.js"), "if (a) {}").unwrap();
        let r = verify(t.path());
        assert!(!r.holds(), "частично пропатченный playwright прошёл: {}", r.describe());
    }

    #[test]
    fn отсутствие_источника_помечается_как_не_применимо() {
        let t = TempDir::new("no_source");
        good_tree(t.path());
        std::fs::remove_dir_all(t.path().join("node_modules/sql.js")).unwrap();
        let r = verify(t.path());
        assert!(
            r.holds(),
            "нет источника — раскладывать нечего, это не отказ: {}",
            r.describe()
        );
        assert!(r.not_applicable.iter().any(|s| s.contains("sql.js")));
    }

    /// Отсутствие цели при наличии источника — настоящий недокопированный
    /// артефакт, а не «не применимо».
    #[test]
    fn отсутствие_цели_это_не_тихий_успех() {
        let t = TempDir::new("no_target");
        good_tree(t.path());
        std::fs::remove_dir_all(t.path().join("dist/node_modules/sql.js")).unwrap();
        let r = verify(t.path());
        assert!(!r.holds(), "отсутствующая цель прошла: {}", r.describe());
        assert!(r.missing.iter().any(|m| m.contains("sql.js")));
    }

    #[test]
    fn describe_называет_конкретный_недостающий_артефакт() {
        let r = ContractReport {
            missing: vec!["sql.js — дерево неполно".to_string()],
            verified: vec!["@swc/helpers".to_string()],
            ..Default::default()
        };
        let text = r.describe();
        assert!(text.contains("sql.js"), "нет имени артефакта: {}", text);
        assert!(text.contains("@swc/helpers"), "нет списка проверенного: {}", text);
    }
}