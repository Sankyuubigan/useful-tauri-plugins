//! Классификация ошибок облачного стрима — транспортный контракт плагина.
//!
//! Корневой инцидент 18.10.2026 (King Orch): 9Router вернул HTTP 200 с SSE без
//! единой дельты ×5 подряд, и оркестратор крутанул пять слепых повторов.
//! Прежний парсер молча проглатывал error-кадры, неразборчивый мусор
//! и finish_reason — «пустой успех» доходил до домена как обычный ответ, и
//! причина оставалась недоказуемой. Теперь пустой успех и внутренние ошибки
//! стрима структурно невозможны как `Ok` — они возвращаются классифицированной
//! ошибкой с машиночитаемым маркером.

use std::fmt;

/// Стрим закрылся без контента, размышлений и tool-вызовов при пустом
/// finish_reason. Единственный источник маркера — эта константа.
pub const CLOUD_EMPTY_STREAM_MARKER: &str = "[cloud_empty_stream]";

/// В HTTP 200 пришёл кадр `{"error": {...}}` — провайдер/роутер сообщает об
/// ошибке при формально успешном статусе.
pub const CLOUD_ERROR_FRAME_MARKER: &str = "[cloud_error_frame]";

/// Кадр `data:` не разобрался как JSON. Молча терять данные запрещено
/// (core rules §2.2 — правило красной кнопки).
pub const CLOUD_UNPARSEABLE_FRAME_MARKER: &str = "[cloud_unparseable]";

/// Форма запроса нарушает контракт OpenAI: `tool_choice` без непустого
/// `tools[]` — инвариант «принуждение имеет смысл только при наличии тулов».
/// Запрос с такой формой до роутера не доходит (валидация на границе).
pub const CLOUD_INVALID_REQUEST_MARKER: &str = "[cloud_invalid_request]";

#[derive(Debug, Clone)]
pub enum CloudStreamError {
    /// Транспортный сбой: HTTP не 2xx, соединение оборвалось, тело не
    /// дочиталось. `message` хранит уже готовый текст прежнего формата
    /// (`"/v1/chat/completions: HTTP {status} {body}"`) — на него завязаны
    /// логи и распознаватели хостов.
    Http { message: String },
    /// В HTTP 200 пришёл кадр с полем `error`. `raw_frame` — сырой кадр для
    /// диагностики, `message` — извлечённое тело ошибки, если удалось.
    InStreamError { raw_frame: String, message: Option<String> },
    /// Один из кадров `data:` не распарсился. `raw_frame` — сырой кадр.
    UnparseableFrame { raw_frame: String },
    /// HTTP 200, стрим закрыт, но ни контента, ни размышлений, ни tool-вызовов,
    /// ни полезного finish_reason — «пустой успех». Возвращать его как Ok
    /// запрещено (инцидент 18.10.2026).
    EmptySuccess,
    /// Запрос собран с нарушенной формой (инцидент 20.10.2026:
    /// `tool_choice:"required"` без `tools[]`). Это дефект приложения, а не
    /// провайдера — квота не расходуется, счётчик не растёт.
    InvalidRequest { message: String },
}

impl fmt::Display for CloudStreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http { message } => write!(f, "{message}"),
            Self::InStreamError { raw_frame, message } => {
                let detail = message.as_deref().unwrap_or(raw_frame.as_str());
                write!(
                    f,
                    "Облако вернуло ошибку внутри успешного HTTP 200: {detail} {CLOUD_ERROR_FRAME_MARKER}"
                )
            }
            Self::UnparseableFrame { raw_frame } => write!(
                f,
                "Ответ облака содержит кадр, который не удалось разобрать: «{raw_frame}» {CLOUD_UNPARSEABLE_FRAME_MARKER}"
            ),
            Self::EmptySuccess => write!(
                f,
                "Облако вернуло пустой ответ: HTTP 200 без контента, размышлений и tool-вызовов (подробности — в кадрах лога) {CLOUD_EMPTY_STREAM_MARKER}"
            ),
            Self::InvalidRequest { message } => write!(
                f,
                "Запрос к облаку собран с невалидной формой: {message} {CLOUD_INVALID_REQUEST_MARKER}"
            ),
        }
    }
}

/// Инвариант формы запроса: `tool_choice` имеет смысл только при непустой
/// корзине инициализации `tools[]`. Проверяется на границе плагина — запрос
/// с нарушенной формой структурно не может покинуть приложение.
pub fn validate_request_shape(req: &super::client::ChatRequest) -> Result<(), CloudStreamError> {
    if let Some(choice) = &req.tool_choice {
        let tools_present = req.tools.as_ref().is_some_and(|t| !t.is_empty());
        if !tools_present {
            return Err(CloudStreamError::InvalidRequest {
                message: format!(
                    "tool_choice=\"{choice}\" указан без tools[] — по контракту OpenAI такой запрос невалиден"
                ),
            });
        }
    }
    Ok(())
}
