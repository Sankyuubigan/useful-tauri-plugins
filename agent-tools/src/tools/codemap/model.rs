//! Данные индекса: что о нём знать и как он сериализуется.
//!
//! Отдельный модуль, потому что формат индекса — это контракт между
//! [`super::store`] (запись на диск) и всеми, кто его читает: картой, поиском и
//! MCP-слоем. Если бы формат жил в модуле записи, его правила разъехались бы
//! по читателям, а это ровно тот случай, где потом появляются два «истиначатых»
//! формата индекса.

use serde::{Deserialize, Serialize};

use super::flat_lang::FlatChunk;
use super::lang::Lang;
use super::tags::Tag;

/// Версия формата индекса.
///
/// Поле не декоративное: при изменении правил ранжирования старый индекс молча
/// стал бы считаться по старым правилам и выдавал бы правдоподобную, но
/// устаревшую карту. Номер версии заставляет перестроить индекс вместо этого.
///
/// Версия отвечает не только за форму JSON, но и за **правила извлечения**:
/// кеш держит готовые теги и освежает их по `mtime` файла, поэтому смена
/// правил разбора без поднятия версии оставила бы на диске индекс, собранный
/// по старым правилам, — и он выглядел бы совершенно рабочим.
///
/// - **2** — добавлено поле [`Index::ignored`]: сводка каталогов, которые обход
///   не раскрывал.
/// - **3** — импорты (`use` в Rust, `import` в Python) начали давать ссылки;
///   до этого кросс-модульных зависимостей в графе не было вовсе, и старый
///   индекс с ними был бы не просто устаревшим, а пустым по смыслу.
pub const FORMAT_VERSION: u32 = 3;

/// Почему каталог не попал в индекс.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IgnoreReason {
    /// Каталог артефактов сборки или зависимостей (`target`, `node_modules`, …).
    ///
    /// Отдельный вариант, а не «gitignore»: решение принял сам инструмент, и
    /// агент должен отличать «здесь нет исходников» от «пользователь попросил
    /// не смотреть».
    Build,
    /// Каталог исключён правилом `.gitignore` корня.
    GitIgnore,
}

/// Каталог, который обход увидел и не раскрыл.
///
/// Хранится в индексе, а не пересчитывается при рендере: единственный проход
/// по дереву — это [`super::scan`], и второй обход ради сводки удвоил бы время
/// построения индекса.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IgnoredDir {
    /// Путь относительно корня, со `/` вместо `\`.
    pub path: String,
    /// Почему не раскрыт; см. [`IgnoreReason`].
    pub reason: IgnoreReason,
}

/// Одна проиндексированная книга проекта.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileEntry {
    /// Путь относительно корня индекса, со `/` вместо `\`.
    pub rel_path: String,
    /// На каком бэкенде файл разбирался.
    pub lang: Lang,
    /// Размер в байтах.
    pub bytes: u64,
    /// Число строк.
    pub lines: u32,
    /// Оценка числа токенов.
    pub tokens: u32,
    /// Время последнего изменения файла — основа кеша.
    ///
    /// Хранится рядом с разбором намеренно: без него пришлось бы заново
    /// открывать каждый файл, чтобы узнать, изменился ли он, и весь смысл
    /// инкрементальной пересборки исчез бы.
    pub mtime_secs: u64,
    /// Разобран ли файл AST-парсером.
    ///
    /// `false` означает, что файл попал в плоский индекс: он учтён и доступен
    /// поиску, но символов из него не извлечено. Поле хранится явно, чтобы
    /// отличать «файл без символов» от «файл выпал».
    pub parsed: bool,
}

impl FileEntry {
    /// Короткое имя для логов и вывода.
    pub fn name(&self) -> &str {
        &self.rel_path
    }

    /// Отметка результата разбора.
    ///
    /// Поле есть и в данных на диске: «файл без символов» и «файл не разобран»
    /// — это разные исходы, и по карте их надо различать, иначе агент решит
    /// будто бы символов нет ни там, ни там.
    pub fn with_parsed(mut self, parsed: bool) -> Self {
        self.parsed = parsed;
        self
    }
}

/// Полный индекс репозитория.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Index {
    /// Формат индекса; см. [`FORMAT_VERSION`].
    pub version: u32,
    /// Корень, относительно которого записаны пути.
    pub root: String,
    /// ISO-подобная метка времени сборки.
    pub built_at: String,
    /// Файлы по алфавиту.
    pub files: Vec<FileEntry>,
    /// Теги всех разобранных файлов: определения и ссылки.
    pub tags: Vec<Tag>,
    /// Чанки файлов без AST-парсера.
    pub chunks: Vec<FlatChunk>,
    /// Каталоги, которые обход не раскрыл; см. [`IgnoredDir`].
    ///
    /// `default` здесь обязателен и относится не к формату, а к порядку
    /// чтения: решение «годен ли индекс» принимает [`FORMAT_VERSION`], а
    /// десериализация идёт **раньше** него. Без `default` индекс прошлой версии
    /// падал бы с «повреждён», и инструмент требовал бы ручного удаления файла
    /// вместо того, чтобы построить его заново сам.
    #[serde(default)]
    pub ignored: Vec<IgnoredDir>,
}

impl Index {
    /// Сумма токенов по всем файлам — оценка «веса» проекта.
    pub fn total_tokens(&self) -> u64 {
        self.files.iter().map(|f| f.tokens as u64).sum()
    }

    /// Сколько файлов разобрано AST-парсером.
    pub fn parsed_count(&self) -> usize {
        self.files.iter().filter(|f| f.parsed).count()
    }

    /// Сколько файлов попало в плоский индекс.
    pub fn flat_count(&self) -> usize {
        self.files.iter().filter(|f| !f.parsed).count()
    }

    /// Каталоги, не раскрытые обходом, одной строкой через запятую.
    ///
    /// Пустая строка — не ошибка, а «обход ничего не пропустил»: вызывающий
    /// решает по факту, печатать ли сводку вообще.
    pub fn ignored_summary(&self) -> String {
        self.ignored
            .iter()
            .map(|dir| dir.path.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::codemap::tags::TagKind;

    fn sample() -> Index {
        Index {
            version: FORMAT_VERSION,
            root: "repo".to_string(),
            built_at: "2026-10-15 00:00:00".to_string(),
            files: vec![
                FileEntry {
                    rel_path: "a.rs".into(),
                    lang: Lang::Rust,
                    bytes: 100,
                    lines: 10,
                    tokens: 25,
                    mtime_secs: 1000,
                    parsed: true,
                },
                FileEntry {
                    rel_path: "b.md".into(),
                    lang: Lang::Flat,
                    bytes: 50,
                    lines: 5,
                    tokens: 12,
                    mtime_secs: 2000,
                    parsed: false,
                },
            ],
            tags: vec![Tag::def("a.rs", "alpha", 1), Tag::reference("a.rs", "alpha", 5)],
            chunks: vec![FlatChunk {
                rel_path: "b.md".into(),
                line: 1,
                text: "hi".into(),
            }],
            ignored: vec![IgnoredDir {
                path: "node_modules".into(),
                reason: IgnoreReason::Build,
            }],
        }
    }

    #[test]
    fn survives_a_json_roundtrip() {
        let index = sample();
        let json = serde_json::to_string(&index).expect("сериализация");
        let restored: Index = serde_json::from_str(&json).expect("десериализация");
        assert_eq!(restored.files, index.files);
        assert_eq!(restored.tags, index.tags);
        assert_eq!(restored.chunks, index.chunks);
        assert_eq!(restored.ignored, index.ignored);
        assert_eq!(restored.version, index.version);
    }

    #[test]
    fn ignored_summary_lists_paths_once_each() {
        assert_eq!(sample().ignored_summary(), "node_modules");
    }

    #[test]
    fn counts_separate_parsed_from_flat() {
        let index = sample();
        assert_eq!(index.parsed_count(), 1);
        assert_eq!(index.flat_count(), 1);
        assert_eq!(index.total_tokens(), 37);
    }

    #[test]
    fn tag_kind_survives_serialization() {
        let json = serde_json::to_string(&sample().tags).expect("сериализация");
        let restored: Vec<Tag> = serde_json::from_str(&json).expect("десериализация");
        assert_eq!(restored[0].kind, TagKind::Def);
        assert_eq!(restored[1].kind, TagKind::Ref);
    }
}