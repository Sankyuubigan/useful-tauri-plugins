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

/// Класс символа строкового значения в MIXED-грамматике (свободный текст ИЛИ
/// JSON, `build_base_grammar`): raw «<» разрешён — проза не является
/// машиночитаемым контрактом.
const FREE_JSON_STRING_CHAR: &str = "[^\"\\\\]";

/// Класс символа строкового значения в СТРОГОМ JSON-контракте: raw «<» запрещён.
///
/// Механика отказа (лог юзера 01.11.26): think-тренированная модель (Ornith-класс,
/// игнорирует `enable_thinking=false`) открывает `<think>` ВНУТРИ строкового
/// значения факта/сигнала. Дифф-парсер llama-server не эмитит ни одной SSE-дельты,
/// пока think не закрыт (llama.cpp#15000), весь `n_predict` сгорает в незакрытом
/// дум-канале, приложение видит «0 токенов, MAX_TOKENS», парсер получает `{}`.
/// JSON-only ретрай think-блок не содержит, но свободный `string-char`
/// пропускал smuggle в значениях — оба вызова экстрактора умерли одинаково
/// (гибрид — через незакрытый think-блок, ретрай — через smuggle в значении).
/// Легитимный «<» остаётся доступен как escape `\u003c` (альтернатива `escape`
/// не тронута). THINK_BODY_RULE (дум-тело) это НЕ касается: там «<» разрешён
/// кроме точной `</think` — то решение (02.11.2026) не откатывается.
pub const STRICT_JSON_STRING_CHAR: &str = "[^\"\\\\<]";

/// Строковые правила JSON-грамматики (json-string / string-char / escape).
/// Единственное место, где класс символа строкового значения превращается
/// в текст правила: [`json_grammar_rules`] вплетает блок в полный набор,
/// [`strict_json_string_rules`] отдаёт его хостовым сигнальным грамматикам
/// (signals.rs) — копий правил в хосте быть не должно (SSOT).
fn json_string_rules(string_char: &str) -> String {
    format!(
        "json-string ::= \"\\\"\" string-char* \"\\\"\"\n\
         string-char ::= {sc} | escape\n\
         escape ::= \"\\\\\" (\"\\\\\" | \"\\\"\" | \"n\" | \"t\" | \"r\" | \"b\" | \"f\" | \"u\" [0-9a-f] [0-9a-f] [0-9a-f] [0-9a-f])\n",
        sc = string_char
    )
}

/// Канонический блок строковых правил СТРОГОГО JSON-контракта для хостовых
/// грамматик: raw «<» в строковых значениях запрещён
/// (см. [`STRICT_JSON_STRING_CHAR`]). Число-правила и `bool ::= …` хост
/// добавляет сам — они специфичны для его конвертов.
pub fn strict_json_string_rules() -> String {
    json_string_rules(STRICT_JSON_STRING_CHAR)
}

/// Общие JSON-правила для GBNF (строгий JSON + свободный текст).
/// Используется как база для всех форматов промптов (кроме Auto).
/// `string_char` — класс символа строкового значения: [`FREE_JSON_STRING_CHAR`]
/// для mixed-грамматики, [`STRICT_JSON_STRING_CHAR`] для строгих JSON-контрактов
/// (через [`build_json_only_grammar`] / [`build_json_object_grammar_with_keys`]).
fn json_grammar_rules(string_char: &str) -> String {
    format!(
        "json-object ::= \"{{\" ws members ws \"}}\"\n\
         members ::= (member (\",\" ws member)*)?\n\
         member ::= json-string ws \":\" ws json-value\n\
         json-array ::= \"[\" ws elements ws \"]\"\n\
         elements ::= (json-value (\",\" ws json-value)*)?\n\
         json-value ::= json-string | json-number | json-object | json-array | json-bool | \"null\"\n\
         {}\
         json-number ::= \"-\"? (\"0\" | [1-9] [0-9]*) (\".\" [0-9]+)? ([eE] [-+]? [0-9]+)?\n\
         json-bool ::= \"true\" | \"false\"\n\
         ws ::= (\" \" | \"\\t\" | \"\\n\" | \"\\r\")*\n",
        json_string_rules(string_char)
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
        json_grammar_rules(FREE_JSON_STRING_CHAR)
    ))
}

/// Строго-JSON грамматика (корень — только JSON-значение). Для агентов,
/// чей контракт — исключительно машиночитаемый JSON (напр. fact_extractor).
/// Строковые значения — строгий класс (`STRICT_JSON_STRING_CHAR`): raw «<»
/// невозможен, think-smuggle в значениях структурно закрыт.
pub fn build_json_only_grammar() -> String {
    format!(
        "root ::= json-object\n{}",
        json_grammar_rules(STRICT_JSON_STRING_CHAR)
    )
}

/// Тело think-блока: свободный текст, ВКЛЮЧАЯ символ «<» (стрелки вида `<-`,
/// сравнения, теги вроде `<file>`), кроме закрывающей последовательности
/// `</think` — она зарезервирована за терминалом think-блока.
///
/// Стандарт constrained decoding (SGLang `--reasoning-parser`, Fireworks,
/// vLLM): дум-канал не ограничивают классом одного символа — запрещают только
/// точную закрывающую последовательность. GBNF не имеет lookahead, поэтому
/// запрет выражается перечислением всех префиксов тега, не доходящих до него:
/// каждый альтернативный символ после префикса гарантированно ломает
/// совпадение с `</think`.
///
/// До 02.11.2026 тело было `[^<]`: модель, пытавшаяся написать «<»
/// (например, стрелку «1 <- Безопасность» из математики переходов),
/// получала маскировку всех продолжений, и единственным валидным
/// «<»-продолжением было `</think>` — дум-тело обрывалось посреди слова
/// (кейс валидатора 02.11.26: 7 из 9 слотов оборвались на строке
/// «- Сверка…», вердикты вышли без завершённой проверки).
pub const THINK_BODY_RULE: &str = "think-char ::= [^<] | \"<\" [^/] | \"</\" [^t] | \"</t\" [^h] | \"</th\" [^i] | \"</thi\" [^n] | \"</thin\" [^k]";

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
         think-block ::= \"{ts}\" think-char* \"{te}\" | \"\"\n\n{tbr}\n\n{body}",
        ts = think_start,
        te = think_end,
        tbr = THINK_BODY_RULE,
        body = renamed,
    )
}

/// Гибридная грамматика с ПОТОЛКОМ размышления: think-block ограничен
/// `budget_chars` символами (переносы строк разрешены — решение владельца
/// 31.10.2026), после чего `</think>` и JSON структурно обязательны.
///
/// Это тот же механизм, что у сигнальных агентов оркестратора
/// (`build_signal_envelope_grammar_with_think_budget`), но для произвольной
/// базовой JSON-грамматики. Потолок — МАКСИМУМ, а не целевая длина: короткая
/// дума не меняется, обрезается только бесконечный runaway (модель игнорирует
/// `enable_thinking=false` и сжигает весь `n_predict` в незакрытом `<think>`
/// — отказ «думатель без ответа», лог юзера 01.11.26). Потребитель появился —
/// механизм возвращён (в откате G1 от 31.10.26 он удалялся именно как мёртвый код).
pub fn get_hybrid_grammar_with_think_budget(base_gbnf: &str, budget_chars: usize) -> String {
    let renamed = base_gbnf.replacen("root ::=", "json-object ::=", 1);
    let think_start = "<think>";
    let think_end = "</think>";
    format!(
        "root ::= think-block json-object\n\
         think-block ::= \"{ts}\" think-char{{0,{budget}}} \"{te}\" | \"\"\n\n{tbr}\n\n{body}",
        ts = think_start,
        te = think_end,
        tbr = THINK_BODY_RULE,
        budget = budget_chars,
        body = renamed,
    )
}

// ── SSOT авто-потолка think для ВСЕХ JSON-контрактов ──
//
// Один механизм для всех потребителей гибридной грамматики (signal-агенты
// оркестратора, узлы `llm_fact_extractor`, fused-группы, вердикт-стенды).
// До 01.11.2026 формула жила в host-домене (`signals.rs`), и потребители,
// до которых она не доехала (узел факт-экстрактора), оставались с
// неограниченным think: модель, игнорирующая `enable_thinking=false`,
// сжигала весь `n_predict` в незакрытом `<think>` — «думатель без ответа»
// (лог юзера 01.11.26: Ornith, 2×400 с, 0 видимых токенов, MAX_TOKENS).

/// Резерв токенов под JSON-конверт (имена ключей, скобки, кавычки).
///
/// Конверт жёстко задан грамматикой: около 150 символов, то есть порядка 50
/// токенов. Здесь взят запас примерно пятикратный: резервировать ровно
/// минимум нельзя, любая ошибка в оценке съедает конверт и возвращает отказ
/// «слой не ответил».
pub const ENVELOPE_TOKENS: usize = 256;

/// Символов на токен для русского текста, **взято с запасом вниз**.
///
/// Грамматика режет черновик в символах, а движок — в токенах, поэтому перевод
/// нужен в любую сторону. Замер 11.10.26 при `max_gen_tokens = 2048`: ответы,
/// дошедшие до конверта, не длиннее 5443 символов, ответы без конверта — от
/// 6851. Значит бюджет в 2048 токенов вмещает от 2.7 до 3.3 символа на токен.
/// Берётся 2.0: при меньшем коэффициенте граница черновика наступает раньше,
/// чем кончится бюджет. Цена — черновики короче измеренного среднего;
/// выигрыш — «не дошёл до конверта» становится структурно невозможен.
pub const CHARS_PER_TOKEN: usize = 2;

/// Авто-потолок ОДНОГО черновика размышления, вычисленный от бюджетов движка.
///
/// У боевого движка два независимых капа думы, и потолок обязан укладываться
/// в оба — иначе модель, заполнившая черновик до потолка грамматики,
/// структурно не доходит до JSON-конверта (тот механический отказ, который
/// граница должна устранять):
///
/// 1. `--reasoning-budget N` (N > 0) — жёсткий кап СУММАРНОГО размышления в
///    токенах, enforced парсером reasoning на стороне сервера. Измерено на
///    батче 31.10.2026: LFM2.5-8B в валидаторе сжёг 1491 токен из 1500 —
///    граница движка наступила раньше конверта.
/// 2. `n_predict = max_gen_tokens` — общий бюджет генерации, покрывающий
///    думу И конверт вместе.
///
/// `reasoning_budget = 0` — кап не активен (не задан или отключён фолбэком
/// цикла запуска): считаем только от общего бюджета генерации.
///
/// Возвращает `None`, если граница выходит нулевой: такой бюджет не способен
/// вместить даже пустой черновик, и молча ставить `Some(0)` значило бы
/// запретить модели размышлять вовсе. Тогда грамматика строится без границы,
/// и отказ приходит из движка, а не из заведомо непроходимого формата.
pub fn auto_think_budget_chars(
    max_gen_tokens: usize,
    reasoning_budget: usize,
    drafts: usize,
) -> Option<usize> {
    auto_think_budget_chars_with_envelope(
        max_gen_tokens,
        reasoning_budget,
        drafts,
        ENVELOPE_TOKENS,
    )
}

/// То же, что [`auto_think_budget_chars`], но резерв под конверт задаёт
/// ВЫЗЫВАЮЩИЙ, а не константа [`ENVELOPE_TOKENS`].
///
/// Зачем (контракт вердикт-текстов валидатора, 05.11.2026): 9 текстовых
/// вердиктов по ≤200 симв + `target` — конверт весит порядка 1200 токенов;
/// константа 256 резервировала бы под него лишь пятую часть, и в худшем
/// случае (черновики забрали бюджет до границы) конверт обрезался бы по
/// `n_predict` — «слой не ответил». Размер резерва вычисляется из КОНТРАКТА
/// (максимальная форма выхода по схеме), а не подобран под кейс: бюджет
/// зашит в формат, а не в константу (§00.2).
pub fn auto_think_budget_chars_with_envelope(
    max_gen_tokens: usize,
    reasoning_budget: usize,
    drafts: usize,
    envelope_tokens: usize,
) -> Option<usize> {
    let gen_budget = max_gen_tokens.saturating_sub(envelope_tokens.min(max_gen_tokens));
    let think_tokens = if reasoning_budget == 0 {
        gen_budget
    } else {
        reasoning_budget.min(gen_budget)
    };
    let per_draft_tokens = think_tokens / drafts.max(1);
    let chars = per_draft_tokens.saturating_mul(CHARS_PER_TOKEN);
    if chars == 0 {
        None
    } else {
        Some(chars)
    }
}

/// ЕДИНСТВЕННАЯ точка сборки гибридной грамматики для JSON-контракта.
///
/// Любой LLM-вызов с JSON-выходом обязан идти через неё, а не через
/// [`get_hybrid_grammar`] напрямую: think-блок здесь автоматически ограничен
/// бюджетами движка ([`auto_think_budget_chars`]) — короткая дума не меняется
/// (потолок — МАКСИМУМ, не целевая длина), а бесконечный runaway становится
/// невозможным по грамматике. `None` (нулевой бюджет) — деградация к
/// неограниченной гибридной грамматике: отказ тогда приходит из движка, а не
/// из заведомо непроходимого формата.
pub fn get_json_contract_grammar(
    base_json_gbnf: &str,
    max_gen_tokens: usize,
    reasoning_budget: usize,
    drafts: usize,
) -> String {
    match auto_think_budget_chars(max_gen_tokens, reasoning_budget, drafts) {
        Some(budget) => get_hybrid_grammar_with_think_budget(base_json_gbnf, budget),
        None => get_hybrid_grammar(base_json_gbnf),
    }
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
        json_grammar_rules(STRICT_JSON_STRING_CHAR)
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
        // Mixed-грамматика НЕ строгий контракт: raw «<» в строковых значениях
        // разрешён (проза не является машиночитаемым контрактом).
        assert!(chatml.contains("string-char ::= [^\"\\\\] | escape"), "{}", chatml);
    }

    #[test]
    fn json_only_grammar_root_is_object() {
        let g = build_json_only_grammar();
        assert!(g.starts_with("root ::= json-object"));
        assert!(g.contains("json-value ::="));
        // Строгий контракт: raw «<» в строковых значениях запрещён
        // (think-smuggle, llama.cpp#15000).
        assert!(g.contains("string-char ::= [^\"\\\\<] | escape"), "{}", g);
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
        // строгий строковый класс: think-smuggle в значениях невозможен
        assert!(g.contains("string-char ::= [^\"\\\\<] | escape"), "{}", g);
    }

    #[test]
    fn strict_json_string_char_bans_think_smuggle_in_values() {
        // Лог юзера 01.11.26: JSON-only ретрай (без think-блока) всё равно умер —
        // Ornith открыла <think> ВНУТРИ строкового значения, сервер держал дельты
        // до его закрытия (llama.cpp#15000), 0 токенов, MAX_TOKENS. В строгом
        // контракте raw «<» в значении невозможен: smuggle структурно закрыт,
        // легитимный «<» остаётся доступен как escape \u003c.
        let bool_keys = vec!["has_problem".to_string()];
        let strict = build_json_object_grammar_with_keys(&bool_keys, &[]);
        assert!(strict.contains("string-char ::= [^\"\\\\<] | escape"), "{}", strict);
        assert!(
            !strict.contains("string-char ::= [^\"\\\\] | escape"),
            "старый свободный класс просочился в строгий контракт: {}",
            strict
        );
        // THINK_BODY_RULE не тронуто: «<» в дум-теле по-прежнему разрешён кроме
        // точной `</think` (решение 02.11.2026, не откатывается) — это правило
        // подключается гибридом, см. json_contract_grammar_bounds_think_by_engine_budgets.
    }

    #[test]
    fn strict_json_string_rules_is_the_canonical_host_block() {
        // SSOT: хостовые сигнальные грамматики (signals.rs) обязаны получать
        // строковые правила этим блоком — тот же байт-в-байт, что вплетён в
        // плагиновые строгие контракты; отдельного текста класса больше нет.
        let rules = strict_json_string_rules();
        assert!(rules.contains("json-string ::="), "{}", rules);
        assert!(rules.contains("string-char ::= [^\"\\\\<] | escape"), "{}", rules);
        assert!(rules.contains("escape ::="), "{}", rules);
        assert!(
            build_json_only_grammar().contains(&rules),
            "канонический блок не совпадает с вплетённым в строгий контракт: {}",
            rules
        );
    }

    #[test]
    fn key_exact_grammar_empty_falls_back_to_object() {
        let g = build_json_object_grammar_with_keys(&[], &[]);
        assert!(g.starts_with("root ::= json-object"), "{}", g);
    }

    #[test]
    fn auto_think_budget_matches_battle_values() {
        // Боевые бюджеты (max_gen_tokens=2048, reasoning_budget=1500):
        // один черновик → 3000 симв; девять (батч 31.10.26) → 332 симв.
        assert_eq!(auto_think_budget_chars(2048, 1500, 1), Some(3000));
        assert_eq!(auto_think_budget_chars(2048, 1500, 9), Some(332));
        // rb=0 — от общего бюджета генерации.
        assert_eq!(auto_think_budget_chars(2048, 0, 1), Some((2048 - 256) * 2));
        // Нулевой бюджет → None (грамматика без границы, а не Some(0)).
        assert_eq!(auto_think_budget_chars(256, 0, 1), None);
    }

    #[test]
    fn auto_think_budget_with_envelope_contract_sized_reserve() {
        // Контракт вердикт-текстов (~1200 токенов конверта): резерв задаёт
        // вызывающий, дума получает остаток — конверт не съедается в худшем
        // случае (см. док with_envelope).
        assert_eq!(
            auto_think_budget_chars_with_envelope(2048, 0, 9, 1200),
            Some(((2048 - 1200) * 2) / 9),
        );
        // Дефолтная константа — прежняя арифметика (2048-256)*2/9.
        assert_eq!(
            auto_think_budget_chars_with_envelope(2048, 0, 9, 256),
            Some(398),
        );
        // Резерв ≥ бюджета генерации → None (нечего делить).
        assert_eq!(auto_think_budget_chars_with_envelope(256, 0, 9, 300), None);
    }

    #[test]
    fn json_contract_grammar_bounds_think_by_engine_budgets() {
        let base = build_json_object_grammar_with_keys(&["has_problem".to_string()], &[]);
        let g = get_json_contract_grammar(&base, 2048, 1500, 1);
        assert!(g.contains("root ::= think-block json-object"), "{}", g);
        assert!(g.contains("think-char{0,3000}"), "нет авто-потолка: {}", g);
        assert!(g.contains("think-char ::= [^<]"), "тело think не разрешает «<»: {}", g);
        assert!(g.contains("has_problem"), "потерян ключ: {}", g);
        // Нулевой бюджет — деградация к неограниченной гибридной, не Some(0).
        let unbounded = get_json_contract_grammar(&base, 256, 0, 1);
        assert!(unbounded.contains("think-char*"), "{}", unbounded);
    }
}