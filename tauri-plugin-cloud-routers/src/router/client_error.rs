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

/// Контент пришёл частично, но стрим закрылся БЕЗ признака конца — ни `[DONE]`,
/// ни finish_reason: провайдер/канал оборвал ответ на середине (инцидент
/// 25.10.2026: «terminated» стримов 9router на gemini — тихо обрезанный текст
/// больше не может пройти как полный ответ). Единственный источник маркера.
pub const CLOUD_TRUNCATED_MARKER: &str = "[cloud_truncated]";

/// Фрагмент текста ошибки при грубом обрыве тела ответа (connect жив, тело
/// читается — и соединение рвётся). SSOT для детектора повторяемого сбоя.
pub const STREAM_INTERRUPTED_PHRASE: &str = "стрим не дочитан";

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

/// Шлюз/провайдер не отдавал ни одного байта дольше порога тишины
/// (`CHAT_IDLE_READ_TIMEOUT_SECS`): живая очередь шлёт keep-alive и не
/// подлегает этому отказу; полная тишина после N секунд = мёртвое соединение.
pub const CLOUD_TIMEOUT_MARKER: &str = "[cloud_timeout]";

/// Запрос прерван пользователем (cancel_flag увиден в поллинге 250 мс).
/// Display варианта начинается с «Прервано пользователем» — фраза уже
/// распознаётся оркестратором хостов (инвариант не менять каскадно).
pub const CLOUD_CANCELLED_MARKER: &str = "[cloud_cancelled]";

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
    /// запрещено (инцидент 18.10.2026). `frames` — сколько кадров `data:` было
    /// в стриме: честная диагностика обязана показать, что именно произошло.
    EmptySuccess { frames: usize },
    /// Ответ оборван на середине: контент пришёл частично, но стрим закрылся
    /// без признака конца — ни `[DONE]`, ни finish_reason. Тихо обрезанный
    /// текст как полный ответ запрещён (инцидент 25.10.2026).
    Truncated { partial_chars: usize },
    /// Запрос собран с нарушенной формой (инцидент 20.10.2026:
    /// `tool_choice:"required"` без `tools[]`). Это дефект приложения, а не
    /// провайдера — квота не расходуется, счётчик не растёт.
    InvalidRequest { message: String },
    /// Соединение оказалось «мёртвым»: ни одного байта за порог тишины
    /// (async `read_timeout` reqwest). `phase` — где наступила тишина:
    /// `"headers"` (не дождались HTTP-ответа) или `"body"` (стрим оборвался
    /// в теле). Живые очереди под этот отказ не попадают: любой байт
    /// сбрасывает порог.
    TimedOut { idle_secs: u64, phase: &'static str },
    /// Пользователь прервал запрос (cancel_flag). Транспорт не глючит и
    /// провайдер не виноват — это штатный исход с собственным маркером,
    /// чтобы хост отличал его от сбоя и не делал ретраев.
    Cancelled { phase: &'static str },
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
            Self::EmptySuccess { frames } => write!(
                f,
                "Причина неизвестна: роутер закрыл соединение (HTTP 200), не передав ни одной \
дельты ответа, ни ошибки, ни объяснения (кадров в стриме: {frames}). Обычно это обрыв \
канала до провайдера — повтор запроса, как правило, помогает {CLOUD_EMPTY_STREAM_MARKER}"
            ),
            Self::Truncated { partial_chars } => write!(
                f,
                "Провайдер оборвал ответ на середине: успело прийти {partial_chars} симв., \
но признак конца стрима ([DONE]/finish_reason) не пришёл. Причину обрыва провайдер \
не передал — это транспортный сбой, повтор запроса, как правило, помогает \
{CLOUD_TRUNCATED_MARKER}"
            ),
            Self::InvalidRequest { message } => write!(
                f,
                "Запрос к облаку собран с невалидной формой: {message} {CLOUD_INVALID_REQUEST_MARKER}"
            ),
            Self::TimedOut { idle_secs, phase } => write!(
                f,
                "Облачный роутер не отдавал данных ≥{idle_secs} c (фаза: {phase}). \
Живая очередь отвечает хоть байтами; полная тишина — это мёртвое соединение. \
Повторите позже или смените модель {CLOUD_TIMEOUT_MARKER}"
            ),
            Self::Cancelled { phase } => write!(
                f,
                "Прервано пользователем (облачный запрос отменён, фаза: {phase}) {CLOUD_CANCELLED_MARKER}"
            ),
        }
    }
}

/// Инвариант формы запроса: `tool_choice` имеет смысл только при непустой
/// корзине инициализации `tools[]`. Проверяется на границе плагина — запрос
/// с нарушенной формой структурально не может покинуть приложение.
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

/// Повторяемый транспортный сбой облака: HTTP 5xx (провайдер/шлюз перегружен
/// или недоступен) или обрыв тела ответа (`STREAM_INTERRUPTED_PHRASE`).
/// Именно эти классы хост обязан ретраить («Google рвётся — это норма»).
/// 4xx — дефект запроса/конфигурации, ретрай бессмыслен и запрещён.
///
/// Разбор идёт по готовому тексту ошибки (формат `Http`:
/// `"/v1/chat/completions: HTTP 502 {body}"`) — хосты получают ошибки
/// строками, поэтому SSOT-детектор живёт рядом с источником формата.
pub fn is_retryable_transport_error(err: &str) -> bool {
    if err.contains(STREAM_INTERRUPTED_PHRASE) {
        return true;
    }
    let Some(pos) = err.find("HTTP ") else {
        return false;
    };
    let code: String = err[pos + 5..]
        .chars()
        .take_while(char::is_ascii_digit)
        .take(3)
        .collect();
    matches!(code.parse::<u16>(), Ok(status) if (500..=599).contains(&status))
}

#[cfg(test)]
mod display_tests {
    use super::*;

    /// Контракт G1: каждый транспортный исход юзер должен понимать БЕЗ
    /// чтения кода — текст обязан содержать маркер и человеческую причину.
    #[test]
    fn timed_out_display_carries_marker_and_reason() {
        let text = CloudStreamError::TimedOut { idle_secs: 600, phase: "body" }.to_string();
        assert!(text.contains(CLOUD_TIMEOUT_MARKER), "текст: {text}");
        assert!(text.contains("600"), "текст: {text}");
        assert!(text.contains("молчал") || text.contains("не отдавал"), "текст: {text}");
    }

    /// Инвариант совместимости: оркестратор хостов распознаёт отмену строкой
    /// «Прервано пользователем» (mod.rs: cancel_flag || e.contains(...)).
    /// Смена формулировки здесь сломает детектор ВСЕХ хостов.
    #[test]
    fn cancelled_display_starts_with_user_abort_phrase() {
        let text = CloudStreamError::Cancelled { phase: "body" }.to_string();
        assert!(text.contains("Прервано пользователем"), "текст: {text}");
        assert!(text.contains(CLOUD_CANCELLED_MARKER), "текст: {text}");
    }

    /// Честность (инцидент 25.10.2026): юзер обязан видеть, что причина
    /// неизвестна, а не загадочный «пустой ответ 200».
    #[test]
    fn empty_success_display_admits_unknown_cause() {
        let text = CloudStreamError::EmptySuccess { frames: 3 }.to_string();
        assert!(text.contains(CLOUD_EMPTY_STREAM_MARKER), "текст: {text}");
        assert!(text.contains("Причина неизвестна"), "текст: {text}");
        assert!(text.contains("3"), "текст обязан нести число кадров: {text}");
        assert!(!text.contains("пустой ответ"), "текст: {text}");
    }

    /// Оборванный ответ обязан называть и объём успевшего, и отсутствие
    /// причины — без ложной определённости.
    #[test]
    fn truncated_display_carries_marker_and_partial_chars() {
        let text = CloudStreamError::Truncated { partial_chars: 872 }.to_string();
        assert!(text.contains(CLOUD_TRUNCATED_MARKER), "текст: {text}");
        assert!(text.contains("872"), "текст: {text}");
        assert!(text.contains("оборвал"), "текст: {text}");
        assert!(text.contains("не передал"), "текст: {text}");
    }

    /// Классификация ретраебельных сбоев: 5xx и обрыв тела — да; 4xx и
    /// произвольный текст — нет.
    #[test]
    fn retryable_transport_error_detection() {
        assert!(is_retryable_transport_error(
            "/v1/chat/completions: HTTP 502 fetch failed (ECONNRESET)"
        ));
        assert!(is_retryable_transport_error(
            "/v1/chat/completions: HTTP 503 {\"error\":{\"code\":503}}"
        ));
        assert!(is_retryable_transport_error(
            "9router /v1/chat/completions (стрим не дочитан): connection reset"
        ));
        assert!(!is_retryable_transport_error(
            "/v1/chat/completions: HTTP 401 Unauthorized"
        ));
        assert!(!is_retryable_transport_error(
            "/v1/chat/completions: HTTP 429 Too Many Requests"
        ));
        assert!(!is_retryable_transport_error("Bad JSON ответа"));
        assert!(!is_retryable_transport_error("HTTP 5")); // недописанный код — не статус
    }
}

#[cfg(test)]
mod validate_tests {
    use super::super::client::{ChatMessage, ChatRequest};
    use super::*;

    /// GP1 (инцидент 20.10.2026): `tool_choice` без `tools[]` — форма запроса
    /// невалидна; запрос до HTTP-слоя не доходит (не уходит на провайдер).
    #[test]
    fn tool_choice_without_tools_is_rejected() {
        let mut req = ChatRequest::default();
        req.model = "test".into();
        req.messages = vec![ChatMessage {
            role: "user".into(),
            content: serde_json::Value::String("привет".into()),
            tool_calls: None,
            tool_call_id: None,
        }];
        req.tool_choice = Some("required".into());

        let err = validate_request_shape(&req);
        match err {
            Err(e) => {
                let text = e.to_string();
                assert!(text.contains("tool_choice"), "текст: {text}");
                assert!(text.contains("required"), "текст: {text}");
                assert!(
                    matches!(e, CloudStreamError::InvalidRequest { .. }),
                    "вариант: {e:?}"
                );
            }
            Ok(()) => panic!("невалидная форма обязана быть ошибкой"),
        }
    }

    /// GP1: `tools: []` — та же невалидность (пустые тулы аргумента не меняют).
    #[test]
    fn tool_choice_with_empty_tools_is_rejected() {
        let req = ChatRequest {
            model: "test".into(),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: serde_json::Value::String("привет".into()),
                tool_calls: None,
                tool_call_id: None,
            }],
            tools: Some(vec![]),
            tool_choice: Some("required".into()),
            ..Default::default()
        };
        assert!(validate_request_shape(&req).is_err());
    }

    /// GP1 (анти-регресс): нормальная форма (tools+choice / choice без тулов) не ломается.
    #[test]
    fn tool_choice_with_tools_is_valid() {
        let req_with_tools = ChatRequest {
            model: "test".into(),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: serde_json::Value::String("привет".into()),
                tool_calls: None,
                tool_call_id: None,
            }],
            tools: Some(vec![serde_json::json!({
                "type": "function",
                "function": {"name": "emit_signal", "parameters": {"type": "object", "properties": {}, "required": []}}
            })]),
            tool_choice: Some("auto".into()),
            ..Default::default()
        };
        assert!(validate_request_shape(&req_with_tools).is_ok());

        // choice отсутствует + tools нет — тоже валидная форма (обычный чат).
        let plain = ChatRequest {
            model: "test".into(),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: serde_json::Value::String("привет".into()),
                tool_calls: None,
                tool_call_id: None,
            }],
            ..Default::default()
        };
        assert!(validate_request_shape(&plain).is_ok());
    }
}
