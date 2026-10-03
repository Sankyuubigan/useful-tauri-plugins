//! Контракт System-1: типизированный вопрос на входе, вероятности на выходе.
//!
//! Дверь модуля. Плагин НЕ знает, что такое невроз, элемент или порог:
//! всё доменное остаётся в хосте. Здесь только формат данных.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Тип типизированного вопроса.
///
/// Контракт из `docs/LAYA_MODEL.md` §1: модель не генерирует текст, а выбирает
/// один из фиксированных вариантов. `Noul` — бинарный вопрос «верно/неверно»,
/// где истина живёт в слоте 1 рендера вариантов.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestionType {
    /// Бинарный вопрос. Возвращает одну вероятность `p_true`.
    Noul,
}

/// Один вариант ответа с его текстом.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionSpec {
    /// Текст критерия, который увидит модель после двоеточия.
    pub text: String,
}

/// Типизированный вопрос к модели System-1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypedQuestion {
    /// Идентификатор для маппинга результата обратно в домен (например, `"e3"`).
    /// Плагин его только переносит и возвращает — не интерпретирует.
    pub id: String,
    /// Тип вопроса.
    pub qtype: QuestionType,
    /// Формулировка вопроса.
    pub instructions: String,
    /// Ровно два варианта: `[negative, positive]`.
    ///
    /// Порядок ЗНАЧИМ и является частью схемы: `docs/LAYA_MODEL.md` §4.2 фиксирует,
    /// что перестановка вариантов меняет ответ. `PermutationPolicy::Letters` это
    /// усредняет, `PermutationPolicy::Single` — нет.
    pub options: [String; 2],
}

/// Сколько раз и в каком порядке прогонять варианты.
///
/// `Letters` — единственный режим, измеренный на 26 кейсах стенда
/// (`tools/laya_probe.py`, `--labels both`): два прохода с обменом букв
/// A/B, результат усредняется. `Single` — один проход, только для контроля.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PermutationPolicy {
    #[default]
    Letters,
    Single,
}

/// Запрос инференса.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionRequest {
    /// Текст, относительно которого решаются вопросы.
    pub state: String,
    /// Вопросы. Все решаются за ОДИН проход энкодера (батч).
    pub questions: Vec<TypedQuestion>,
    /// Политика перестановок.
    #[serde(default)]
    pub permutations: PermutationPolicy,
}

/// Устройство исполнения.
///
/// `Cpu` — единственный поддерживаемый режим. Обоснование: Ort-CUDA требует
/// CUDA 12 + cuDNN на машине пользователя, то есть системную зависимость,
/// которую хост запрещает; DirectML уже откачен после падения
/// `STATUS_DLL_NOT_FOUND`. См. docs/LAYA_MODEL.md §23.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Device {
    #[default]
    Cpu,
}

impl Device {
    pub fn as_str(&self) -> &'static str {
        match self {
            Device::Cpu => "cpu",
        }
    }
}

/// Ответ по одному вопросу.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionAnswer {
    /// Идентификатор из запроса.
    pub id: String,
    /// Усреднённая по перестановкам вероятность истинного варианта, 0..=1.
    pub p_true: f32,
    /// Сколько проходов фактически выполнено.
    pub passes: usize,
}

/// Результат инференса.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionResult {
    /// Ответы в порядке вопросов запроса.
    pub answers: Vec<QuestionAnswer>,
    /// Устройство исполнения.
    pub device: Device,
    /// Суммарное время инференса, мс.
    pub elapsed_ms: u64,
}

impl DecisionResult {
    /// Удобный доступ по идентификатору — единственный путь, которым
    /// пользуется хост, чтобы не искать ответ линейно.
    pub fn probabilities(&self) -> BTreeMap<String, f32> {
        self.answers
            .iter()
            .map(|answer| (answer.id.clone(), answer.p_true))
            .collect()
    }
}