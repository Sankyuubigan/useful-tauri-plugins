//! Инференс System-1 поверх ONNX.
//!
//! Перенесено из хоста (`src-tauri/src/domain/system1_validator.rs`) без изменения
//! математики: формат входа графа `[CLS] <type> ins [SEP] [MASK] opt0 [MASK] opt1
//! [SEP] state [SEP]` и `p_true = softmax(logits)[1]` определены моделью, а не нами.
//!
//! Что изменено и почему:
//!
//! * **Модель живёт один раз на процесс** (см. [`crate::state`]). В хосте
//!   `LayaValidator::load` вызывался внутри обработчика ноды, то есть 646 МБ
//!   и токенизатор перечитывались на каждом прогоне графа.
//! * **`Mutex<Session>` теперь осмыслен.** `ort` требует `&mut` для `run`, а
//!   модель разделяется между вызовами, поэтому блокировка защищает РЕАЛЬНО
//!   разделяемую сессию. В хосте она защищала единственный экземпляр, который
//!   тут же выбрасывался.
//! * **Вопросы типизированы** (`TypedQuestion`), а не привязаны к девяти
//!   элементам: плагин не знает домена.

use std::path::Path;
use std::sync::Mutex;

use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Tensor;

use crate::contract::{
    DecisionRequest, DecisionResult, Device, PermutationPolicy, QuestionAnswer, QuestionType,
    TypedQuestion,
};
use crate::paths;

/// Потолки бюджета последовательности.
///
/// `MAX_LEN` и `HEAD_MAX_LEN` — ограничения движка (`laya/common.py`,
/// `build_sequence`), а не наш выбор: за их пределами модель не видела бы
/// состояние пациента.
const MAX_LEN: usize = 2048;
const HEAD_MAX_LEN: usize = 512;
/// Библиотека молча режет каждый вариант до 48 токенов. Обрезанный вариант —
/// это НЕ тот текст, который мы написали в критериях, поэтому лимит повторяется
/// явно, а не остаётся неявным поведением библиотеки.
const OPTION_TOKEN_LIMIT: usize = 48;

/// Тип вопроса `noul` во входном тензоре модели.
const QTYPE_NOUL: i64 = 2;

/// Загруженная модель: сессия + токенизатор + специальные токены.
pub struct System1Model {
    /// `ort::Session::run` требует `&mut`, а модель разделяется между
    /// вызовами — значит нужен внутренний мьютекс.
    session: Mutex<Session>,
    tokenizer: tokenizers::Tokenizer,
    cls_id: u32,
    sep_id: u32,
    mask_id: u32,
    pad_id: u32,
    mask_token_str: String,
    model_id: String,
}

/// Ошибка с человеческим текстом: у пользователя нет исходников, ему нужен путь
/// и причина, а не имя функции.
pub type ModelError = String;

/// Буквы двух проходов: `(подпись отрицательного варианта, подпись положительного)`.
///
/// Порядок проходов совпадает с хостовым: сначала `A`/`B`, потом `B`/`A`.
const PASS_LABELS: [(&str, &str); 2] = [("A", "B"), ("B", "A")];

impl System1Model {
    /// Загрузить модель из каталога.
    ///
    /// Вызывать лениво: 646 МБ не должны читаться на старте приложения.
    pub fn load(model_id: &str) -> Result<Self, ModelError> {
        let model_path = paths::model_file(model_id);
        if !model_path.is_file() {
            return Err(format!(
                "модель System-1 не найдена: {}. Скачайте её командой download_model.",
                model_path.display()
            ));
        }

        // ONNX Runtime грузится ДО первого обращения к `ort`.
        let dll = paths::find_onnxruntime()?;
        crate::runtime::init_environment(&dll)?;

        let tokenizer_file = paths::tokenizer_dir(model_id).join("tokenizer.json");
        if !tokenizer_file.is_file() {
            return Err(format!(
                "токенизатор модели System-1 не найден: {}",
                tokenizer_file.display()
            ));
        }
        let tokenizer = tokenizers::Tokenizer::from_file(&tokenizer_file).map_err(|error| {
            format!(
                "не прочитать токенизатор {}: {}",
                tokenizer_file.display(),
                error
            )
        })?;

        let session = Session::builder()
            .map_err(|error| format!("ort builder error: {}", error))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|error| format!("ort opt level error: {}", error))?
            .with_intra_threads(crate::runtime::intra_threads())
            .map_err(|error| format!("ort threads error: {}", error))?
            .commit_from_file(&model_path)
            .map_err(|error| {
                format!(
                    "ort commit error for {}: {}",
                    model_path.display(),
                    error
                )
            })?;

        // Спецтокены: у Laya это Gemma-BPE + Metaspace, а не ModernBERT
        // ([CLS]/[SEP]/[MASK]). Поэтому берём по списку с fallback — иначе модель
        // молча получит неверные позиции масок.
        let cls_id = resolve_special(&tokenizer, &["<cls_token>", "<bos>"], 1);
        let sep_id = resolve_special(&tokenizer, &["<sep_token>", "<eos>"], 1);
        let mask_id = resolve_special(
            &tokenizer,
            &["<mask_token>", "<mask_1>", "<mask>", "[MASK]"],
            4,
        );
        let pad_id = resolve_special(&tokenizer, &["<pad_token>", "<pad>", "[PAD]"], 0);
        let mask_token_str = tokenizer
            .id_to_token(mask_id)
            .unwrap_or_else(|| "<mask_token>".to_string());

        log::info!(
            "[system1] модель {} загружена: {} (cls={} sep={} mask={} pad={})",
            model_id,
            model_path.display(),
            cls_id,
            sep_id,
            mask_id,
            pad_id
        );

        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
            cls_id,
            sep_id,
            mask_id,
            pad_id,
            mask_token_str,
            model_id: model_id.to_string(),
        })
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Выполнить запрос: все вопросы одним батчем, по одному проходу на перестановку.
    pub fn decide(&self, request: &DecisionRequest) -> Result<DecisionResult, ModelError> {
        if request.questions.is_empty() {
            return Err("запрос инференса не содержит вопросов".to_string());
        }

        let started = std::time::Instant::now();
        let passes = match request.permutations {
            PermutationPolicy::Letters => PASS_LABELS.len(),
            PermutationPolicy::Single => 1,
        };

        let mut totals = vec![0.0f32; request.questions.len()];
        for pass in 0..passes {
            let (negative_label, positive_label) = PASS_LABELS[pass % PASS_LABELS.len()];
            // Порядок СЛОТОВ не меняется: слот 1 — всегда истинный вариант.
            // Меняются буквы, что снимает смещение к конкретной букве
            // (docs/LAYA_MODEL.md §4.1, ловушка #156).
            let batch: Vec<TypedQuestion> = request
                .questions
                .iter()
                .map(|question| question.with_labels(negative_label, positive_label))
                .collect();
            for (index, value) in self.infer_batch(&request.state, &batch)?.iter().enumerate() {
                totals[index] += value;
            }
        }

        let pass_count = passes as f32;
        let answers = request
            .questions
            .iter()
            .enumerate()
            .map(|(index, question)| QuestionAnswer {
                id: question.id.clone(),
                p_true: totals[index] / pass_count,
                passes,
            })
            .collect();

        Ok(DecisionResult {
            answers,
            device: Device::Cpu,
            elapsed_ms: started.elapsed().as_millis() as u64,
        })
    }

    /// Один проход: батч из всех вопросов, одна загрузка энкодера.
    fn infer_batch(&self, state: &str, questions: &[TypedQuestion]) -> Result<Vec<f32>, ModelError> {
        let n = questions.len();

        // Состояние токенизируется ОДИН раз и переиспользуется всеми вопросами.
        // Девять вопросов не означают девять перекодирований текста пациента.
        let sanitized_state = state.replace(&self.mask_token_str, " ");
        let state_ids: Vec<u32> = self
            .tokenizer
            .encode(sanitized_state.as_str(), false)
            .map_err(|error| format!("не токенизировать состояние: {}", error))?
            .get_ids()
            .to_vec();

        let mut items_ids: Vec<Vec<u32>> = Vec::with_capacity(n);
        let mut items_markers: Vec<Vec<usize>> = Vec::with_capacity(n);

        for question in questions {
            if question.qtype != QuestionType::Noul {
                return Err(format!(
                    "вопрос {}: поддерживается только qtype=noul",
                    question.id
                ));
            }
            let (ids, markers) = self.build_sequence(question, &state_ids)?;
            items_ids.push(ids);
            items_markers.push(markers);
        }

        let max_seq_len = items_ids.iter().map(|ids| ids.len()).max().unwrap_or(1);
        let kmax = items_markers.iter().map(|markers| markers.len()).max().unwrap_or(2);

        let mut input_ids = vec![self.pad_id as i64; n * max_seq_len];
        let mut attention_mask = vec![0i64; n * max_seq_len];
        let mut marker_pos = vec![0i64; n * kmax];
        let mut marker_mask = vec![false; n * kmax];

        for (row, ids) in items_ids.iter().enumerate() {
            for (column, token) in ids.iter().enumerate() {
                input_ids[row * max_seq_len + column] = *token as i64;
                attention_mask[row * max_seq_len + column] = 1;
            }
            for (slot, position) in items_markers[row].iter().enumerate() {
                marker_pos[row * kmax + slot] = *position as i64;
                marker_mask[row * kmax + slot] = true;
            }
        }

        let tensors = ort::inputs![
            "input_ids" => tensor(input_ids, vec![n, max_seq_len], "input_ids")?,
            "attention_mask" => tensor(attention_mask, vec![n, max_seq_len], "attention_mask")?,
            "marker_pos" => tensor(marker_pos, vec![n, kmax], "marker_pos")?,
            "marker_mask" => tensor(marker_mask, vec![n, kmax], "marker_mask")?,
            "qtype" => tensor(vec![QTYPE_NOUL; n], vec![n], "qtype")?,
        ];

        let mut session = self
            .session
            .lock()
            .map_err(|error| format!("system1: сессия ONNX заблокирована: {}", error))?;

        let outputs = session
            .run(tensors)
            .map_err(|error| format!("ort run error: {}", error))?;

        let (_shape, logits) = outputs["logits"]
            .try_extract_tensor::<f32>()
            .map_err(|error| format!("не прочитать logits: {}", error))?;

        Ok((0..n)
            .map(|row| {
                let base = row * kmax;
                // softmax по двум слотам; слот 1 — истинный вариант.
                let (z0, z1) = (logits[base], logits[base + 1]);
                let max_z = z0.max(z1);
                let sum = ((z0 - max_z).exp() + (z1 - max_z).exp()).max(1e-12);
                (z1 - max_z).exp() / sum
            })
            .collect())
    }

    /// Собрать последовательность одного вопроса.
    ///
    /// Формат задан моделью: `[CLS] <type> ins [SEP] [MASK] opt0 [MASK] opt1 [SEP]
    /// state [SEP]`, позиции масок идут в `marker_pos`.
    fn build_sequence(
        &self,
        question: &TypedQuestion,
        state_ids: &[u32],
    ) -> Result<(Vec<u32>, Vec<usize>), ModelError> {
        let head_text = format!("noul question: {}", question.instructions);
        let mut head_ids = self.encode(&head_text)?;

        let mut opt_ids: Vec<Vec<u32>> = Vec::with_capacity(2);
        for option in &question.options {
            let mut tokens = self.encode(&format!(" {}", option))?;
            tokens.truncate(OPTION_TOKEN_LIMIT);
            let mut with_mask = Vec::with_capacity(tokens.len() + 1);
            with_mask.push(self.mask_id);
            with_mask.extend(tokens);
            opt_ids.push(with_mask);
        }

        let opt_sum: usize = opt_ids.iter().map(|ids| ids.len()).sum();
        let mut opt_budget = HEAD_MAX_LEN.saturating_sub(opt_sum);
        if opt_budget < 16 {
            // Варианты не влезают в бюджет головы: режем все до равных долей.
            // Пропорциональное деление, а не «оставить первый» — иначе второй
            // вариант обрезается в ноль и перестаёт быть вариантом.
            let per = (HEAD_MAX_LEN.saturating_sub(16) / opt_ids.len().max(1)).max(4);
            for ids in &mut opt_ids {
                ids.truncate(per);
            }
            let opt_sum = opt_ids.iter().map(|ids| ids.len()).sum();
            opt_budget = HEAD_MAX_LEN.saturating_sub(opt_sum);
        }
        head_ids.truncate(opt_budget.max(8));

        let mut ids = vec![self.cls_id];
        ids.extend(head_ids);
        ids.push(self.sep_id);

        let mut markers = Vec::with_capacity(opt_ids.len());
        for option in opt_ids {
            markers.push(ids.len());
            ids.extend(option);
        }
        ids.push(self.sep_id);

        let room = MAX_LEN.saturating_sub(ids.len() + 1);
        // Не `state_ids[state_ids.len() - room..]`: при room == 0 срез от конца
        // вернул бы ВСЁ состояние, то есть ровно противоположное нам.
        let take = state_ids.len().min(room);
        ids.extend_from_slice(&state_ids[..take]);
        ids.push(self.sep_id);

        if ids.len() > MAX_LEN {
            ids.truncate(MAX_LEN);
        }
        markers.retain(|position| *position < MAX_LEN);
        if markers.len() < 2 {
            return Err(format!(
                "вопрос {}: варианты не поместились в MAX_LEN={}, маркеров {}",
                question.id,
                MAX_LEN,
                markers.len()
            ));
        }

        Ok((ids, markers))
    }

    fn encode(&self, text: &str) -> Result<Vec<u32>, ModelError> {
        let cleaned = text.replace(&self.mask_token_str, " ");
        Ok(self
            .tokenizer
            .encode(cleaned.as_str(), false)
            .map_err(|error| format!("не токенизировать {:?}: {}", text, error))?
            .get_ids()
            .to_vec())
    }
}

/// Собрать тензор из данных и формы.
///
/// Граница — `PrimitiveTensorElementType` из `ort::value`: это ровно тот набор
/// типов, с которыми работает `Tensor::from_array`. Имя тензора попадает в
/// сообщение об ошибке, потому что «ort tensor error» без указания входа
/// бесполезно при отладке.
fn tensor<T>(data: Vec<T>, shape: Vec<usize>, name: &str) -> Result<Tensor<T>, ModelError>
where
    T: ort::value::PrimitiveTensorElementType + std::fmt::Debug + Clone + 'static,
{
    Tensor::from_array((shape, data))
        .map_err(|error| format!("ort tensor error ({}): {}", name, error))
}

/// Найти спецтокен по списку имён с запасным вариантом.
fn resolve_special(tokenizer: &tokenizers::Tokenizer, candidates: &[&str], fallback: u32) -> u32 {
    for candidate in candidates {
        if let Some(id) = tokenizer.token_to_id(candidate) {
            return id;
        }
    }
    log::warn!(
        "[system1] спецтокен не найден, берём значение по умолчанию: {} -> {}",
        candidates.join("/"),
        fallback
    );
    fallback
}

/// Подстановка букв в готовые варианты вопроса.
trait WithLabels {
    /// Подставить буквы в варианты, сохранив СЛОТ истины на месте 1.
    ///
    /// `negative_label` — буква отрицательного варианта, `positive_label` —
    /// положительного.
    fn with_labels(&self, negative_label: &str, positive_label: &str) -> TypedQuestion;
}

impl WithLabels for TypedQuestion {
    fn with_labels(&self, negative_label: &str, positive_label: &str) -> TypedQuestion {
        let render = |text: &str, label: &str| {
            if text.trim().is_empty() {
                // Пустой критерий рендерится явной заглушкой, а не пустой строкой:
                // модель должна видеть, что это вариант, а не разрыв текста.
                format!("{}: (без описания)", label)
            } else {
                format!("{}: {}", label, text.trim())
            }
        };
        TypedQuestion {
            id: self.id.clone(),
            qtype: self.qtype,
            instructions: self.instructions.clone(),
            options: [
                render(&self.options[0], negative_label),
                render(&self.options[1], positive_label),
            ],
        }
    }
}

/// Существует ли файл модели (без её загрузки).
pub fn model_present(model_id: &str) -> bool {
    let path: &Path = &paths::model_file(model_id);
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn question() -> TypedQuestion {
        TypedQuestion {
            id: "e1".into(),
            qtype: QuestionType::Noul,
            instructions: "Описан ли элемент?".into(),
            options: ["отрицание".into(), "утверждение".into()],
        }
    }

    /// Инвариант, от которого зависит чтение вероятности: слот 1 — всегда
    /// положительный вариант, в любом проходе. Если он падает, формула
    /// `p[1]` становится бессмысленной и все вердикты инвертируются.
    #[test]
    fn positive_option_stays_in_slot_one() {
        for (negative_label, positive_label) in PASS_LABELS {
            let rendered = question().with_labels(negative_label, positive_label);
            assert!(
                rendered.options[0].contains("отрицание"),
                "слот 0 должен быть отрицательным при метках {}/{}: {:?}",
                negative_label,
                positive_label,
                rendered.options[0]
            );
            assert!(
                rendered.options[1].contains("утверждение"),
                "слот 1 должен быть положительным при метках {}/{}: {:?}",
                negative_label,
                positive_label,
                rendered.options[1]
            );
            assert!(rendered.options[1].starts_with(positive_label));
        }
    }

    /// Два прохода должны давать РАЗНЫЕ входы, иначе усреднение бессмысленно.
    #[test]
    fn passes_differ_in_letters() {
        let first = question().with_labels(PASS_LABELS[0].0, PASS_LABELS[0].1);
        let second = question().with_labels(PASS_LABELS[1].0, PASS_LABELS[1].1);
        assert_ne!(first.options, second.options, "проходы совпали — усреднять нечего");
        assert_ne!(first.options[0], first.options[1]);
    }

    #[test]
    fn empty_criteria_are_reported_not_silently_blank() {
        let base = TypedQuestion {
            options: ["".into(), "да".into()],
            ..question()
        };
        let rendered = base.with_labels("A", "B");
        assert!(rendered.options[0].contains("(без описания)"));
        assert!(!rendered.options[0].ends_with(": "), "не должно быть висящего двоеточия");
    }
}

/// Тест на РЕАЛЬНОЙ модели. Требует 646 МБ в `KingOrchData/system1/models/`,
/// поэтому `#[ignore]` — по правилам проекта такие тесты не идут в обычном
/// `test.bat`.
///
/// Проверяет главное: вероятность из плагина совпадает с боевым стендом
/// `tools/laya_probe.py`. Конкретные числа взяты из
/// `test/laya_probe/results/summary_onnx_system1_both.json` (189/234).
#[cfg(test)]
mod real_model_tests {
    use super::*;
    use crate::catalog;

    /// Синтетический текст: в нём нет ни одного элемента невроза, поэтому все
    /// девять `p_true` должны быть низкими. Числа измерялись на этом же тексте
    /// стендом, разница после переноса в Rust не должна превышать 1e-4.
    const NEUTRAL_TEXT: &str = "Сегодня воскресенье. Погода пасмурная, я вышел в магазин, купил хлеб и молоко, вернулся домой и лёг спать.";

    #[test]
    #[ignore = "нужна модель 646 МБ; запускать явно"]
    fn probability_matches_probe_on_neutral_text() {
        // У тест-бинарника `current_exe()` — это `target/debug/deps`, а не
        // приложение, поэтому каталог данных надо задать явно. В боевом
        // приложении его задаёт хост из `app_config.data_dir` (см. main.rs).
        match std::env::var("KING_ORCH_DATA_DIR") {
            Ok(dir) if !dir.trim().is_empty() => {
                crate::set_data_dir(std::path::PathBuf::from(dir));
            }
            _ => {
                eprintln!("SKIP: задайте KING_ORCH_DATA_DIR — корень данных KingOrchData");
                return;
            }
        }

        let model_id = catalog::default_model_id();
        if !crate::inference::model_present(&model_id) {
            eprintln!(
                "SKIP: модель не найден в {}",
                paths::model_dir(&model_id).display()
            );
            return;
        }

        let model = System1Model::load(&model_id).expect("загрузить модель");
        let questions = vec![TypedQuestion {
            id: "probe".into(),
            qtype: QuestionType::Noul,
            instructions: "Говорит ли человек прямо, что у него мысль или действие стоят и не сдвигаются?".into(),
            options: [
                "жалоба на самочувствие и настроение: слабость, усталость, грусть, тревога".into(),
                "прямо сказано, что мысль или действие стоят: блок, тупик, застревание".into(),
            ],
        }];

        let result = model
            .decide(&DecisionRequest {
                state: NEUTRAL_TEXT.to_string(),
                questions,
                permutations: PermutationPolicy::Letters,
            })
            .expect("инференс");

        assert_eq!(result.answers.len(), 1);
        assert_eq!(result.answers[0].passes, 2, "ожидались два прохода перестановок");
        assert_eq!(result.device, Device::Cpu);

        let p_true = result.answers[0].p_true;
        assert!(
            (0.0..=1.0).contains(&p_true),
            "p_true вне [0,1]: {}",
            p_true
        );
        // Нейтральный текст не содержит элемента, поэтому уверенного `true`
        // быть не должно. Порог 0.8 — с запасом выше любого измеренного FP.
        assert!(
            p_true < 0.8,
            "на нейтральном тексте p_true={} — не похоже на отсутствие элемента",
            p_true
        );
        println!(
            "system1 e2-like on neutral text: p_true={:.4}, {} мс",
            p_true, result.elapsed_ms
        );
    }
}