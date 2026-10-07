//! Команда `generate_text`: разовая текстовая генерация через `LlamaEngine`
//! для хостов, которым нужен инференс без чата (commit-сообщения, суммари и т.п.).
//!
//! Движок запускается как отдельный процесс `llama-server.exe`, общается по
//! HTTP (localhost, random-порт + api-key) и гарантированно убивается при Drop.
//! Контекст оценивается эвристикой worst-case ДО старта (rules.md §6.7):
//! ~3 символа/токен + запас на генерацию, точная подгонка — через /tokenize
//! после старта недоступна (--ctx-size фиксируется при запуске).

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use serde::Deserialize;
use tauri::AppHandle;

use crate::engine::{self, LlmMessage};

use super::commands;

/// Запрос на генерацию текста.
///
/// `rename_all = "camelCase"` обязателен: JS-обёртка `generateText` шлёт
/// `req: { modelPath, maxTokens, temperature }`, а Tauri конвертирует
/// camelCase→snake_case только для ОТДЕЛЬНЫХ аргументов команды, не внутри
/// вложенной структуры. Без этого ключи молча терялись, `model_path` был
/// `None`, и плагин ругался «Модель не выбрана» при реально выбранной модели.
///
/// `deny_unknown_fields` — страховка: рассогласование имён даёт явную ошибку
/// вместо тихой подстановки `#[serde(default)]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenerateTextRequest {
    /// Путь к GGUF-модели. `None`/пусто — взять `last_model` из конфига.
    #[serde(default)]
    pub model_path: Option<String>,
    /// Сообщения в OpenAI-формате (role/content).
    pub messages: Vec<LlmMessage>,
    /// Лимит токенов генерации. По умолчанию 256.
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Override temperature из параметров модели.
    #[serde(default)]
    pub temperature: Option<f32>,
}

#[tauri::command]
pub async fn generate_text(
    app: AppHandle,
    req: GenerateTextRequest,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || generate_text_blocking(&app, req))
        .await
        .map_err(|e| format!("generate_text task join error: {e}"))?
}

fn generate_text_blocking(app: &AppHandle, req: GenerateTextRequest) -> Result<String, String> {
    let model_path = resolve_model_path(app, req.model_path.as_deref())?;
    let mut params = commands::get_model_params(app.clone(), model_path.clone());
    if let Some(t) = req.temperature {
        params.temperature = t;
    }
    let max_tokens = req.max_tokens.unwrap_or(256) as usize;
    let model_max_ctx = engine::llm_gguf::extract_context_length(&model_path).ok_or_else(|| {
        format!(
            "Не удалось прочитать context_length из GGUF-модели «{model_path}». Проверьте файл."
        )
    })?;
    let ctx_limit = estimate_context(&req.messages, max_tokens, model_max_ctx);

    let log_cb = |msg: String| log::info!("[generate_text] {msg}");
    log_cb(format!(
        "модель: {model_path}, контекст: {ctx_limit}, max_tokens: {max_tokens}, model_max_ctx: {model_max_ctx}"
    ));

    let engine_dir = commands::get_engine_dir(app);
    let mut engine = engine::LlamaEngine::new(
        &engine_dir,
        &model_path,
        ctx_limit,
        false,
        false,
        0,
        log_cb,
        |_text| {},
    )?;

    let messages = req.messages;
    let cancel = Arc::new(AtomicBool::new(false));
    let result = engine.generate_chat(
        &messages,
        max_tokens,
        &params,
        "text",
        true,
        cancel,
        "generate_text",
        None,
        |_p, _t| {},
        |msg| log::info!("[generate_text] {msg}"),
    )?;

    Ok(result.text)
}

fn resolve_model_path(app: &AppHandle, explicit: Option<&str>) -> Result<String, String> {
    if let Some(p) = explicit {
        if p.is_empty() {
            return Err("model_path пуст".to_string());
        }
        if !Path::new(p).is_file() {
            return Err(format!("Файл модели не найден: {p}"));
        }
        return Ok(p.to_string());
    }
    let cfg = engine::load_config(app);
    match cfg.last_model {
        Some(m) if !m.is_empty() && Path::new(&m).is_file() => Ok(m),
        _ => Err(
            "Модель не выбрана. Добавьте модель в настройках или укажите model_path."
                .to_string(),
        ),
    }
}

/// Эвристика контекста worst-case (rules.md §6.7): ~3 символа/токен на вход,
/// плюс лимит генерации, плюс резерв на служебные токены.
///
/// Потолок — `model_max_ctx` из GGUF-метаданных модели; если ключ не читается,
/// `generate_text_blocking` возвращает ошибку, а не подставляет число.
/// Нижняя граница 2048 — чтобы llama-server согласился стартовать.
fn estimate_context(messages: &[LlmMessage], max_tokens: usize, model_max_ctx: u32) -> u32 {
    const CHARS_PER_TOKEN: usize = 3;
    const RESERVE: usize = 512;
    let input_tokens: usize = messages.iter().map(|m| m.content.len()).sum::<usize>() / CHARS_PER_TOKEN;
    let needed = input_tokens + max_tokens + RESERVE;
    (needed as u32).min(model_max_ctx).max(2048)
}
