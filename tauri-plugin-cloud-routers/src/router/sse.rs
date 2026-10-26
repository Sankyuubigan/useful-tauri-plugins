//! Инкрементальный разбор SSE-стрима ответа облачного роутера.
//!
//! До 21.10.2026 тело стрима целиком читалось блокирующим `resp.bytes()` и
//! только потом разбиралось (`accumulate_sse_body`). Асинк-клиент читает
//! чанками (`Response::chunk`), поэтому разбор обязан быть инкрементальным:
//! дельты отдаются `on_delta` по мере прихода кадров, а не после всего тела.
//!
//! Классификация результата (`classify_sse_result`) остаётся в `client.rs`.
//! Единственный источник правил «когда Ok запрещён» — классификатор; редуктор
//! только накапливает кадры с теми же инвариантами (G1–G5 — тесты ниже).

use std::collections::BTreeMap;

use super::client::{ChatCompletionResult, CloudToolCall, StreamDelta};

/// Максимальный кусок сырого кадра в логах и ошибках: диагностика, не дамп.
pub(crate) const MAX_RAW_FRAME_CHARS: usize = 300;

pub(crate) fn raw_frame_of(line: &str) -> String {
    line.chars().take(MAX_RAW_FRAME_CHARS).collect()
}

/// Аномалии, накопленные при разборе SSE-тела. Нужны для классификации
/// результата: ошибка провайдера сильнее мусора, мусор сильнее пустоты.
#[derive(Debug, Default)]
pub(crate) struct SseScan {
    /// Обработано кадров `data:` без учёта `[DONE]` и пустых строк.
    pub(crate) frames: usize,
    /// Первый кадр с полем `error`: (сырой кадр, message из error-объекта).
    pub(crate) error_frame: Option<(String, Option<String>)>,
    /// Первый неотпарсable кадр `data:`.
    pub(crate) unparseable: Option<String>,
    /// Встречен ли `[DONE]` — признак нормального конца стрима. Обрыв
    /// (соединение закрылось без `[DONE]`) при непустом контенте — это
    /// недописанный ответ; классификатор различает их только по этому полю.
    pub(crate) done_seen: bool,
}

/// Инкрементальный накапливатель SSE-кадров.
///
/// Кадры могут рваться границей чанка в произвольном месте, поэтому хвост
/// строки доединяется буфером; обрабатываются только полные строки
/// (`\n`-терминированные). `finish` дочечивает последний неполный хвост —
/// эквивалентно старому разбору `split` по всему телу.
pub(crate) struct SseReducer {
    result: ChatCompletionResult,
    scan: SseScan,
    tool_calls: BTreeMap<usize, (String, String, String)>,
    buffer: Vec<u8>,
    /// Встречен `[DONE]` — кадры после него игнорируются.
    done: bool,
}

impl SseReducer {
    pub(crate) fn new() -> Self {
        Self {
            result: ChatCompletionResult::default(),
            scan: SseScan::default(),
            tool_calls: BTreeMap::new(),
            buffer: Vec::new(),
            done: false,
        }
    }

    /// Скормить новый чанк и отдать все ставшие полными строки.
    /// `on_delta` вызывается на каждую непустую дельту — вживую, до EOF.
    pub(crate) fn feed(&mut self, bytes: &[u8], on_delta: &mut (dyn FnMut(StreamDelta) + '_)) {
        self.buffer.extend_from_slice(bytes);
        while let Some(pos) = self.buffer.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.buffer[..pos].to_vec();
            self.buffer.drain(..=pos);
            if self.done {
                continue;
            }
            self.ingest_line(&line, on_delta);
        }
    }

    /// Дочечить хвост без `\n` (конец стрима) и собрать окончательный результат.
    pub(crate) fn finish(
        mut self,
        on_delta: &mut (dyn FnMut(StreamDelta) + '_),
    ) -> (ChatCompletionResult, SseScan) {
        if !self.done && !self.buffer.is_empty() {
            let tail = self.buffer.clone();
            self.ingest_line(&tail, on_delta);
            self.buffer.clear();
        }
        self.result.tool_calls = self
            .tool_calls
            .into_values()
            .map(|(id, name, arguments)| CloudToolCall { id, name, arguments })
            .filter(|call| !call.name.is_empty())
            .collect();
        self.scan.done_seen = self.done;
        (self.result, self.scan)
    }

    /// Разбор одной строки SSE-кадра. Логика идентична прежнему
    /// `accumulate_sse_body` (перенос, не переписывание) — изменения
    /// только в источнике строк (буфер чанков вместо сплита тела).
    fn ingest_line(&mut self, raw_line: &[u8], on_delta: &mut dyn FnMut(StreamDelta)) {
        let line = String::from_utf8_lossy(raw_line);
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with("data:") {
            return;
        }
        let data = trimmed.trim_start_matches("data:").trim();
        if data == "[DONE]" {
            self.done = true;
            self.buffer.clear();
            return;
        }
        let value: serde_json::Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(e) => {
                // Молча выбрасывать кадр запрещено (core rules §2.2): фиксируем
                // первый такой кадр сырым текстом, чтобы классификация могла
                // вернуть Err вместо «пустого успеха».
                let raw = raw_frame_of(trimmed);
                log::warn!("SSE 200: кадр стрима не разобран ({}): {}", e, raw);
                if self.scan.unparseable.is_none() {
                    self.scan.unparseable = Some(raw);
                }
                return;
            }
        };
        if value.get("error").is_some() {
            // Провайдер/роутер сообщил об ошибке уже ПОСЛЕ стартового 200
            // (типовой способ для квоты/рейт-лимита). Каждый такой кадр обязан
            // попасть в лог, по нему классифицируется Err.
            let raw = raw_frame_of(trimmed);
            let message = value
                .pointer("/error/message")
                .and_then(|m| m.as_str())
                .map(str::to_string)
                .or_else(|| {
                    value
                        .pointer("/error/code")
                        .and_then(|c| c.as_i64())
                        .map(|code| format!("code {code}"))
                });
            log::error!("SSE 200: провайдер вернул error-кадр: {}", raw);
            if self.scan.error_frame.is_none() {
                self.scan.error_frame = Some((raw, message));
            }
            return;
        }
        self.scan.frames += 1;
        if let Some(reason) = value
            .pointer("/choices/0/finish_reason")
            .and_then(|v| v.as_str())
        {
            self.result.finish_reason = reason.to_string();
        }
        let content = value
            .pointer("/choices/0/delta/content")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let reasoning = value
            .pointer("/choices/0/delta/reasoning_content")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if !content.is_empty() || !reasoning.is_empty() {
            self.result.content.push_str(content);
            self.result.reasoning.push_str(reasoning);
            on_delta(StreamDelta {
                content: content.to_string(),
                reasoning: reasoning.to_string(),
            });
        }
        if let Some(items) = value
            .pointer("/choices/0/delta/tool_calls")
            .and_then(|v| v.as_array())
        {
            for item in items {
                let index = item.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let entry = self.tool_calls.entry(index).or_default();
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    if !id.is_empty() {
                        entry.0.push_str(id);
                    }
                }
                if let Some(name) = item.pointer("/function/name").and_then(|v| v.as_str()) {
                    entry.1.push_str(name);
                }
                if let Some(arguments) = item.pointer("/function/arguments").and_then(|v| v.as_str()) {
                    entry.2.push_str(arguments);
                }
            }
        }
    }
}

/// Единый прогон готового тела — ТОЛЬКО тестовая обёртка (G1–G5): боевая
/// асинхронная цепочка кормит редуктор чанками напрямую (client.rs).
#[cfg(test)]
pub(crate) fn accumulate_sse_body(
    body: &[u8],
    on_delta: &mut (dyn FnMut(StreamDelta) + '_),
) -> (ChatCompletionResult, SseScan) {
    let mut reducer = SseReducer::new();
    reducer.feed(body, on_delta);
    reducer.finish(on_delta)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::client_error::{
        CLOUD_EMPTY_STREAM_MARKER, CLOUD_ERROR_FRAME_MARKER, CLOUD_UNPARSEABLE_FRAME_MARKER,
    };

    fn no_delta(delta: StreamDelta) {
        let _ = delta;
    }

    fn accumulate(body: &str) -> (ChatCompletionResult, SseScan) {
        accumulate_sse_body(body.as_bytes(), &mut no_delta)
    }

    /// G4 (анти-регресс): обычный стрим с контентом и finish_reason — Ok,
    /// текст склеивается по всем дельтам.
    #[test]
    fn normal_stream_with_content_is_ok() {
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"при\"}}]}\n\n\
                    data: {\"choices\":[{\"delta\":{\"content\":\"вет\"},\"finish_reason\":\"stop\"}]}\n\n\
                    data: [DONE]\n\n";
        let (result, scan) = accumulate(body);
        assert_eq!(result.content, "привет");
        assert_eq!(result.finish_reason, "stop");
        assert_eq!(scan.frames, 2);
        assert!(scan.error_frame.is_none());
        assert!(scan.unparseable.is_none());
    }

    /// G4 (анти-регресс): tool_calls-стрим по-прежнему собирается из осколков.
    #[test]
    fn stream_with_tool_calls_is_ok() {
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"\"},\"finish_reason\":null}]}\n\n\
                    data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"emit_signal\",\"arguments\":\"{\\\"key\\\"\"}}]}}]}\n\n\
                    data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\":\\\"validator_report\\\"}\"}}]}}]}\n\n\
                    data: {\"choices\":[{\"finish_reason\":\"tool_calls\"}]}\n\n\
                    data: [DONE]\n\n";
        let (result, scan) = accumulate(body);
        assert_eq!(result.tool_calls.len(), 1);
        let call = &result.tool_calls[0];
        assert_eq!(call.name, "emit_signal");
        assert_eq!(call.arguments, "{\"key\":\"validator_report\"}");
        assert_eq!(result.finish_reason, "tool_calls");
        assert!(scan.error_frame.is_none());
    }

    /// G1: error-кадр в 200-стриме → Err(InStreamError), Display содержит
    /// message из error-объекта и машиночитаемый маркер.
    #[test]
    fn sse_error_frame_is_classified_as_in_stream_error() {
        let body = "data: {\"error\":{\"message\":\"You exceeded your current quota\",\"code\":429}}\n\n\
                    data: [DONE]\n\n";
        let (result, scan) = accumulate(body);
        assert_eq!(scan.frames, 0, "error-кадр не должен считаться обычным");
        assert!(result.content.is_empty());

        let err = super::super::client::classify_sse_result(result, scan);
        match err {
            Err(e) => {
                let text = e.to_string();
                assert!(text.contains("You exceeded your current quota"), "текст: {text}");
                assert!(text.contains(CLOUD_ERROR_FRAME_MARKER), "текст: {text}");
                assert!(
                    matches!(e, super::super::client_error::CloudStreamError::InStreamError { .. }),
                    "рен: {e:?}"
                );
            }
            Ok(_) => panic!("error-кадр обязан запретить Ok («пустой успех»)"),
        }
    }

    /// G3: не-JSON кадр при отсутствии других данных → Err(UnparseableFrame)
    /// с сырым кадром и маркером (а не бесшумный Null-JSON).
    #[test]
    fn unparseable_frame_is_classified_when_nothing_recovered() {
        let body = "data: {сломанный json без кавычек}\n\n";
        let (result, scan) = accumulate(body);
        assert!(result.content.is_empty());
        assert!(result.reasoning.is_empty());
        assert!(scan.unparseable.is_some());

        let err = super::super::client::classify_sse_result(result, scan);
        match err {
            Err(e) => {
                let text = e.to_string();
                assert!(text.contains("сломанный"), "текст: {text}");
                assert!(text.contains(CLOUD_UNPARSEABLE_FRAME_MARKER), "текст: {text}");
            }
            Ok(_) => panic!("неразборчивый кадр обязан запретить Ok при пустом результате"),
        }
    }

    // ── Инкрементальность: то, ради чего редуктор существует ──

    /// Кадр, порванный пополам границей чанка, собирается из двух частей;
    /// разрыв проходит внутри многобайтового символа кириллицы («и»);
    /// дельты отдаются ровно один раз и сразу (не после EOF).
    #[test]
    fn torn_frame_reassembles_and_deltas_arrive_incrementally() {
        let mut deltas: Vec<StreamDelta> = Vec::new();
        let mut reducer = SseReducer::new();
        // Часть 1: строка кадра обрывается посреди JSON, разрыв — первый
        // байт буквы «и» (0xD0 0xB8) улетает в следующий чанк.
        let mut head: Vec<u8> =
            "data: {\"choices\":[{\"delta\":{\"content\":\"пр".into();
        head.push(0xD0);
        // Часть 2 не начинается на границе строки — дозапись между чанками.
        let mut mid: Vec<u8> = vec![0xB8];
        mid.extend_from_slice("вет\"}}]}\n\n".as_bytes());
        let rest =
            "data: {\"choices\":[{\"delta\":{\"content\":\"\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n".as_bytes();
        reducer.feed(&head, &mut |d| deltas.push(d));
        let emitted_after_head = deltas.len();
        reducer.feed(&mid, &mut |d| deltas.push(d));
        assert_eq!(emitted_after_head, 0, "первая дельта не может выйти до полного кадра");
        assert_eq!(deltas.len(), 1, "после дозаписки кадр завершается ровно одной дельтой");
        reducer.feed(rest, &mut |d| deltas.push(d));
        let (result, scan) = reducer.finish(&mut |d| deltas.push(d));
        assert_eq!(result.content, "привет");
        assert_eq!(result.finish_reason, "stop");
        assert_eq!(deltas.iter().map(|d| d.content.clone()).collect::<Vec<_>>().join(""), "привет");
        assert!(scan.unparseable.is_none());
    }

    /// Кадры после `[DONE]` игнорируются (анти-двойной-счёт).
    #[test]
    fn frames_after_done_are_ignored() {
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"ок\"},\"finish_reason\":\"stop\"}]}\n\n\
                    data: [DONE]\n\n\
                    data: {\"choices\":[{\"delta\":{\"content\":\"лишние\"}}]}\n\n";
        let (result, scan) = accumulate(body);
        assert_eq!(result.content, "ок");
        assert_eq!(scan.frames, 1);
    }

    /// Пустой успех (200, кадры есть, дельт нет) — различимо классификатором.
    #[test]
    fn empty_success_scan_visible() {
        let body = "data: {\"choices\":[{\"finish_reason\":\"\"}]}\n\n";
        let (result, scan) = accumulate(body);
        assert!(result.content.is_empty());
        assert_eq!(scan.frames, 1);
        // Само правило «пустой Ok запрещён» — в classify_sse_result (client.rs);
        let err = super::super::client::classify_sse_result(result, scan);
        match err {
            Err(e) => {
                let text = e.to_string();
                assert!(text.contains(CLOUD_EMPTY_STREAM_MARKER), "текст: {text}");
            }
            Ok(_) => panic!("пустой успех обязан запретить Ok"),
        }
    }

    /// Мусорный кадр при ВОССТАНОВЛЕННОМ контенте не считает весь запрос
    /// провалившимся: Ok, но кадр виден в логе (scan.unparseable заполнен).
    #[test]
    fn unparseable_frame_with_recovered_content_is_ok() {
        let body = "data: %%%мусор%%%\n\n\
                    data: {\"choices\":[{\"delta\":{\"content\":\"ок\"},\"finish_reason\":\"stop\"}]}\n\n";
        let (result, scan) = accumulate(body);
        assert_eq!(result.content, "ок");
        assert!(scan.unparseable.is_some(), "кадр обязан остаться в диагностике");
        assert!(super::super::client::classify_sse_result(result, scan).is_ok());
    }

    /// G2: стрим только с [DONE] / пустое тело → EmptySuccess, Ok запрещён.
    #[test]
    fn empty_stream_is_classified_as_empty_success() {
        for body in ["data: [DONE]\n\n", ""] {
            let (result, scan) = accumulate(body);
            assert_eq!(scan.frames, 0);
            assert!(scan.done_seen == !body.is_empty(), "done_seen: {}", scan.done_seen);
            match super::super::client::classify_sse_result(result, scan) {
                Err(e) => {
                    assert!(
                        matches!(
                            e,
                            super::super::client_error::CloudStreamError::EmptySuccess { .. }
                        ),
                        "для «{body}» ждём EmptySuccess, получено: {e:?}"
                    );
                    assert!(e.to_string().contains(CLOUD_EMPTY_STREAM_MARKER));
                }
                Ok(_) => panic!("«{body}» обязан вернуть Err («пустой успех» запрещён)"),
            }
        }
    }

    /// Обрыв после частичного контента (инцидент 25.10.2026): дельты пришли,
    /// но стрим закрылся без `[DONE]` и без finish_reason — тихо обрезанный
    /// Ok запрещён, ошибка несёт объём успевшего текста.
    #[test]
    fn truncated_after_partial_content_is_error() {
        use super::super::client_error::{CloudStreamError, CLOUD_TRUNCATED_MARKER};
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"поло\"}}]}\n\n\
                    data: {\"choices\":[{\"delta\":{\"content\":\"вина\"}}]}\n\n";
        let (result, scan) = accumulate(body);
        assert_eq!(result.content, "половина");
        assert!(!scan.done_seen);
        assert!(result.finish_reason.is_empty());

        match super::super::client::classify_sse_result(result, scan) {
            Err(e) => {
                let text = e.to_string();
                assert!(text.contains(CLOUD_TRUNCATED_MARKER), "текст: {text}");
                match e {
                    CloudStreamError::Truncated { partial_chars } => {
                        assert_eq!(partial_chars, "половина".chars().count());
                    }
                    other => panic!("ожидался Truncated, получено: {other:?}"),
                }
            }
            Ok(_) => panic!("обрыв без признака конца обязан запретить Ok"),
        }
    }

    /// `[DONE]` без finish_reason при непустом контенте — валидный конец:
    /// часть провайдеров не шлёт finish_reason, но завершает стрим честно.
    #[test]
    fn done_without_finish_reason_is_ok() {
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"полный\"}}]}\n\n\
                    data: [DONE]\n\n";
        let (result, scan) = accumulate(body);
        assert!(scan.done_seen);
        assert_eq!(result.content, "полный");
        assert!(result.finish_reason.is_empty());
        assert!(super::super::client::classify_sse_result(result, scan).is_ok());
    }

    /// G5: инвариант Ok — обязан быть непустой payload: либо контент/reasoning/
    /// tool_calls, либо finish_reason, названный провайдером. Ok со всем
    /// пустым и finish_reason пустым невозможен (конструкционно, через Err).
    #[test]
    fn ok_requires_payload_or_finish_reason() {
        // finish_reason есть, контента нет → Ok разрешён (провайдер назвал причину).
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"\"},\"finish_reason\":\"length\"}]}\n\n";
        let (result, scan) = accumulate(body);
        assert!(
            super::super::client::classify_sse_result(result, scan).is_ok(),
            "finish_reason валидирует Ok"
        );

        // Ни payload, ни finish_reason → Err EmptySuccess.
        match super::super::client::classify_sse_result(
            ChatCompletionResult::default(),
            SseScan::default(),
        ) {
            Err(super::super::client_error::CloudStreamError::EmptySuccess { .. }) => {}
            other => panic!("ожидался EmptySuccess, получено: {other:?}"),
        }

        // Контент восстановлен + [DONE] пришёл (finish_reason отсутствует) → Ok валиден.
        let body_ok = "data: {\"choices\":[{\"delta\":{\"content\":\"д\"}}]}\n\n\
                       data: [DONE]\n\n";
        let (r, scan) = accumulate(body_ok);
        assert!(scan.done_seen);
        assert!(super::super::client::classify_sse_result(r, scan).is_ok());
    }
}
