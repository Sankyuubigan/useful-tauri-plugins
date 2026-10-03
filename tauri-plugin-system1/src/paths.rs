//! Пути к артефактам System-1. Единственное место, которое знает, где что лежит.
//!
//! ## Правило cwd
//!
//! Все пути считаются от `current_exe().parent()`, а НЕ от рабочего каталога и
//! НЕ от `app.path().executable_dir()`. В dev это разные пути, в инсталле
//! рабочий каталог вообще указывает в папку exe. Относительные пути
//! ломаются ровно так, как это уже случалось: `test/laya_probe/models/...`
//! не существует ни в одном инсталле.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Корневой каталог данных, заданный хостом из `app_config.data_dir`.
///
/// Хост вызывает [`crate::set_data_dir`] на старте. Без этого каталога
/// `KingOrchData` берётся от exe, но в реальной раскладке он лежит рядом с
/// папкой программы, а не внутри неё (`D:\Programs\nildencorp\KingOrchData`
/// против `D:\Programs\nildencorp\King Orch\`), и модель уехала бы в
/// несуществующее место.
static DATA_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Задать корень данных. Вызывается хостом ОДИН раз на старте.
pub fn set_data_root(path: PathBuf) {
    let _ = DATA_ROOT.set(path);
}

/// Корень данных хоста (`KingOrchData`).
///
/// Если хост не задал — каталог рядом с exe. Это запасной вариант для
/// свежей раскладки без app_config, а не основной путь.
pub fn data_root() -> PathBuf {
    DATA_ROOT
        .get()
        .cloned()
        .unwrap_or_else(|| exe_dir().join("KingOrchData"))
}

/// Корень System-1 внутри данных хоста.
pub fn system1_dir() -> PathBuf {
    data_root().join("system1")
}

/// Каталог моделей System-1.
pub fn models_dir() -> PathBuf {
    system1_dir().join("models")
}

/// Каталог конкретной модели.
pub fn model_dir(model_id: &str) -> PathBuf {
    models_dir().join(model_id)
}

/// Файл модели.
pub fn model_file(model_id: &str) -> PathBuf {
    model_dir(model_id).join("model.onnx")
}

/// Каталог токенизатора.
pub fn tokenizer_dir(model_id: &str) -> PathBuf {
    model_dir(model_id).join("tokenizer")
}

/// Каталог рантайма ONNX внутри данных хоста.
pub fn runtime_dir() -> PathBuf {
    system1_dir().join("runtime")
}

/// Путь к `onnxruntime.dll`.
///
/// **Одно место, без альтернатив.** Раньше кандидатов было два — `<exe>/laya`
/// (ставился установщиком через `bundle.resources`) и `system1/runtime`.
/// Первый нарушал `llama_cpp_engine.md:70-72` и `crispasr_engine.md:56-58`:
/// оба движка проекта не бандлятся, а `bundle.resources` по документу содержит
/// только `agents/`, `mcp_servers/`, каталоги и `*.json`. DLL — тоже движок.
pub fn onnxruntime_dll() -> PathBuf {
    runtime_dir().join(RUNTIME_DLL_NAME)
}

/// Имя файла ONNX Runtime.
pub const RUNTIME_DLL_NAME: &str = "onnxruntime.dll";

/// Найти `onnxruntime.dll` или вернуть ошибку с точным путём.
///
/// Пользователю нужен конкретный путь, а не «не найдено»: он должен видеть,
/// КУДА качать и куда панель положит файл.
pub fn find_onnxruntime() -> Result<PathBuf, String> {
    let dll = onnxruntime_dll();
    if dll.is_file() {
        log::info!("[system1] ONNX Runtime: {}", dll.display());
        return Ok(dll);
    }
    log::error!("[system1] onnxruntime.dll не найден: {}", dll.display());
    Err(format!(
        "onnxruntime.dll не найден. Ожидаемый путь: {}. Файл ставится кнопкой \
         «Скачать всё» в настройках либо вручную из официального релиза \
         microsoft/onnxruntime (v{})",
        dll.display(),
        crate::runtime::RUNTIME_VERSION
    ))
}

/// Папка exe. Единственная надёжная точка отсчёта для запущенного приложения.
pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Создать каталог, сообщив об ошибке, а не промолчав.
pub fn ensure_dir(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|error| {
        log::error!("[system1] не создать каталог {}: {}", path.display(), error);
        format!("не создать каталог {}: {}", path.display(), error)
    })
}