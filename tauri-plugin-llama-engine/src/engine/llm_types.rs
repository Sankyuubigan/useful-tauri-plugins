//! Типы данных LLM и форматирование промптов

use serde::{Deserialize, Serialize};

use super::llm_gguf::extract_string_from_gguf;

/// Описание функции (инструмента) для нативного tool-calling в OpenAI-формате
/// (поле `tools` в /v1/chat/completions). `parameters` — JSON Schema (объект):
/// движок llama.cpp сам конвертирует её в GBNF и инжектит в промпт через
/// чат-шаблон. Кастомная GBNF/JSON-schema грамматика с tools[] несовместима
/// (HTTP 400 llama.cpp) — при передаче tools грамматика принудительно гасится.
#[derive(Debug, Clone, Serialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Инструмент в OpenAI-формате: `{"type": "function", "function": {...}}`.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub r#type: String,
    pub function: FunctionDef,
}

/// Вызов инструмента, возвращённый моделью (SSE `delta.tool_calls`).
/// `id` — для ответа модели ролью "tool" с `tool_call_id`; `arguments` —
/// JSON-строка аргументов (парсится на стороне хоста).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// Вложения (картинки/файлы) ВХОДА ген-вызова движка (мультимодальность).
/// API-тип плагина: его принимают методы generate_* движка, на него ссылается
/// и поле `attachments` хостового `ChatMessage` (сессии хоста).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatAttachment {
    pub file_name: String,
    pub mime_type: String,
    #[serde(default)]
    pub data_base64: String,
    #[serde(default)]
    pub file_path: Option<String>,
    #[serde(default)]
    pub is_dir: Option<bool>,
}

#[derive(Debug, Clone, Default)]
pub struct LlmMetrics {
    /// Токенов во входном промпте (из timings движка).
    pub prompt_tokens: u32,
    /// Сгенерировано токенов ответа (+ думатель).
    pub generated_tokens: u32,
    /// Токенов думателя (reasoning).
    pub reasoning_tokens: u32,
    /// Скорость приёма промпта, токен/с (из timings движка).
    pub prompt_per_second: f64,
    /// Скорость генерации, токен/с (из timings движка).
    pub predicted_per_second: f64,
    /// Время до первого токена ответа, сек.
    pub ttft_sec: f64,
    /// Полное время генерации, сек.
    pub elapsed_sec: f64,
}

#[derive(Debug, Clone)]
pub struct GenerationResult {
    pub text: String,
    pub stop_reason: String,
    /// Думатель (reasoning) модели, если движок вернул его отдельным полем
    /// (--reasoning-format deepseek → reasoning_content). Пусто, если думателя нет.
    pub reasoning: String,
    /// Вызовы инструментов (нативный OpenAI tools[]), если модель завершила ход
    /// finish_reason="tool_calls". Пусто, если модель выдала текст или инструменты
    /// не передавались. Содержимое `arguments` — JSON-строка.
    pub tool_calls: Vec<ToolCall>,
    /// Метрики генерации (токены, скорости, TTFT).
    pub metrics: LlmMetrics,
}

/// Грамматика llama.cpp для генерации: свободный текст ИЛИ строгий JSON.
/// Передаётся на /completion в поле `grammar` (GBNF), либо в поле
/// `json_schema` (JSON Schema, конвертируется движком в GBNF на лету).
#[derive(Debug, Clone, Default)]
pub struct GrammarSpec {
    /// GBNF-грамматика (сырой текст)
    pub gbnf: Option<String>,
    /// JSON Schema для конвертации движком в GBNF
    pub json_schema: Option<serde_json::Value>,
}

/// Общие JSON-правила для GBNF (строгий JSON + свободный текст).
/// Используется как база для всех форматов промптов (кроме Auto).
fn json_grammar_rules() -> &'static str {
    concat!(
        "json-object ::= \"{\" ws members ws \"}\"\n",
        "members ::= (member (\",\" ws member)*)?\n",
        "member ::= json-string ws \":\" ws json-value\n",
        "json-array ::= \"[\" ws elements ws \"]\"\n",
        "elements ::= (json-value (\",\" ws json-value)*)?\n",
        "json-value ::= json-string | json-number | json-object | json-array | json-bool | \"null\"\n",
        "json-string ::= \"\\\"\" string-char* \"\\\"\"\n",
        "string-char ::= [^\"\\\\] | escape\n",
        "escape ::= \"\\\\\" (\"\\\\\" | \"\\\"\" | \"n\" | \"t\" | \"r\" | \"b\" | \"f\" | \"u\" [0-9a-f] [0-9a-f] [0-9a-f] [0-9a-f])\n",
        "json-number ::= \"-\"? (\"0\" | [1-9] [0-9]*) (\".\" [0-9]+)? ([eE] [-+]? [0-9]+)?\n",
        "json-bool ::= \"true\" | \"false\"\n",
        "ws ::= (\" \" | \"\\t\" | \"\\n\" | \"\\r\")*\n",
    )
}

/// Базовая GBNF-грамматика движка: свободный текст ИЛИ строгий JSON.
/// Защищает структурную целостность: если модель начала выводить JSON
/// (встретился `{`), то обязана закрыть его корректно; свободная проза
/// (без фигурных скобок и обратного слэша) не ограничивается.
/// Для `PromptFormat::Auto` (Jinja-шаблон) грамматика НЕ задаётся —
/// формат токенов недетерминирован.
pub fn build_base_grammar(format: &PromptFormat) -> Option<String> {
    if *format == PromptFormat::Auto { return None; }
    Some(format!(
        "root ::= seq\n\
         seq ::= (plain-char | \"\\n\" | \"\\r\" | \"\\t\" | json-object)*\n\
         plain-char ::= [^{{}}]\n\
         {}",
        json_grammar_rules()
    ))
}

/// Строго-JSON грамматика (корень — только JSON-значение). Для агентов,
/// чей контракт — исключительно машиночитаемый JSON (напр. fact_extractor).
pub fn build_json_only_grammar() -> String {
    format!("root ::= json-object\n{}", json_grammar_rules())
}

/// Универсальная гибридная грамматика (Method 3 из docs/gbnf.md).
/// Оборачивает базовую JSON-GBNF в think-block: модель может думать в <think>...</think>,
/// а может сразу выдать JSON. Works для thinking и non-thinking моделей.
/// Non-thinking модели не имеют токенов <think> в vocab → grammar falling back на "" → сразу JSON.
pub fn get_hybrid_grammar(base_gbnf: &str) -> String {
    // Переименовываем корень базовой грамматики, чтобы не конфликтовал с нашим root
    let renamed = base_gbnf.replacen("root ::=", "json-object ::=", 1);
    let think_start = "<think>";
    let think_end = "</think>";
    format!(
        "root ::= think-block json-object\n\
         think-block ::= \"{ts}\" [^<]* \"{te}\" | \"\"\n\n{body}",
        ts = think_start,
        te = think_end,
        body = renamed,
    )
}

/// Строго-JSON грамматика с ФИКСИРОВАННЫМИ ключами — контракт fact_extractor.
/// Ключи перечислены в фиксированном порядке и без опций: boolean-факты строго
/// `true`/`false`, строковые поля — JSON-строка. Модель физически НЕ может выдать
/// неизвестный ключ (вроде `has_grounding_dont_exist` вместо `scenario_established`)
/// или пропустить обязательный факт — грамматика этого не позволит на уровне
/// декодирования, а не «просьбы» в промпте.
pub fn build_json_object_grammar_with_keys(bool_keys: &[String], string_keys: &[String]) -> String {
    if bool_keys.is_empty() && string_keys.is_empty() {
        return build_json_only_grammar();
    }
    // Локальное ПРАВИЛО ПРОБЕЛОВ `sp` — ОГРАНИЧЕННОЕ (как в официальном конвертере
    // JSON-схем llama.cpp: `space ::= | " " | "\n" [ \t]{0,20}`). НЕ используем
    // глобальный `ws ::= (...)*`: неограниченный `ws*` позволяет модели бесконечно
    // генерировать пробельные токены после `{` и зацикливаться (LOOP_DETECTED),
    // после чего экстрактор падает в fallback «все факты false».
    let sp = "sp ::= \" \" | \"\\n\" [ \\t]{0,4}";
    let mut parts: Vec<String> = Vec::new();
    for k in bool_keys {
        // GBNF: \"key\" — экранированные кавычки, чтобы в ВЫВОДЕ КЛЮЧ БЫЛ В КАВЫЧКАХ
        // (валидный JSON). Без экранирования GBNF трактует "key" как литерал БЕЗ
        // кавычек -> модель выдаёт { key : ... } -> serde падает ("key must be a string")
        // -> parse_fact_json возвращает {} -> facts_json_valid=false -> fallback "все false"
        // -> has_problem=false -> бот игнорирует запрос юзера. Это и есть корень бага.
        // (Совпадает с официальным конвертером llama.cpp: "\"name\"" -> вывод "name".)
        parts.push(format!("\"\\\"{}\\\"\" sp \":\" sp bool", k));
    }
    for k in string_keys {
        parts.push(format!("\"\\\"{}\\\"\" sp \":\" sp json-string", k));
    }
    let body = parts.join(" \",\" sp ");
    format!(
        "root ::= \"{{\" sp {} sp \"}}\"\nbool ::= \"true\" | \"false\"\n{}\n{}",
        body,
        sp,
        json_grammar_rules()
    )
}

/// Лёгкий тип для промпта LLM — только role + content.
/// Используется временно при вызове generate_chat(), не сохраняется в сессию.
///
/// Дополнительные поля `tool_calls`/`tool_call_id` — `pub(crate)`: доступны
/// сериализатору в llm.rs, но скрыты от хоста, поэтому существующие литералы
/// `LlmMessage { role, content }` в хосте НЕ ломаются. Для нативного
/// tool-calling хост используют билдеры `with_tool_calls`/`with_tool_call_id`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LlmMessage {
    pub role: String,
    pub content: String,
    pub tool_calls: Option<Vec<serde_json::Value>>,
    pub tool_call_id: Option<String>,
}

impl LlmMessage {
    /// Нативный вызов инструмента в assistant-сообщении (OpenAI tools[]):
    /// `tool_calls: [{id, type: "function", function: {name, arguments}}]`.
    /// Заполняет поля для существующего сообщения (role/content уже заданы).
    pub fn with_tool_calls(mut self, tool_calls: Vec<serde_json::Value>) -> Self {
        self.tool_calls = Some(tool_calls);
        self
    }

    /// Результат инструмента: сообщение роли "tool" с `tool_call_id` (OpenAI
    /// tools[]). Роль меняется на "tool", content — текст результата.
    pub fn as_tool_result(mut self, tool_call_id: String, content: impl Into<String>) -> Self {
        self.role = "tool".to_string();
        self.content = content.into();
        self.tool_call_id = Some(tool_call_id);
        self
    }
}

/// Извлекает имя файла из полного пути модели.
pub fn extract_model_filename(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

#[derive(Clone, PartialEq, Debug)]
pub enum PromptFormat {
    Auto,
    ChatML,
    Gemma,
    Gemma4,
    Llama3,
}

impl PromptFormat {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "gemma" => PromptFormat::Gemma,
            "gemma4" | "gemma-4" => PromptFormat::Gemma4,
            "llama3" | "llama-3" => PromptFormat::Llama3,
            "chatml" => PromptFormat::ChatML,
            _ => PromptFormat::Auto,
        }
    }

    pub fn detect_from_path(path: &str) -> Self {
        let lower = path.to_lowercase();
        if lower.contains("gemma-4") || lower.contains("gemma4") { PromptFormat::Gemma4 }
        else if lower.contains("gemma") { PromptFormat::Gemma }
        else if lower.contains("llama-3") || lower.contains("llama3") { PromptFormat::Llama3 }
        else { PromptFormat::ChatML }
    }

    pub fn detect_from_gguf(path: &str) -> Self {
        if let Some(template) = extract_string_from_gguf(path, "tokenizer.chat_template") {
            if template.contains("<|im_start|>") { return PromptFormat::ChatML; }
            if template.contains("<|start_header_id|>") { return PromptFormat::Llama3; }
            if template.contains("<|turn>") || template.contains("<turn|>") { return PromptFormat::Gemma4; }
            if template.contains("<start_of_turn>") { return PromptFormat::Gemma; }
        }
        Self::detect_from_path(path)
    }

    pub fn format_messages_jinja(template: &str, messages: &[LlmMessage]) -> Option<String> {
        let mut env = minijinja::Environment::new();
        env.add_template("chat", template).ok()?;
        let tmpl = env.get_template("chat").ok()?;

        let mut msgs_val = Vec::new();
        for m in messages {
            msgs_val.push(minijinja::context! {
                role => m.role,
                content => m.content
            });
        }

        tmpl.render(minijinja::context! {
            messages => msgs_val,
            add_generation_prompt => true
        }).ok()
    }

    pub fn format_messages(&self, messages: &[LlmMessage]) -> String {
        let mut full_prompt = String::new();
        match self {
            PromptFormat::ChatML | PromptFormat::Auto => {
                for msg in messages {
                    full_prompt.push_str(&format!("<|im_start|>{}\n{}<|im_end|>\n", msg.role, msg.content));
                }
                full_prompt.push_str("<|im_start|>assistant\n");
            },
            PromptFormat::Gemma => {
                let mut system_text = String::new();
                for msg in messages {
                    let role = &*msg.role;
                    if role == "system" {
                        if !system_text.is_empty() { system_text.push_str("\n\n"); }
                        system_text.push_str(&msg.content);
                        continue;
                    }
                    let content = if role == "user" && !system_text.is_empty() {
                        let combined = format!("{}\n\n{}", system_text, msg.content);
                        system_text.clear();
                        combined
                    } else {
                        msg.content.clone()
                    };
                    let out_role = if role == "assistant" { "model".to_string() } else { role.to_string() };
                    full_prompt.push_str(&format!("<start_of_turn>{}\n{}<end_of_turn>\n", out_role, content));
                }
                if !system_text.is_empty() {
                    full_prompt.push_str(&format!("<start_of_turn>user\n{}<end_of_turn>\n", system_text));
                }
                full_prompt.push_str("<start_of_turn>model\n");
            },
            PromptFormat::Gemma4 => {
                let mut system_text = String::new();
                for msg in messages {
                    let role = &*msg.role;
                    if role == "system" {
                        if !system_text.is_empty() { system_text.push_str("\n\n"); }
                        system_text.push_str(&msg.content);
                        continue;
                    }
                    let content = if role == "user" && !system_text.is_empty() {
                        let combined = format!("{}\n\n{}", system_text, msg.content);
                        system_text.clear();
                        combined
                    } else {
                        msg.content.clone()
                    };
                    let out_role = if role == "assistant" { "model".to_string() } else { role.to_string() };
                    full_prompt.push_str(&format!("<|turn>{}\n{}<turn|>\n", out_role, content));
                }
                if !system_text.is_empty() {
                    full_prompt.push_str(&format!("<|turn>user\n{}<turn|>\n", system_text));
                }
                full_prompt.push_str("<|turn>model\n");
            },
            PromptFormat::Llama3 => {
                for msg in messages {
                    full_prompt.push_str(&format!("<|start_header_id|>{}<|end_header_id|>\n\n{}<|eot_id|>", msg.role, msg.content));
                }
                full_prompt.push_str("<|start_header_id|>assistant<|end_header_id|>\n\n");
            }
        }
        full_prompt
    }

    pub fn get_stop_words(&self) -> Vec<&'static str> {
        match self {
            PromptFormat::ChatML | PromptFormat::Auto => vec!["<|im_end|>", "<|im_start|>"],
            PromptFormat::Gemma => vec!["<end_of_turn>", "<start_of_turn>", "<|turn|>"],
            PromptFormat::Gemma4 => vec!["<turn|>", "<|turn|>"],
            PromptFormat::Llama3 => vec!["<|eot_id|>", "<|start_header_id|>"],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_grammar_none_for_auto() {
        assert!(build_base_grammar(&PromptFormat::Auto).is_none());
    }

    #[test]
    fn base_grammar_some_for_chatml_and_json_rules() {
        let chatml = build_base_grammar(&PromptFormat::ChatML).expect("ChatML grammar");
        assert!(chatml.starts_with("root ::= seq"), "{}", chatml);
        assert!(chatml.contains("json-object ::="));
        assert!(chatml.contains("plain-char ::= [^{}]"), "{}", chatml);
    }

    #[test]
    fn json_only_grammar_root_is_object() {
        let g = build_json_only_grammar();
        assert!(g.starts_with("root ::= json-object"));
        assert!(g.contains("json-value ::="));
    }

    #[test]
    fn key_exact_grammar_lists_keys_without_options() {
        let bool_keys = vec!["has_problem".to_string(), "scenario_established".to_string()];
        let string_keys = vec!["rewritten_query".to_string()];
        let g = build_json_object_grammar_with_keys(&bool_keys, &string_keys);
        assert!(g.starts_with("root ::= \"{\" sp"), "{}", g);
        // Ключи ДОЛЖНЫ быть в экранированных кавычках GBNF (\"key\"), иначе модель
        // выдаёт { key : ... } без кавычек -> serde падает ("key must be a string").
        assert!(g.contains("\"\\\"has_problem\\\"\""), "{}", g);
        assert!(g.contains("\"\\\"scenario_established\\\"\""), "{}", g);
        assert!(g.contains("\"\\\"rewritten_query\\\"\""), "{}", g);
        // пробелы ограничены локальным правилом sp (НЕ глобальным ws*), чтобы модель
        // не могла зациклиться на пробелах после `{`
        assert!(g.contains("sp ::= \" \" | \"\\n\" [ \\t]{0,4}"), "{}", g);
        // корень — фиксированные ключи, а НЕ свободный members-цикл: модель не может добавить свои
        assert!(!g.starts_with("root ::= json-object"), "{}", g);
        assert!(!g.starts_with("root ::= seq"), "{}", g);
        // порядок строго фиксирован: has_problem идёт раньше rewritten_query
        let p1 = g.find("has_problem").unwrap();
        let p2 = g.find("rewritten_query").unwrap();
        assert!(p1 < p2);
    }

    #[test]
    fn key_exact_grammar_empty_falls_back_to_object() {
        let g = build_json_object_grammar_with_keys(&[], &[]);
        assert!(g.starts_with("root ::= json-object"), "{}", g);
    }
}