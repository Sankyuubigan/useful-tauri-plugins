//! Перевод Kiro EventStream → OpenAI SSE.
//!
//! События апстрима (подтверждено `transformEventStreamToSSE` 9Router):
//! - `assistantResponseEvent` → delta content;
//! - `reasoningContentEvent` → reasoning (отдельным чанком, не смешивая с текстом);
//! - `codeEvent` → delta content (код — тоже текст ответа);
//! - `toolUseEvent` → накопление tool_calls (id/name/фрагменты input), отдача
//!   одним чанком после `stop: true` фрейма;
//! - `messageStopEvent`/`metadataEvent` → finish_reason;
//! - `meteringEvent`/`metricsEvent` → usage (только учёт, клиенту не отдаём
//!   отдельно — войдёт в финальный чанк);
//! - `message-type: error|exception` → ошибка апстрима с текстом payload.
//!
//! Неизвестное событие — пропуск с `warn` в журнал, а не падение стрима:
//! AWS может добавить новый тип, и это не должно ронять все запросы.

use crate::eventstream::{EventFrame, EventStreamDecoder};

/// Состояние перевода одного SSE-потока.
pub struct KiroToSse {
    decoder: EventStreamDecoder,
    model: String,
    id: String,
    created: u64,
    /// Накопленные tool-вызовы текущего ответа.
    tools: Vec<ToolCall>,
    /// Причина завершения (из messageStop/metadata).
    finish: Option<String>,
    finished: bool,
}

#[derive(Debug, Clone, Default)]
struct ToolCall {
    id: String,
    name: String,
    input: String,
}

impl KiroToSse {
    pub fn new(model: &str, id: &str, created: u64) -> Self {
        Self {
            decoder: EventStreamDecoder::new(),
            model: model.to_string(),
            id: id.to_string(),
            created,
            tools: Vec::new(),
            finish: None,
            finished: false,
        }
    }

    /// Скормить чанк байт апстрима, получить готовые SSE-строки для клиента.
    ///
    /// Возвращает строки вида `data: {...}\n\n` и, в самом конце,
    /// `data: [DONE]\n\n`. После DONE новых строк не будет.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<String>, String> {
        let frames = self.decoder.feed(chunk).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for f in frames {
            out.extend(self.apply(&f)?);
        }
        Ok(out)
    }

    fn apply(&mut self, f: &EventFrame) -> Result<Vec<String>, String> {
        if self.finished {
            return Ok(Vec::new());
        }
        if f.message_type == "error" || f.message_type == "exception" {
            let msg = f.payload.get("message").and_then(|v| v.as_str()).unwrap_or("unknown");
            return Err(format!("Kiro-апстрим вернул {}: {}", f.event_type, msg));
        }
        let mut out = Vec::new();
        match f.event_type.as_str() {
            "assistantResponseEvent" | "codeEvent" => {
                if let Some(text) = f.payload.get("content").and_then(|v| v.as_str()) {
                    if !text.is_empty() {
                        out.push(self.chunk(serde_json::json!({
                            "delta": { "content": text },
                        })));
                    }
                }
            }
            "reasoningContentEvent" => {
                if let Some(text) = f.payload.get("content").and_then(|v| v.as_str()) {
                    if !text.is_empty() {
                        out.push(self.chunk(serde_json::json!({
                            "delta": { "reasoning_content": text },
                        })));
                    }
                }
            }
            "toolUseEvent" => {
                let id = f.payload.get("toolUseId").and_then(|v| v.as_str()).unwrap_or("");
                let name = f.payload.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let input = f.payload.get("input").and_then(|v| v.as_str()).unwrap_or("");
                let stop = f.payload.get("stop").and_then(|v| v.as_bool()).unwrap_or(false);
                if !id.is_empty() && self.tools.iter().all(|t| t.id != id) {
                    self.tools.push(ToolCall { id: id.to_string(), ..Default::default() });
                }
                if let Some(t) = self.tools.iter_mut().find(|t| t.id == id) {
                    if !name.is_empty() && t.name.is_empty() {
                        t.name = name.to_string();
                    }
                    t.input.push_str(input);
                }
                if stop {
                    let calls: Vec<serde_json::Value> = self
                        .tools
                        .iter()
                        .map(|t| {
                            serde_json::json!({
                                "id": t.id,
                                "type": "function",
                                "function": { "name": t.name, "arguments": t.input },
                            })
                        })
                        .collect();
                    self.tools.clear();
                    if !calls.is_empty() {
                        out.push(self.chunk(serde_json::json!({
                            "delta": { "tool_calls": calls },
                        })));
                    }
                }
            }
            "messageStopEvent" | "metadataEvent" => {
                let reason = f
                    .payload
                    .get("stopReason")
                    .and_then(|v| v.as_str())
                    .map(normalize_stop)
                    .unwrap_or_else(|| "stop".to_string());
                self.finish = Some(reason);
            }
            "meteringEvent" | "metricsEvent" | "contextUsageEvent" => {
                // Учёт на стороне шлюза — в журнал шага 5; клиенту не отдаём.
            }
            "initial-response" | "" => {
                // Служебный первый фрейм / фрейм без типа — игнорируем.
            }
            unknown => {
                log::warn!("cloud-routers: неизвестное Kiro-событие «{unknown}» пропущено");
            }
        }
        Ok(out)
    }

    /// Завершить поток: финальный чанк + DONE. Идемпотентно.
    pub fn finish(&mut self) -> Vec<String> {
        if self.finished {
            return Vec::new();
        }
        self.finished = true;
        // Неотданные tool-вызовы без stop-фрейма — отдаём как есть: потерять
        // вызов функции хуже, чем отдать его без явного флага завершения.
        let mut out = Vec::new();
        if !self.tools.is_empty() {
            let calls: Vec<serde_json::Value> = self
                .tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "id": t.id,
                        "type": "function",
                        "function": { "name": t.name, "arguments": t.input },
                    })
                })
                .collect();
            self.tools.clear();
            out.push(self.chunk(serde_json::json!({ "delta": { "tool_calls": calls } })));
        }
        let finish = self.finish.clone().unwrap_or_else(|| "stop".to_string());
        out.push(self.chunk(serde_json::json!({ "finish_reason": finish })));
        out.push("data: [DONE]\n\n".to_string());
        out
    }

    /// Собрать OpenAI-чанк: `choices[0].delta|finish_reason`.
    ///
    /// Форма обязана совпадать с OpenAI: IDE парсит именно `choices[0]`.
    /// Плоский `{"delta": ...}` без `choices` — не чанк, а мусор для клиента.
    fn chunk(&self, mut delta_or_finish: serde_json::Value) -> String {
        let obj = delta_or_finish.as_object_mut().expect("всегда объект");
        let choice = if obj.contains_key("finish_reason") {
            let reason = obj.remove("finish_reason").unwrap_or(serde_json::Value::Null);
            serde_json::json!({ "index": 0, "delta": {}, "finish_reason": reason })
        } else {
            let delta = obj.remove("delta").unwrap_or(serde_json::json!({}));
            serde_json::json!({ "index": 0, "delta": delta, "finish_reason": null })
        };
        let mut out = serde_json::Map::new();
        out.insert("object".to_string(), serde_json::json!("chat.completion.chunk"));
        out.insert("id".to_string(), serde_json::json!(self.id));
        out.insert("created".to_string(), serde_json::json!(self.created));
        out.insert("model".to_string(), serde_json::json!(self.model));
        out.insert("choices".to_string(), serde_json::json!([choice]));
        format!("data: {}\n\n", serde_json::Value::Object(out))
    }
}

fn normalize_stop(s: &str) -> String {
    match s.to_ascii_lowercase().as_str() {
        "end_turn" | "stop" | "stop_sequence" => "stop".to_string(),
        "tool_use" | "tooluse" => "tool_calls".to_string(),
        "max_tokens" | "length" => "length".to_string(),
        "content_filtered" => "content_filter".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eventstream::build_frame;

    fn frame(event: &str, payload: serde_json::Value) -> Vec<u8> {
        build_frame("event", event, &payload.to_string().into_bytes())
    }

    fn payloads(sse: &[String]) -> Vec<serde_json::Value> {
        sse.iter()
            .filter(|s| s.starts_with("data: ") && !s.contains("[DONE]"))
            .map(|s| serde_json::from_str::<serde_json::Value>(&s[6..]).expect("json"))
            .collect()
    }

    #[test]
    fn text_event_becomes_delta_chunk() {
        let mut t = KiroToSse::new("kiro/m", "chatcmpl-1", 1);
        let sse = t
            .feed(&frame("assistantResponseEvent", serde_json::json!({"content": "hi"})))
            .expect("feed");
        let p = payloads(&sse);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0]["choices"][0]["delta"]["content"], "hi");
        assert_eq!(p[0]["object"], "chat.completion.chunk");
    }

    #[test]
    fn tool_calls_accumulate_until_stop() {
        let mut t = KiroToSse::new("kiro/m", "chatcmpl-1", 1);
        let f1 = frame(
            "toolUseEvent",
            serde_json::json!({"toolUseId": "t1", "name": "bash", "input": "{\"cmd\":"}),
        );
        let f2 = frame("toolUseEvent", serde_json::json!({"toolUseId": "t1", "input": "\"ls\"}", "stop": true}));
        let mut all = t.feed(&f1).expect("f1");
        all.extend(t.feed(&f2).expect("f2"));
        let p = payloads(&all);
        assert_eq!(p.len(), 1, "отдача — одним чанком после stop: {p:?}");
        assert_eq!(p[0]["choices"][0]["delta"]["tool_calls"][0]["function"]["name"], "bash");
    }

    #[test]
    fn stop_event_finishes_stream_with_done() {
        let mut t = KiroToSse::new("kiro/m", "chatcmpl-1", 1);
        let _ = t
            .feed(&frame("messageStopEvent", serde_json::json!({"stopReason": "end_turn"})))
            .expect("stop");
        let end = t.finish();
        assert!(end.iter().any(|s| s.contains("\"finish_reason\":\"stop\"")), "{end:?}");
        assert!(end.last().expect("non-empty").contains("[DONE]"));
        assert!(t.finish().is_empty(), "повторный finish молчит");
    }

    #[test]
    fn exception_becomes_honest_error() {
        let mut t = KiroToSse::new("kiro/m", "chatcmpl-1", 1);
        let raw = build_frame(
            "exception",
            "com.amazon.codewhisperer#ValidationException",
            &serde_json::json!({"message": "bad"}).to_string().into_bytes(),
        );
        let err = t.feed(&raw).expect_err("exception обязан ошибаться");
        assert!(err.contains("bad"), "{err}");
    }

    #[test]
    fn unknown_event_does_not_kill_stream() {
        let mut t = KiroToSse::new("kiro/m", "chatcmpl-1", 1);
        let sse = t
            .feed(&frame("futureEvent2027", serde_json::json!({"x": 1})))
            .expect("неизвестное событие не роняет поток");
        assert!(sse.is_empty());
    }
}
