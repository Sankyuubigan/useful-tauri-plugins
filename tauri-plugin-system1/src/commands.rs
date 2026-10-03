//! Tauri-команды плагина.
//!
//! Все команды — дженерики над `Runtime` (§4.5 PLUGIN_STANDARD): не-дженерик
//! `AppHandle` даёт `E0277` при сборке крейта хоста.

use tauri::{AppHandle, Manager, Runtime, State};

use crate::catalog::{self, ModelState};
use crate::contract::{DecisionRequest, DecisionResult};
use crate::paths;
use crate::runtime::{self, RuntimeState};
use crate::state::{resolve_model, ModelSlot};
use crate::System1Error;

/// Один файл, который System-1 обязана иметь на диске.
///
/// Отдельная структура, а не `Vec<String>`: панели нужно показать по файлу
/// размер, наличие и что именно скачивать — одним списком строк это не
/// выражается, и панель начала бы догадываться (то есть врать, §2.2).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStatus {
    /// Идентификатор компонента: `runtime` | `model` | `tokenizer`.
    pub kind: String,
    /// Имя файла, как его знает пользователь.
    pub name: String,
    /// Полный локальный путь.
    pub path: String,
    /// Файл на месте.
    pub present: bool,
    /// Размер на диске, 0 если файла нет.
    pub size_bytes: u64,
    /// Что делать, если файла нет.
    pub action: String,
}

/// Сводное состояние System-1 для UI.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusReport {
    /// Идентификатор модели по умолчанию.
    pub default_model: String,
    /// Готова ли модель к работе (все файлы на месте).
    pub model_present: bool,
    /// Ожидаемый размер модели, байт — UI показывает «646 МБ».
    pub expected_bytes: u64,
    /// Найдена ли `onnxruntime.dll`.
    pub runtime_present: bool,
    /// Реальный путь к DLL, если найдена (пусто иначе).
    pub runtime_path: String,
    /// Проверенные пути DLL — показываются при отсутствии, чтобы пользователь
    /// увидел, КУДА положить файл, а не гадал.
    pub runtime_search_paths: Vec<String>,
    /// Модель уже в памяти (загружена лениво).
    pub loaded: bool,
    /// Устройство исполнения.
    pub device: String,
    /// Каталог данных System-1.
    pub models_dir: String,
    /// Состояние ONNX Runtime.
    pub runtime: RuntimeState,
    /// Пофайловая проверка комплекта: рантайм, модель, токенизатор.
    pub files: Vec<FileStatus>,
}

/// Собрать пофайловую проверку комплекта.
///
/// Комплект ровно такой: ONNX Runtime, `model.onnx` и два файла токенизатора.
/// Порядок групп — от обязательного к опциональному, чтобы панель показывала
/// «готово» раньше, а не после всех файлов.
fn collect_files(entry: &catalog::ModelEntry) -> Vec<FileStatus> {
    let runtime_state = runtime::state();
    let mut files = vec![FileStatus {
        kind: "runtime".to_string(),
        name: "ONNX Runtime (CPU)".to_string(),
        path: runtime_state.path.clone(),
        present: runtime_state.present,
        size_bytes: runtime_state.size_bytes,
        action: format!("Скачать ONNX Runtime {}", runtime::RUNTIME_VERSION),
    }];

    for file in &entry.files {
        let local = catalog::local_path(entry, &file.remote);
        let size_bytes = std::fs::metadata(&local).map(|meta| meta.len()).unwrap_or(0);
        let kind = if file.remote == "model.onnx" {
            "model"
        } else {
            "tokenizer"
        };
        files.push(FileStatus {
            kind: kind.to_string(),
            name: file.remote.clone(),
            path: local.display().to_string(),
            present: local.is_file(),
            size_bytes,
            action: format!("Скачать {}", file.remote),
        });
    }

    files
}

/// Статус System-1: модель, рантайм, устройство.
///
/// Без дженерика по `Runtime`: команде не нужен `AppHandle`, а параметр без
/// использования оставляет тип невыводимым в `generate_handler!`.
#[tauri::command]
pub fn get_status(state: State<'_, ModelSlot>) -> Result<StatusReport, String> {
    let model_id = catalog::default_model_id();
    let entry = resolve_model(&model_id)?;
    let model = catalog::model_state(&entry);
    let runtime_path = paths::find_onnxruntime().ok();

    Ok(StatusReport {
        default_model: model_id,
        model_present: model.present,
        expected_bytes: model.expected_bytes,
        runtime_present: runtime_path.is_some(),
        runtime_path: runtime_path
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
        // Единственное место, куда кладётся DLL. Показывается даже когда DLL
        // найдена: пользователь должен видеть, куда смотреть, а не искать.
        runtime_search_paths: vec![paths::onnxruntime_dll().display().to_string()],
        loaded: state.is_loaded(),
        device: crate::contract::Device::Cpu.as_str().to_string(),
        models_dir: paths::models_dir().display().to_string(),
        runtime: runtime::state(),
        files: collect_files(&entry),
    })
}

/// Скачать весь комплект System-1: сначала ONNX Runtime, затем модель.
///
/// Порядок не переставляется: инференс без DLL невозможен, а модель без DLL
/// бесполезна, поэтому качать модель при отсутствии рантайма — тратить 646 МБ
/// впустую.
///
/// Возвращает `ModelState` — команда одна на весь комплект, потому что
/// пользователю нужно «скачать всё», а не «скачать модель, а DLL где-то ещё».
#[tauri::command]
pub async fn download_all<R: Runtime>(app: AppHandle<R>) -> Result<ModelState, String> {
    let model_id = catalog::default_model_id();
    let entry = resolve_model(&model_id)?;

    let runtime_downloaded = runtime::ensure_downloaded().await?;
    log::info!(
        "[system1] ONNX Runtime: {}",
        if runtime_downloaded {
            "скачан"
        } else {
            "уже был на месте"
        }
    );

    crate::provisioning::download(&entry)
        .await
        .map_err(|error| {
            log::error!("[system1] скачивание модели {} не удалось: {}", model_id, error);
            format!("не скачать модель {}: {}", model_id, error)
        })?;

    // Файлы на диске изменились — снять из памяти, чтобы следующий вызов
    // подхватил именно их, а не прошлые.
    if let Some(state) = app.try_state::<ModelSlot>() {
        state.release();
    }

    let state = catalog::model_state(&entry);
    if !state.present {
        return Err(format!(
            "модель скачалась, но файлы не на месте: {}",
            state.missing.join(", ")
        ));
    }
    if !paths::onnxruntime_dll().is_file() {
        return Err("ONNX Runtime не появился после скачивания".to_string());
    }
    log::info!("[system1] комплект готов: модель {} в {}", model_id, state.dir);
    Ok(state)
}

/// Скачать модель в `KingOrchData/system1/models/<id>`.
///
/// Отдельная команда сохранена для сценария «модель удалили, рантайм на месте».
/// Для установки с нуля панель зовёт [`download_all`].
///
/// Прогресс идёт через `tauri-plugin-downloader` (единый движок скачивания
/// хоста), поэтому UI получает `downloader:progress` без собственной реализации.
#[tauri::command]
pub async fn download_model<R: Runtime>(
    app: AppHandle<R>,
    model_id: Option<String>,
) -> Result<ModelState, String> {
    let model_id = model_id.unwrap_or_else(catalog::default_model_id);
    let entry = resolve_model(&model_id)?;

    crate::provisioning::download(&entry)
        .await
        .map_err(|error| {
            log::error!(
                "[system1] скачивание модели {} не удалось: {}",
                model_id,
                error
            );
            format!("не скачать модель {}: {}", model_id, error)
        })?;

    // Модель на диске изменилась — снять из памяти, чтобы следующий вызов
    // подхватил именно её, а не прошлую.
    if let Some(state) = app.try_state::<ModelSlot>() {
        state.release();
    }

    let state = catalog::model_state(&entry);
    if !state.present {
        return Err(format!(
            "модель скачалась, но файлы не на месте: {}",
            state.missing.join(", ")
        ));
    }
    log::info!("[system1] модель {} готова: {}", model_id, state.dir);
    Ok(state)
}

/// Удалить скачанную модель с диска.
#[tauri::command]
pub fn remove_model<R: Runtime>(
    app: AppHandle<R>,
    model_id: Option<String>,
) -> Result<(), String> {
    let model_id = model_id.unwrap_or_else(catalog::default_model_id);
    // Каталог удаляем целиком, поэтому проверка записи нужна не для пути, а чтобы
    // отличить «модель не установлена» (не ошибка) от «имя неизвестно» (ошибка).
    resolve_model(&model_id)?;

    // Сначала выгружаем модель из памяти: на Windows загруженная ONNX-сессия
    // держит файл открытым, и `remove_dir_all` на запертом файле падает.
    // Порядок «сначала удалить, потом выгрузить» здесь не работал бы.
    if let Some(state) = app.try_state::<ModelSlot>() {
        state.release();
    }

    let dir = paths::model_dir(&model_id);
    if !dir.exists() {
        return Ok(());
    }
    std::fs::remove_dir_all(&dir).map_err(|error| {
        log::error!("[system1] не удалить {}: {}", dir.display(), error);
        format!("не удалить модель: {}", error)
    })?;
    log::info!("[system1] модель {} удалена: {}", model_id, dir.display());
    Ok(())
}

/// Выполнить типизированные вопросы и вернуть вероятности.
///
/// Домен целиком на стороне хоста: плагин не знает, что такое `e3`, и не решает,
/// какой вердикт «правильный» — он возвращает `p_true`.
#[tauri::command]
pub fn decide<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, ModelSlot>,
    request: DecisionRequest,
) -> Result<DecisionResult, String> {
    let model_id = catalog::default_model_id();
    let model = state.get_or_load(&model_id).map_err(|error| {
        let classified = System1Error::from_message(error);
        log::error!("[system1] модель не загружена: {}", classified.user_message());
        classified.user_message()
    })?;

    model.decide(&request).map_err(|error| {
        log::error!("[system1] инференс не удался: {}", error);
        error
    })
}