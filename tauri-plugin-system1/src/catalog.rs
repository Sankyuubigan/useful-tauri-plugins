//! Каталог моделей System-1 и их автоскачивание.
//!
//! Решение владельца: модель автоматически скачивается в подкаталог
//! `KingOrchData`. Поэтому модель НЕ бандлится в инсталлер (это +646 МБ к
//! каждому обновлению приложения) — она живёт в данных пользователя, как
//! движок llama.cpp и .gguf-модели у `tauri-plugin-llama-engine`.
//!
//! Модель — артефакт внешнего репозитория, поэтому у каждой записи есть
//! ожидаемый sha256. Без проверки «скачалось что-то» означало бы «модель может
//! быть чем угодно», а весит она 646 МБ и молча меняет все вердикты.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::paths;

/// Описание модели в каталоге.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    /// Идентификатор, он же имя каталога в `KingOrchData/system1/models/`.
    pub id: String,
    /// Человекочитаемое имя для UI.
    pub display_name: String,
    /// Репозиторий на HuggingFace.
    pub repo: String,
    /// Ревision — фиксированный коммит, а не ветка `main`.
    ///
    /// Ветка означает, что содержимое может измениться под тем же sha256, и
    /// проверка целостности станет ложной. Коммит делает её осмысленной.
    pub revision: String,
    /// Файлы модели: путь внутри репозитория → sha256.
    pub files: Vec<ModelFile>,
}

/// Один файл модели.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelFile {
    /// Путь внутри репозитория HuggingFace.
    pub remote: String,
    /// Ожидаемый sha256 в hex. Пустая строка = не проверять (см. `verify`).
    pub sha256: String,
}

/// Каталог моделей System-1.
///
/// Одна модель. Запись жёстко зашита, а не читается из JSON-конфига: это
/// единственный известный исправный артефакт, и выносить его в конфиг — значит
/// создать второй источник правды о том, какой файл модели вообще валиден.
pub fn catalog() -> Vec<ModelEntry> {
    vec![ModelEntry {
        id: "laya-multilingual-onnx".to_string(),
        display_name: "Laya (System-1, ONNX fp16)".to_string(),
        repo: "mizchi/laya-multilingual-onnx".to_string(),
        // Коммит, а не ветка `main`: ветка может уехать под тем же именем, и
        // проверка целостности станет ложной. Взят из HF API 03.10.2026.
        revision: "d9d003d543e63d6d3375c21d44624136bd1e0bad".to_string(),
        files: vec![
            ModelFile {
                remote: "model.onnx".to_string(),
                sha256: "0b095e005a4c295cae74d47b7eb6931c369d48f5b720b45278d774165798310c".to_string(),
            },
            ModelFile {
                remote: "tokenizer/tokenizer.json".to_string(),
                sha256: "609d8f4c067cd3950f88594c5a802616cea245823836ef5848ee4fc40aab5b6f".to_string(),
            },
            ModelFile {
                remote: "tokenizer/tokenizer_config.json".to_string(),
                sha256: "6c6b2d8e3c84ce0e671c129cd6b374b235d6f9863042a5836358d00a89bbb5a1".to_string(),
            },
        ],
    }]
}

/// Найти запись по идентификатору.
pub fn find(id: &str) -> Option<ModelEntry> {
    catalog().into_iter().find(|entry| entry.id == id)
}

/// Идентификатор модели по умолчанию — первая запись каталога.
pub fn default_model_id() -> String {
    catalog()
        .into_iter()
        .next()
        .map(|entry| entry.id)
        .unwrap_or_default()
}

/// URL файла на HuggingFace для фиксированной ревизии.
pub fn file_url(entry: &ModelEntry, remote: &str) -> String {
    format!(
        "https://huggingface.co/{}/resolve/{}/{}",
        entry.repo, entry.revision, remote
    )
}

/// Состояние модели на диске для UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelState {
    pub id: String,
    pub display_name: String,
    /// Каталог модели (пустой, если её нет).
    pub dir: String,
    /// Все ожидаемые файлы на месте.
    pub present: bool,
    /// Какие файлов не хватает — чтобы UI сказал «нет» с перечнем, а не «ошибка».
    pub missing: Vec<String>,
    /// Ожидаемый размер model.onnx в байтах, для оценки времени загрузки.
    pub expected_bytes: u64,
}

/// Проверить состояние модели на диске, ничего не загружая.
pub fn model_state(entry: &ModelEntry) -> ModelState {
    let mut missing = Vec::new();
    let mut expected_bytes = 0u64;

    for file in &entry.files {
        let local = local_path(entry, &file.remote);
        if !local.is_file() {
            missing.push(file.remote.clone());
        }
        if file.remote == "model.onnx" {
            expected_bytes = 646_870_871;
        }
    }

    ModelState {
        id: entry.id.clone(),
        display_name: entry.display_name.clone(),
        dir: paths::model_dir(&entry.id).display().to_string(),
        present: missing.is_empty(),
        missing,
        expected_bytes,
    }
}

/// Локальный путь файла модели по его удалённому пути.
pub fn local_path(entry: &ModelEntry, remote: &str) -> PathBuf {
    // `tokenizer/tokenizer.json` — единственный вложенный путь, и он должен
    // лечь в подкаталог `tokenizer/`, как его ждёт `tokenizers`.
    let mut path = paths::model_dir(&entry.id);
    for part in remote.split('/') {
        path.push(part);
    }
    path
}

/// Скачать sha256 файла.
pub fn sha256_file(path: &Path) -> Result<String, String> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)
        .map_err(|error| format!("не открыть {}: {}", path.display(), error))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("не прочитать {}: {}", path.display(), error))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Сверить sha256 файла с ожидаемым.
///
/// Пустой ожидаемый хэш = проверять нечем. Это НЕ «проверка пройдена», и
/// возвращается наружу явным `None`, чтобы вызывающий знал: контроля не было.
/// Молчаливый `true` здесь означал бы «файл верный», что было бы ложью
/// (`core/rules.md` §2.2).
pub fn verify(path: &Path, expected: &str) -> Result<Option<bool>, String> {
    if expected.trim().is_empty() {
        return Ok(None);
    }
    let actual = sha256_file(path)?;
    Ok(Some(actual.eq_ignore_ascii_case(expected.trim())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizer_lands_in_subdirectory() {
        let entry = find("laya-multilingual-onnx").expect("модель есть в каталоге");
        let path = local_path(&entry, "tokenizer/tokenizer.json");
        assert!(path.ends_with("tokenizer/tokenizer.json"), "получено {:?}", path);
    }

    #[test]
    fn empty_hash_means_no_verification_not_passed() {
        let dir = std::env::temp_dir();
        let file = dir.join("system1_verify_probe.txt");
        std::fs::write(&file, b"x").expect("записать пробный файл");
        let checked = verify(&file, "").expect("проверка не падает");
        assert!(checked.is_none(), "пустой хэш обязан дать None, а не true/false");
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn url_uses_pinned_revision() {
        let entry = find("laya-multilingual-onnx").expect("модель есть в каталоге");
        let url = file_url(&entry, "model.onnx");
        assert!(url.contains(&entry.repo), "url должен содержать репозиторий");
        assert!(url.contains(&entry.revision), "url должен содержать ревизию");
    }
}