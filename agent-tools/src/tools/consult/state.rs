//! Состояние и лимиты инструмента `consult`.
//!
//! Файл состояния — `.consult_state.json` в рабочей директории (переопределяется
//! флагом `--state-file`):
//!
//! ```json
//! { "calls": 0, "limit": 5 }
//! ```
//!
//! Запись идёт только через `ko-json-store` (tmp → `sync_all` → `rename` +
//! sidecar-лок на `<файл>.lock`): счётчик нельзя потерять из-за падения
//! посреди записи, а два параллельных агента не могут израсходовать один и тот же
//! вызов — резервирование [`ConsultState::reserve`] атомарно.

use std::path::{Path, PathBuf};

use ko_json_store::{read_json, update_json, write_atomic};
use serde::{Deserialize, Serialize};

/// Имя файла состояния по умолчанию (в рабочей директории).
pub const STATE_FILE_NAME: &str = ".consult_state.json";

/// Лимит по умолчанию, если файла состояния нет.
pub const DEFAULT_LIMIT: u32 = 5;

/// Ошибка чтения/записи состояния — внутренняя ошибка инструмента (exit 4).
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("не удалось прочитать состояние {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("состояние {path} не разобрано: {reason}")]
    Parse { path: PathBuf, reason: String },
    #[error("не удалось записать состояние {path}: {reason}")]
    Write { path: PathBuf, reason: String },
}

/// Счётчик и лимит вызовов умной модели.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsultState {
    /// Израсходовано вызовов.
    pub calls: u32,
    /// Максимум вызовов (0 — вызовы запрещены полностью).
    pub limit: u32,
}

impl Default for ConsultState {
    fn default() -> Self {
        Self {
            calls: 0,
            limit: DEFAULT_LIMIT,
        }
    }
}

/// Результат резервирования вызова.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reservation {
    /// Вызов зарезервирован, `calls` уже увеличен; при сетевой ошибке вызов
    /// возвращается через [`ConsultState::refund`].
    Reserved(ConsultState),
    /// Лимит исчерпан — запрос выполнять нельзя. Состояние не менялось.
    Exhausted(ConsultState),
}

impl ConsultState {
    /// Путь к файлу состояния по умолчанию (текущая директория).
    pub fn default_path() -> PathBuf {
        PathBuf::from(STATE_FILE_NAME)
    }

    /// Лимит исчерпан: следующий вызов делать нельзя.
    pub fn is_exhausted(&self) -> bool {
        self.calls >= self.limit
    }

    /// Сколько вызовов ещё можно сделать (0, если лимит исчерпан или лимит 0).
    pub fn remaining(&self) -> u32 {
        self.limit.saturating_sub(self.calls)
    }

    /// Прочитать состояние. Отсутствующий файл — дефолт (0 вызовов, лимит 5).
    ///
    /// Битый файл — ошибка, а не тихий сброс счётчика: молчаливый откат к нулю
    /// обнулял бы лимит именно тогда, когда агент упирается в него чаще всего.
    /// Починить файл можно `--reset` / `--set-limit` — они пишут поверх битого.
    pub fn load(path: &Path) -> Result<Self, StateError> {
        match read_json::<ConsultState>(path) {
            Ok(state) => Ok(state),
            Err(reason) => Err(Self::classify(path, reason)),
        }
    }

    /// Прочитать состояние для служебных команд (`--status/--reset/--set-limit`).
    ///
    /// Отличие от [`ConsultState::load`]: битый файл здесь не помеха — команда
    /// перезапишет его целиком, поэтому исходное значение не теряется.
    pub fn load_or_default(path: &Path) -> Self {
        match read_json::<ConsultState>(path) {
            Ok(state) => state,
            Err(reason) => {
                let err = Self::classify(path, reason);
                log::warn!(
                    "{} — состояние считается дефолтным и будет перезаписано",
                    err
                );
                Self::default()
            }
        }
    }

    /// Записать состояние (атомарно).
    pub fn save(&self, path: &Path) -> Result<(), StateError> {
        ensure_parent(path)?;
        let content = serde_json::to_string_pretty(self).map_err(|e| StateError::Write {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;
        write_atomic(path, &content).map_err(|source| StateError::Write {
            path: path.to_path_buf(),
            reason: source.to_string(),
        })
    }

    /// Атомарно занять один вызов.
    ///
    /// Счётчик увеличивается под sidecar-локом `ko-json-store` ДО сетевого
    /// запроса: иначе два параллельных агента одновременно увидят свободный
    /// вызов и израсходуют его дважды. Если запрос не состоился (сеть/API),
    /// вызов возвращается через [`ConsultState::refund`] — неудачная попытка
    /// не должна стоить лимита.
    pub fn reserve(path: &Path) -> Result<Reservation, StateError> {
        ensure_parent(path)?;
        let mut reserved: Option<ConsultState> = None;
        let mut exhausted: Option<ConsultState> = None;
        update_json(path, |state: &mut ConsultState| {
            if state.is_exhausted() {
                exhausted = Some(*state);
            } else {
                state.calls = state.calls.saturating_add(1);
                reserved = Some(*state);
            }
        })
        .map_err(|reason| StateError::Write {
            path: path.to_path_buf(),
            reason,
        })?;

        match reserved {
            Some(state) => Ok(Reservation::Reserved(state)),
            None => Ok(Reservation::Exhausted(exhausted.unwrap_or_default())),
        }
    }

    /// Вернуть вызов, забронированный [`ConsultState::reserve`] (сетевая ошибка).
    pub fn refund(path: &Path) -> Result<(), StateError> {
        ensure_parent(path)?;
        update_json(path, |state: &mut ConsultState| {
            state.calls = state.calls.saturating_sub(1);
        })
        .map_err(|reason| StateError::Write {
            path: path.to_path_buf(),
            reason,
        })
    }

    /// Увеличить счётчик на 1 (изменение в памяти; для фиксации — [`ConsultState::save`]).
    ///
    /// Штатный путь — [`ConsultState::reserve`]: он атомарен и не даёт двум
    /// агентам потратить один вызов. Прямой `bump` нужен там, где счётчик уже
    /// лежит в памяти и решение о вызове принято (тесты, служебные команды).
    pub fn bump(&mut self) {
        self.calls = self.calls.saturating_add(1);
    }

    /// Сбросить счётчик вызовов в 0 (лимит сохраняется).
    pub fn reset(&mut self) {
        self.calls = 0;
    }

    /// Установить новый лимит.
    pub fn set_limit(&mut self, limit: u32) {
        self.limit = limit;
    }

    /// Человекочитаемый статус: «Использовано X из Y. Осталось: Z».
    pub fn status_line(&self) -> String {
        format!(
            "Использовано {} из {}. Осталось: {}",
            self.calls,
            self.limit,
            self.remaining()
        )
    }

    fn classify(path: &Path, reason: String) -> StateError {
        // `read_json` не различает «не читается» и «не разобрано» — по тексту
        // причины видно, что это разбор.
        if reason.contains("не разобран") {
            StateError::Parse {
                path: path.to_path_buf(),
                reason,
            }
        } else {
            StateError::Read {
                path: path.to_path_buf(),
                source: std::io::Error::other(reason),
            }
        }
    }
}

/// Создать каталог файла состояния, если его ещё нет.
///
/// `--state-file state/consult.json` обязан работать «из коробки»: агент не
/// должен знать, что перед записью нужно завести папку, а `ko-json-store` такую
/// папку не создаёт.
fn ensure_parent(path: &Path) -> Result<(), StateError> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() || parent.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(parent).map_err(|source| StateError::Write {
        path: parent.to_path_buf(),
        reason: format!("не удалось создать каталог: {source}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    /// Временная директория на тест: файлы состояния не должны попадать в репозиторий.
    fn tmp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "agent_tools_consult_{}_{}_{}_{}",
            tag,
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst),
            nanos
        ));
        std::fs::create_dir_all(&dir).expect("создать временную директорию");
        dir
    }

    fn read_calls(path: &Path) -> u32 {
        serde_json::from_str::<serde_json::Value>(
            &std::fs::read_to_string(path).expect("прочитать файл состояния"),
        )
        .expect("разобрать json состояния")["calls"]
            .as_u64()
            .expect("calls — число") as u32
    }

    #[test]
    fn default_state_when_file_absent() {
        let dir = tmp_dir("absent");
        let path = dir.join(STATE_FILE_NAME);

        let state = ConsultState::load(&path).expect("отсутствующий файл — дефолт");

        assert_eq!(state.calls, 0);
        assert_eq!(state.limit, DEFAULT_LIMIT);
        assert!(!state.is_exhausted());
        assert_eq!(state.remaining(), DEFAULT_LIMIT);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn roundtrip_keeps_calls_and_limit() {
        let dir = tmp_dir("roundtrip");
        let path = dir.join(STATE_FILE_NAME);
        let state = ConsultState {
            calls: 3,
            limit: 7,
        };

        state.save(&path).expect("сохранить состояние");

        assert_eq!(ConsultState::load(&path).expect("загрузить"), state);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reserve_increments_and_persists() {
        let dir = tmp_dir("reserve");
        let path = dir.join(STATE_FILE_NAME);

        let first = ConsultState::reserve(&path).expect("резерв");
        let second = ConsultState::reserve(&path).expect("резерв");

        assert_eq!(
            first,
            Reservation::Reserved(ConsultState {
                calls: 1,
                limit: 5
            })
        );
        assert_eq!(
            second,
            Reservation::Reserved(ConsultState {
                calls: 2,
                limit: 5
            })
        );
        assert_eq!(read_calls(&path), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reserve_at_limit_refuses_and_does_not_touch_counter() {
        let dir = tmp_dir("exhausted");
        let path = dir.join(STATE_FILE_NAME);
        ConsultState {
            calls: 5,
            limit: 5,
        }
        .save(&path)
        .expect("подложить состояние");

        let result = ConsultState::reserve(&path).expect("резерв при лимите");

        assert_eq!(
            result,
            Reservation::Exhausted(ConsultState {
                calls: 5,
                limit: 5
            })
        );
        assert_eq!(read_calls(&path), 5, "отказ не должен двигать счётчик");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn is_exhausted_on_boundary_and_over_limit() {
        let mut state = ConsultState { calls: 4, limit: 5 };
        assert!(!state.is_exhausted());
        assert_eq!(state.remaining(), 1);

        state.bump();
        assert!(state.is_exhausted());
        assert_eq!(state.remaining(), 0);

        // Лимит понижен ниже израсходованного — вызовов больше нет.
        state.set_limit(2);
        assert!(state.is_exhausted());
        assert_eq!(state.remaining(), 0);

        // Лимит 0 — жёсткая блокировка всех вызовов.
        let blocked = ConsultState {
            calls: 0,
            limit: 0,
        };
        assert!(blocked.is_exhausted());
        assert_eq!(blocked.remaining(), 0);
    }

    #[test]
    fn reset_clears_calls_and_keeps_limit() {
        let dir = tmp_dir("reset");
        let path = dir.join(STATE_FILE_NAME);
        let mut state = ConsultState {
            calls: 4,
            limit: 9,
        };

        state.reset();

        assert_eq!(state.calls, 0);
        assert_eq!(state.limit, 9);
        state.save(&path).expect("сохранить");
        assert_eq!(ConsultState::load(&path).expect("загрузить").calls, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_limit_changes_limit_only() {
        let mut state = ConsultState {
            calls: 2,
            limit: 5,
        };

        state.set_limit(3);

        assert_eq!(state, ConsultState { calls: 2, limit: 3 });
    }

    #[test]
    fn refund_returns_the_reserved_call() {
        let dir = tmp_dir("refund");
        let path = dir.join(STATE_FILE_NAME);

        ConsultState::reserve(&path).expect("резерв");
        ConsultState::refund(&path).expect("возврат");

        assert_eq!(read_calls(&path), 0, "неудачная попытка не должна стоить лимита");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refund_never_goes_below_zero() {
        let dir = tmp_dir("refund_zero");
        let path = dir.join(STATE_FILE_NAME);

        ConsultState::refund(&path).expect("возврат без резерва");

        assert_eq!(read_calls(&path), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_is_an_error_but_repairable_by_management_commands() {
        let dir = tmp_dir("corrupt");
        let path = dir.join(STATE_FILE_NAME);
        std::fs::write(&path, r#"{"calls": 3, "limit""#).expect("испортить файл");

        assert!(matches!(
            ConsultState::load(&path),
            Err(StateError::Parse { .. })
        ));

        let repaired = ConsultState::load_or_default(&path);
        assert_eq!(repaired, ConsultState::default());
        repaired.save(&path).expect("перезаписать битое состояние");
        assert_eq!(
            ConsultState::load(&path).expect("файл снова читается"),
            ConsultState::default()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn state_file_in_missing_directory_is_created() {
        let dir = tmp_dir("nested");
        let path = dir.join("state").join("nested").join("consult.json");

        ConsultState::reserve(&path).expect("резерв создаёт каталоги сам");

        assert!(path.is_file(), "файл состояния должен появиться в новой папке");
        assert_eq!(ConsultState::load(&path).expect("загрузить").calls, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn status_line_is_human_readable() {
        let state = ConsultState {
            calls: 2,
            limit: 5,
        };
        assert_eq!(state.status_line(), "Использовано 2 из 5. Осталось: 3");
    }

    #[test]
    fn reserve_survives_concurrent_agents() {
        let dir = tmp_dir("race");
        let path = Arc::new(dir.join(STATE_FILE_NAME));

        let mut handles = Vec::new();
        for _ in 0..8 {
            let path = Arc::clone(&path);
            handles.push(std::thread::spawn(move || {
                for _ in 0..10 {
                    let _ = ConsultState::reserve(&path);
                }
            }));
        }
        for handle in handles {
            handle.join().expect("поток завершился");
        }

        let state = ConsultState::load(&path).expect("состояние после гонки");
        assert_eq!(
            state.calls, DEFAULT_LIMIT,
            "параллельные резервы не должны перерасходовать лимит"
        );
        assert!(state.is_exhausted());
        let _ = std::fs::remove_dir_all(&dir);
    }
}