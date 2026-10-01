//! 🩺 Диагностика падения `llama-server` НА СТАРТЕ — перевод технической ошибки
//! на человеческий язык.
//!
//! ## Зачем отдельный модуль
//!
//! `LlamaEngine::new` раньше отдавал юзеру `format!("Движок llama-server завершился
//! при запуске (код {}). {}", code, log_tail)` — то есть 50 строк ggml-спама.
//! Юзер читал это и делал вывод «программа сломалась». На деле движок падал по
//! двум вполне объяснимым причинам, и обе лечатся в Настройках.
//!
//! Существующий `diagnose_cuda_fallback` не помогает: он вызывается только ПОСЛЕ
//! успешного `/health` (ветка проверки прироста VRAM), а обе наши ошибки
//! случаются ДО health — сервер умирает, так и не поднявшись.
//!
//! ## Два источника текста (обязательно читать оба)
//!
//! CUDA/ggml-ошибки идут в **stderr** процесса (в `--log-file` их нет), а ошибки
//! инициализации KV-кэша BeeLlama — наоборот, в **log-файл**. Поэтому
//! классификатор работает по объединению `log_tail` и `stderr_lines`.
//!
//! ## Принципы
//!
//! - Никакого выдуманного диагноза: текст юзеру строится ТОЛЬКО из фактического
//!   сообщения движка + фактической конфигурации (`GpuInfo`, установленный вариант,
//!   источник бинарей).
//! - Никаких скрытых перезапусков и авто-скачивания движка: приложение
//!   сообщает, что делать, и решение принимает юзер.
//! - Текст технической ошибки НЕ теряется: полный лог уходит в `log::error!`
//!   (вкладка «Логи» + телеметрия), юзеру возвращается только `user_message`.

use std::path::Path;

/// Классифицированная причина падения движка на старте.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupFailureKind {
    /// В сборке движка нет вычислительных ядер для видеокарты
    /// (`CUDA_ERROR_NO_KERNEL_IMAGE`).
    CudaArchMissing,
    /// Драйвер NVIDIA старше, чем требует выбранная сборка CUDA.
    CudaDriverTooOld,
    /// Не хватило видеопамяти при создании контекста.
    CudaOom,
    /// BeeLlama: KV-precision-tail не поддерживается архитектурой модели.
    KvTailIncompatible,
    /// Драйвер/среда не смогли инициализировать CUDA.
    CudaInitFailed,
    /// Процесс аварийно завершился без диагностируемой причины.
    EngineCrash,
    /// Причина не распознана — вернуть вызывающему код как есть.
    Unknown,
}

/// Что нужно знать о среде, чтобы объяснить ошибку без догадок.
#[derive(Debug, Clone, Default)]
pub struct DiagnosisContext {
    /// Имя установленного GPU (может быть пустым).
    pub gpu_name: String,
    /// Архитектура GPU (`sm_61`, `не определена`).
    pub compute_label: String,
    /// Доступна ли вообще сборка `cuda-13.x` для этой карты.
    pub cuda13_supported: bool,
    /// Идентификатор источника бинарей (`ggml-org` / `beellama`).
    pub source_id: String,
    /// Человекочитаемое имя источника бинарей.
    pub source_label: String,
    /// Установленный вариант бекенда (`cuda-13.3`, `beellama`, …).
    pub variant: String,
    /// Человекочитаемое имя варианта (`cuda-13.x`, …).
    pub variant_label: String,
    /// Рекомендованный для этой карты вариант (`cuda-12.4`, `cpu`, …).
    pub recommended_variant: String,
    /// Сколько токенов «хвоста точности» KV-кэша просит источник движка
    /// (`source.runtime.kv_tail_tokens`). `0` = режим выключен, и тогда
    /// ограничений на архитектуру модели нет.
    pub kv_tail_tokens: u32,
    /// Имя модели (для текста «эта модель…»).
    pub model_name: String,
}

/// Результат диагностики: текст для юзера + короткая метка для логов.
#[derive(Debug, Clone)]
pub struct StartupDiagnosis {
    pub kind: StartupFailureKind,
    /// Текст для юзера: что произошло + что сделать. Без ggml-спама.
    pub user_message: String,
    /// Короткая метка причины для логов и телеметрии.
    pub log_hint: String,
}

/// Собрать текст диагностики из хвоста лога движка и его stderr.
///
/// `log_tail` — результат `read_log_tail(&server_log)` (файл `llama_server.log`),
/// `stderr_lines` — `ServerTrace::diagnostic_lines()`. Ни один источник не
/// является достаточным: CUDA-ошибки живут в stderr, ошибки KV-кэша — в файле.
/// Возвращает `None`, если ни один известный паттерн не распознан: тогда
/// вызывающий код сохраняет своё (нынешнее) поведение и показывает хвост лога.
pub fn diagnose_startup_failure(
    log_tail: &str,
    stderr_lines: &[String],
    exit_code: i32,
    ctx: &DiagnosisContext,
) -> Option<StartupDiagnosis> {
    let haystack = combined_log(log_tail, stderr_lines);

    // Порядок важен: частные причины проверяем раньше общих «CUDA-проблема».
    if let Some(d) = diagnose_kv_tail(&haystack, ctx) {
        return Some(d);
    }
    if let Some(d) = diagnose_no_kernel_image(&haystack, ctx) {
        return Some(d);
    }
    if let Some(d) = diagnose_driver_too_old(&haystack, ctx) {
        return Some(d);
    }
    if let Some(d) = diagnose_cuda_oom(&haystack, ctx) {
        return Some(d);
    }
    if let Some(d) = diagnose_cuda_init(&haystack, ctx) {
        return Some(d);
    }
    diagnose_crash(exit_code, &haystack, ctx)
}

/// Объединение файла лога и stderr в один регистронезависимый haystack.
fn combined_log(log_tail: &str, stderr_lines: &[String]) -> String {
    let mut out = String::with_capacity(log_tail.len() + 256);
    out.push_str(log_tail);
    for line in stderr_lines {
        if !line.is_empty() {
            out.push('\n');
            out.push_str(line);
        }
    }
    out.to_lowercase()
}

/// Движок не смог построить «хвост точности» KV-кэша.
///
/// `KV tail has no execution descriptor for layer N` — это `std::logic_error`
/// из `llama-kv-cache.cpp` форка: слой не попал в `tail_plan.layer_routes`,
/// потому что у него нет собственного execution-дескриптора для KV-хвоста.
///
/// ## Почему слоя нет — Cross-Layer Attention, а не SWA
///
/// Проверено на метаданных реальных моделей (`*.attention.shared_kv_layers`):
///
/// | модель | `block_count` | `shared_kv_layers` | BeeLlama |
/// |---|---|---|---|
/// | Gemma 4 E4B  | 42 | **18** | падает на слое 24 = `42 - 18` |
/// | Gemma 4 12B  | 48 | 0    | работает |
///
/// `42 - 18 = 24` — ровно тот слой, на котором движок спотыкается: это первый
/// слой, чей KV-кэш **общий** с предыдущими. Скользящее окно (SWA) тут ни при
/// чём — ключи `sliding_window`/`key_length_swa` присутствуют у обеих моделей,
/// просто с разными значениями (512 против 1024).
///
/// То есть несовместим движок, а не модель: официальный `ggml-org/llama.cpp`
/// Cross-Layer Attention понимает и такие модели запускает нормально.
fn diagnose_kv_tail(haystack: &str, ctx: &DiagnosisContext) -> Option<StartupDiagnosis> {
    let is_tail_error = haystack.contains("kv tail has no execution descriptor")
        || (haystack.contains("kv tail") && haystack.contains("execution descriptor"))
        || (haystack.contains("kv tail") && haystack.contains("failed to initialize the context"));
if !is_tail_error {
        return None;
    }
    Some(kv_tail_diagnosis(ctx, "after_crash", None))
}

/// Публичная проверка ДО запуска движка: ловит ту же несовместимость по
/// метаданным GGUF, не дожидаясь падения.
///
/// Вызывается из `llm.rs` перед spawn. Причина не в тексте лога движка, а в
/// самой модели: `kv_tail_tokens > 0` (BeeLlama) + `*.attention.shared_kv_layers > 0`
/// = детерминированная несовместимость, которая иначе стоит юзеру двух
/// неудачных запусков (~3 с) и сырого трейса в логах.
///
/// Критерий читается из GGUF, а не из имени файла, поэтому работает на любой
/// модели с совместным KV-кэшем — без хардкода конкретных имён.
pub fn diagnose_shared_kv_before_spawn(model_path: &str, ctx: &DiagnosisContext) -> Option<StartupDiagnosis> {
    if ctx.kv_tail_tokens == 0 {
        return None;
    }
    // Ключ лежит под префиксом архитектуры (`gemma4.attention.shared_kv_layers`),
    // поэтому архитектуру читаем из самого файла, а не хардкодим.
    let arch = crate::engine::llm_gguf::extract_gguf_arch(model_path);
    let shared = crate::engine::llm_gguf::extract_u32_with_arch(
        model_path,
        arch.as_deref(),
        "attention.shared_kv_layers",
    )?;
    if shared == 0 {
        return None;
    }
    Some(kv_tail_diagnosis(ctx, "preflight", Some(shared)))
}

/// Общий текст для обоих путей (реактивного по логу и превентивного по GGUF),
/// чтобы юзер в обоих случаях читал одно и то же объяснение.
///
/// `shared_layers` — точное число слоёв с общим KV из метаданных GGUF. Есть
/// только у превентивного пути: у реактивного движок сообщает лишь номер
/// слоя, на котором споткнулся, и без знания архитектуры модели мы это число
/// не восстановим.
fn kv_tail_diagnosis(ctx: &DiagnosisContext, detected: &str, shared_layers: Option<u32>) -> StartupDiagnosis {
    let model = if ctx.model_name.is_empty() { "эта модель".to_string() } else { ctx.model_name.clone() };
    let arch_note = match shared_layers {
        Some(n) => format!(" В метаданных модели указано: {} слоёв используют общий KV-кэш.", n),
        None => String::new(),
    };
    let reason = format!(
        "Модель исправна, несовместим именно движок. {} запускает модель с «хвостом точности» \
         KV-кэша (--kv-tail-tokens {} — режим KVarN).{} У части слоёв этой модели KV-кэш общий с \
         предыдущими (архитектура Cross-Layer Attention), а у таких слоёв нет собственного \
         дескриптора для KV-хвоста — движок на этом прерывает запуск.",
        ctx.source_label, ctx.kv_tail_tokens, arch_note
    );
    let remedy = "Что сделать:\n\
         1. Настройки → «Движок запуска нейромоделей» → источник: выберите «Официальный llama.cpp (ggml-org)».\n\
         2. Нажмите «Установить» и повторите запрос — официальный движок такие модели поддерживает.\n\
         3. Если нужен именно BeeLlama — дождитесь обновления: поддержка Cross-Layer Attention в нём ещё не исправлена.\n\
         Совет: BeeLlama экономит видеопамять, но поддерживает не все архитектуры моделей. Для незнакомых моделей надёжнее официальный движок."
        .to_string();
    StartupDiagnosis {
        kind: StartupFailureKind::KvTailIncompatible,
        user_message: format!("Движок не смог запустить модель «{}».\n\n{}\n\n{}", model, reason, remedy),
        log_hint: format!(
            "kv_tail_incompatible: detected={} source={} variant={} kv_tail={} shared_kv_layers={} model={}",
            detected,
            ctx.source_id,
            ctx.variant,
            ctx.kv_tail_tokens,
            shared_layers.map(|n| n.to_string()).unwrap_or_else(|| "unknown".into()),
            ctx.model_name
        ),
    }
}

/// В сборке движка нет ядер для видеокарты.
///
/// `CUDA error: no kernel image is available for execution on the device`
/// (CUDA_ERROR_NO_KERNEL_IMAGE, код 209) означает, что в бинарнике нет ни
/// cubin, ни совместимого PTX для архитектуры карты. Две противоположные
/// ситуации, и лечатся они противоположно:
/// - на карте младше Turing (sm_50/61/70) стоит сборка `cuda-13.x`, в которой
///   ядра есть только от sm_75 → нужен `cuda-12.4`;
/// - на Blackwell (sm_120) стоит `cuda-12.4`, в которой нет sm_120 → нужен `cuda-13.x`.
fn diagnose_no_kernel_image(haystack: &str, ctx: &DiagnosisContext) -> Option<StartupDiagnosis> {
    let arch_missing = haystack.contains("no kernel image")
        || haystack.contains("cudaerrornokernelimage")
        || haystack.contains("cuda_error_no_kernel_image");
    if !arch_missing {
        return None;
    }

    let on_cuda13 = is_cuda13(ctx.variant.as_str());
    let gpu = if ctx.gpu_name.is_empty() { "ваша видеокарта".to_string() } else { ctx.gpu_name.clone() };
    let arch = ctx.compute_label.as_str();
    let model = if ctx.model_name.is_empty() { "модель".to_string() } else { ctx.model_name.clone() };

    let (reason, remedy, hint) = if on_cuda13 && !ctx.cuda13_supported {
        (
            format!(
                "Видеокарта «{}» имеет архитектуру {}, а установленная сборка «{}» собрана только \
                 под Turing и новее (sm_75+). Ядер для вашей карты в ней физически нет, поэтому \
                 видеокарта не смогла запустить вычисления и движок упал. Это ограничение сборки \
                 движка, а не проблема драйвера и не поломка программы.",
                gpu, arch, ctx.variant_label
            ),
            format!(
                "Что сделать:\n\
                 1. Настройки → «Движок запуска нейромоделей».\n\
                 2. Установите вариант «{}» (он поддерживает вашу видеокарту), затем повторите запрос.\n\
                 Если вариант «{}» уже установлен — просто переключитесь на него.",
                ctx.recommended_variant, ctx.recommended_variant
            ),
            format!(
                "cuda_arch_missing: too_old_gpu variant={} arch={} recommended={}",
                ctx.variant, arch, ctx.recommended_variant
            ),
        )
    } else if !on_cuda13 && !ctx.cuda13_supported {
        (
            format!(
                "Установленная сборка «{}» не содержит ядер для вашей видеокарты «{}» ({}).",
                ctx.variant_label, gpu, arch
            ),
            format!(
                "Что сделать: установите и выберите вариант «{}» (Настройки → «Движок запуска \
                 нейромоделей»).",
                ctx.recommended_variant
            ),
            format!("cuda_arch_missing: variant={} arch={}", ctx.variant, arch),
        )
    } else if !on_cuda13 {
        // Turing+ / Ampere / Ada на cuda-12.x — ядра в ней есть, значит причина
        // в чём-то ещё; не выдумываем версию, а указываем на следующий шаг.
        (
            format!(
                "Драйвер не смог запустить вычисления на видеокарте «{}» ({}): в сборке «{}» нет \
                 подходящего машинного кода.",
                gpu, arch, ctx.variant_label
            ),
            format!(
                "Что сделать: обновите драйвер NVIDIA до последней версии и повторите запрос. \
                 Если не поможет — установите вариант «{}».",
                ctx.recommended_variant
            ),
            format!("cuda_arch_missing: unknown arch={} variant={}", arch, ctx.variant),
        )
    } else {
        return None;
    };

    Some(StartupDiagnosis {
        kind: StartupFailureKind::CudaArchMissing,
        user_message: format!(
            "Движок «{}» не смог запустить модель «{}» на видеокарте.\n\n{}\n\n{}",
            ctx.variant_label, model, reason, remedy
        ),
        log_hint: hint,
    })
}

/// Драйвер старше, чем требует сборка CUDA.
///
/// Здесь НЕ проверяем архитектуру карты: несовместимость состава сборки — это
/// другая причина, и её ловит `diagnose_no_kernel_image` по своему маркеру.
/// Сообщение «driver version is insufficient» само по себе означает именно
/// недостаточную свежесть драйвера, поэтому и лечится обновлением драйвера.
fn diagnose_driver_too_old(haystack: &str, ctx: &DiagnosisContext) -> Option<StartupDiagnosis> {
    let too_old = haystack.contains("driver version is insufficient")
        || haystack.contains("cudaerrorinsufficientdriver")
        || haystack.contains("cuda driver version is insufficient");
    if !too_old {
        return None;
    }
    let min = if is_cuda13(ctx.variant.as_str()) { "580" } else { "527.41" };
    Some(StartupDiagnosis {
        kind: StartupFailureKind::CudaDriverTooOld,
        user_message: format!(
            "Драйвер видеокарты «{}» слишком старый для сборки движка «{}».\n\n\
             Что сделать:\n\
             1. Обновите драйвер NVIDIA до версии {}+ (официальный сайт nvidia.com → «Драйверы»).\n\
             2. После перезагрузки повторите запрос.\n\
             Обновлять нужно именно драйвер видеокарты, а не программу.",
            if ctx.gpu_name.is_empty() { "NVIDIA".to_string() } else { ctx.gpu_name.clone() },
            ctx.variant_label,
            min
        ),
        log_hint: format!(
            "cuda_driver_too_old: variant={} required_driver={}+",
            ctx.variant, min
        ),
    })
}

/// Не хватило видеопамяти при создании контекста (в т.ч. на pre-flight).
fn diagnose_cuda_oom(haystack: &str, ctx: &DiagnosisContext) -> Option<StartupDiagnosis> {
    let oom = haystack.contains("out of memory")
        || haystack.contains("cuda_malloc failed")
        || haystack.contains("cumemalloc")
        || (haystack.contains("not enough memory") && haystack.contains("memory"));
    if !oom {
        return None;
    }
    Some(StartupDiagnosis {
        kind: StartupFailureKind::CudaOom,
        user_message: format!(
            "Модель «{}» не поместилась в видеопамять.\n\n\
             Что сделать (по порядку):\n\
             1. Уменьшите размер контекста в Настройках (например с 24576 до 8192).\n\
             2. Возьмите модель полегче (меньше квантизация Q4 → Q3, либо меньше параметров).\n\
             3. Либо переключитесь на вариант «{}» движка: он экономнее расходует видеопамять.",
            if ctx.model_name.is_empty() { "эта модель".to_string() } else { ctx.model_name.clone() },
            ctx.recommended_variant
        ),
        log_hint: format!("cuda_oom: variant={} model={}", ctx.variant, ctx.model_name),
    })
}

/// CUDA не инициализировалась (проблема драйвера/среды, а не состава сборки).
fn diagnose_cuda_init(haystack: &str, ctx: &DiagnosisContext) -> Option<StartupDiagnosis> {
    let failed = haystack.contains("failed to initialize cuda")
        || haystack.contains("cuda error: initialization error")
        || haystack.contains("cudaerrornodevice")
        || (haystack.contains("cuda") && haystack.contains("initialization"));
    if !failed {
        return None;
    }
    Some(StartupDiagnosis {
        kind: StartupFailureKind::CudaInitFailed,
        user_message: format!(
            "Драйвер видеокарты «{}» не смог инициализировать вычисления на GPU.\n\n\
             Что сделать: обновите драйвер NVIDIA до последней версии (nvidia.com → «Драйверы») и \
             перезагрузите компьютер. Если не поможет — переключитесь на вариант «{}» или CPU.",
            if ctx.gpu_name.is_empty() { "NVIDIA".to_string() } else { ctx.gpu_name.clone() },
            ctx.recommended_variant
        ),
        log_hint: format!("cuda_init_failed: variant={} gpu={}", ctx.variant, ctx.gpu_name),
    })
}

/// Аварийное завершение процесса без распознанной причины.
///
/// Windows-код `-1073740791` = `0xC0000409` (STATUS_STACK_BUFFER_OVERRUN,
/// он же fail-fast `__fastfail`). Сам по себе он ничего не говорит о причине,
/// поэтому текст строим вокруг того, что удалось вытащить из stderr.
fn diagnose_crash(exit_code: i32, haystack: &str, ctx: &DiagnosisContext) -> Option<StartupDiagnosis> {
    if exit_code != FAIL_FAST_EXIT_CODE && exit_code != 1 {
        return None;
    }
    // Если в тексте есть хоть что-то говорящее про ошибку CUDA/KV — это уже
    // разобрано выше; сюда попадаем только с «пустым» логом.
    let clue = first_error_line(haystack);
    Some(StartupDiagnosis {
        kind: StartupFailureKind::EngineCrash,
        user_message: format!(
            "Движок «{}» аварийно завершился при запуске модели{}{}.\n\n\
             Подробности — в логах приложения (вкладка «Логи»).\n\
             Что попробовать: переустановить движок (Настройки → «Движок запуска нейромоделей» → \
             «Переустановить»), затем повторить запрос.",
            ctx.variant_label,
            if ctx.model_name.is_empty() { String::new() } else { format!(" «{}»", ctx.model_name) },
            if ctx.recommended_variant != ctx.variant {
                format!(" Рекомендуемый для вашей видеокарты вариант — «{}».", ctx.recommended_variant)
            } else {
                String::new()
            }
        ),
        log_hint: format!(
            "engine_crash: exit={} variant={} source={} clue={:?}",
            exit_code, ctx.variant, ctx.source_id, clue
        ),
    })
}

/// Первая строка из лога, где есть признак проблемы — для лога/телеметрии.
fn first_error_line(haystack: &str) -> Option<String> {
    haystack
        .lines()
        .find(|l| {
            let l = l.to_lowercase();
            l.contains("error") || l.contains("failed") || l.contains("fatal")
        })
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
}

/// `0xC0000409` в знаковом виде — exit code Windows для fail-fast.
const FAIL_FAST_EXIT_CODE: i32 = -1073740791;

/// Вариант относится к ветке cuda-13.x.
fn is_cuda13(variant: &str) -> bool {
    variant.contains("13")
}

/// Собрать контекст диагностики из фактического состояния движка.
/// Вынесено отдельно, чтобы `llm.rs` не знал про ассортимент полей.
pub fn build_context(
    gpu: &crate::engine::gpu_detector::GpuInfo,
    variant: &str,
    variant_label: &str,
    source_id: &str,
    source_label: &str,
    kv_tail_tokens: u32,
    recommended_variant: &str,
    model_path: &str,
) -> DiagnosisContext {
    use crate::engine::gpu_detector::{compute_label, cuda13_arch_supported};
    DiagnosisContext {
        gpu_name: gpu.gpu_name.clone(),
        compute_label: compute_label(gpu),
        cuda13_supported: cuda13_arch_supported(gpu),
        source_id: source_id.to_string(),
        source_label: source_label.to_string(),
        variant: variant.to_string(),
        variant_label: variant_label.to_string(),
        kv_tail_tokens,
        recommended_variant: recommended_variant.to_string(),
        model_name: std::path::Path::new(model_path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| model_path.to_string()),
    }
}

/// Сохранить полный технический след в лог-файл приложения (вкладка «Логи»).
/// Вызывается перед возвратом `user_message`, чтобы ни один диагностический
/// признак не потерялся.
pub fn log_full_trace(log_path: &Path, log_tail: &str, stderr_lines: &[String], diag: &StartupDiagnosis) {
    log::error!("[llama-engine] startup failure ({:?}): {}", diag.kind, diag.log_hint);
    log::error!("[llama-engine] exit_code={} log_path={}", diag.log_hint, log_path.display());
    if !log_tail.trim().is_empty() {
        log::error!("[llama-engine] ---- llama_server.log (tail) ----\n{}", log_tail.trim_end());
    }
    let stderr_tail: Vec<&String> = stderr_lines.iter().filter(|l| !l.trim().is_empty()).collect();
    if !stderr_tail.is_empty() {
        log::error!("[llama-engine] ---- llama-server stderr (tail) ----");
        for line in stderr_tail {
            log::error!("[llama-server] {}", line.trim_end());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beellama_ctx() -> DiagnosisContext {
        DiagnosisContext {
            source_id: "beellama".into(),
            source_label: "BeeLlama (KVarN, меньше VRAM)".into(),
            variant: "cuda-12.4".into(),
            variant_label: "cuda-12.x".into(),
            kv_tail_tokens: 1024,
            model_name: "gemma-4-E4B-it-heretic-QAT-UD-Q4_K_XL.gguf".into(),
            ..DiagnosisContext::default()
        }
    }

    fn pascal_ctx() -> DiagnosisContext {
        DiagnosisContext {
            gpu_name: "NVIDIA GeForce GTX 1050 Ti".into(),
            compute_label: "sm_6".into(),
            cuda13_supported: false,
            source_id: "ggml-org".into(),
            source_label: "Официальный llama.cpp (ggml-org)".into(),
            variant: "cuda-13.3".into(),
            variant_label: "cuda-13.x".into(),
            recommended_variant: "cuda-12.4".into(),
            model_name: "gemma-4-E4B-it-heretic-QAT-UD-Q4_K_XL.gguf".into(),
            ..DiagnosisContext::default()
        }
    }

    // ── Реальные логи инцидента ──

    #[test]
    fn detects_beellama_kv_tail_failure_from_log_file() {
        // Ровно то, что прислал юзер: ошибка в llama_server.log, stderr пустой.
        let log_tail = "\
I common_param: verbosity = 3
E llama_init_from_model: failed to initialize the context: KV tail has no execution descriptor for layer 24
E common_fit_params: encountered an error while trying to fit params to free device memory: failed to create llama_context from model
W load: control-looking token: 50 '<|tool_response>' was not control-type
E common_init_: failed to create context with model 'gemma-4-E4B-it-heretic-QAT-UD-Q4_K_XL.gguf'
E srv llama_server: exiting due to model loading error";
        let d = diagnose_startup_failure(log_tail, &[], 1, &beellama_ctx()).expect("диагноз обязателен");
        assert_eq!(d.kind, StartupFailureKind::KvTailIncompatible);
        assert!(
            d.user_message.contains("Cross-Layer Attention"),
            "нет объяснения про совместный KV-кэш: {}",
            d.user_message
        );
        // Юзер должен понимать, что виноват движок, а не модель и не драйвер.
        assert!(
            d.user_message.contains("Модель исправна, несовместим именно движок"),
            "нет однозначного виновника: {}",
            d.user_message
        );
        assert!(d.user_message.contains("ggml-org"), "не предложен официальный источник: {}", d.user_message);
        assert!(d.log_hint.contains("detected=after_crash"));
        // Юзеру не показываем внутренности движка
        assert!(!d.user_message.contains("llama_init_from_model"));
        assert!(!d.user_message.contains("common_fit_params"));
        assert!(d.log_hint.contains("kv_tail_incompatible"));
    }

    #[test]
    fn kv_tail_message_never_mentions_sliding_window() {
        // Регрессия на неверную формулировку: причина — совместный KV-кэш между
        // слоями, а НЕ скользящее окно. Ключи sliding_window есть у обеих
        // моделей (и у падающей, и у работающей), различие только в
        // shared_kv_layers. Упоминание SWA уводит юзера не туда.
        let d = diagnose_startup_failure(
            "E llama_init_from_model: failed to initialize the context: KV tail has no execution descriptor for layer 24",
            &[],
            1,
            &beellama_ctx(),
        )
        .unwrap();
        let lower = d.user_message.to_lowercase();
        assert!(!lower.contains("скользящ"), "SWA в объяснении быть не должно: {}", d.user_message);
        assert!(!lower.contains("swa"), "SWA в объяснении быть не должно: {}", d.user_message);
    }

    #[test]
    fn preflight_skips_model_when_kv_tail_disabled() {
        // Официальный движок (kv_tail_tokens = 0) запускает ту же модель без
        // проблем, поэтому проверка обязана молчать, даже если файл читается.
        let mut ctx = beellama_ctx();
        ctx.kv_tail_tokens = 0;
        let d = diagnose_shared_kv_before_spawn(NONEXISTENT_GGUF, &ctx);
        assert!(d.is_none(), "без KV-хвоста проверка не должна срабатывать");
    }

    #[test]
    fn preflight_stays_silent_when_model_metadata_absent() {
        // Модель без `shared_kv_layers` (или не-GGUF) — проверять нечего.
        let d = diagnose_shared_kv_before_spawn(NONEXISTENT_GGUF, &beellama_ctx());
        assert!(d.is_none(), "нет метаданных — диагноз не выдумываем");
    }

    #[test]
    fn preflight_reports_real_shared_kv_model() {
        // Настоящий файл Gemma 4 E4B: shared_kv_layers = 18, block_count = 42,
        // движок спотыкается ровно на слое 42 - 18 = 24.
        if !std::path::Path::new(SHARED_KV_GGUF).exists() {
            eprintln!("skip: тестовая GGUF не найдена");
            return;
        }
        let d = diagnose_shared_kv_before_spawn(SHARED_KV_GGUF, &beellama_ctx())
            .expect("модель с совместным KV обязана ловиться до запуска");
        assert_eq!(d.kind, StartupFailureKind::KvTailIncompatible);
        assert!(d.user_message.contains("Cross-Layer Attention"), "{}", d.user_message);
        // Точное число слоёв из метаданных — конкретнее, чем номер слоя из лога.
        assert!(d.user_message.contains("18"), "не названо число общих слоёв: {}", d.user_message);
        assert!(d.user_message.contains("ggml-org"), "не предложен официальный источник: {}", d.user_message);
        assert!(d.log_hint.contains("detected=preflight"));
        // Никакого spawn: юзер не должен платить за падение движка.
        assert!(!d.user_message.contains("автоматически"));
    }

    #[test]
    fn preflight_allows_model_without_shared_kv() {
        // Gemma 4 12B: shared_kv_layers = 0 → BeeLlama её запускает.
        if !std::path::Path::new(NO_SHARED_KV_GGUF).exists() {
            eprintln!("skip: тестовая GGUF не найдена");
            return;
        }
        let d = diagnose_shared_kv_before_spawn(NO_SHARED_KV_GGUF, &beellama_ctx());
        assert!(d.is_none(), "модель без совместного KV запускается в BeeLlama");
    }

    /// Заведомо несуществующий путь: проверки на отсутствии файла.
    const NONEXISTENT_GGUF: &str = r"Z:\definitely\not\here\model.gguf";
    /// Gemma 4 E4B — 18 слоёв с общим KV (падает на BeeLlama).
    const SHARED_KV_GGUF: &str =
        r"D:\nn\models\llm\uncen\gemma-4-8b\gemma-4-E4B-it-heretic-QAT-UD-Q4_K_XL.gguf";
    /// Gemma 4 12B — shared_kv_layers = 0 (работает на BeeLlama).
    const NO_SHARED_KV_GGUF: &str =
        r"D:\nn\models\llm\uncen\gemma-4-12B\gemma-4-12B-it-qat-q4_0-unquantized-heretic-ja-v2.i1-IQ4_NL.gguf";

    #[test]
    fn detects_no_kernel_image_from_stderr_on_pascal_with_cuda13() {
        // CUDA-ошибка приходит в STDERR, а не в log-файл (реальный случай юзера).
        let stderr = vec![
            "I init: llama threadpool init, n_threads = 4".to_string(),
            "E CUDA error: no kernel image is available for execution on the device".to_string(),
        ];
        let d = diagnose_startup_failure("I srv init: The UI is disabled", &stderr, -1073740791, &pascal_ctx())
            .expect("диагноз обязателен");
        assert_eq!(d.kind, StartupFailureKind::CudaArchMissing);
        assert!(d.user_message.contains("sm_6"), "не назван архитектуру: {}", d.user_message);
        assert!(d.user_message.contains("cuda-12.4"), "не назван нужный вариант: {}", d.user_message);
        assert!(
            d.user_message.contains("не проблема драйвера"),
            "юзер должен понимать, что это не драйвер: {}",
            d.user_message
        );
        // Никакой «автоматической починки» в тексте быть не должно
        assert!(!d.user_message.contains("автоматически"));
        assert!(d.log_hint.contains("too_old_gpu"));
    }

    #[test]
    fn no_kernel_image_never_blames_driver_on_old_arch() {
        let stderr = vec!["E CUDA error: no kernel image is available for execution on the device".to_string()];
        let d = diagnose_startup_failure("", &stderr, -1073740791, &pascal_ctx()).unwrap();
        // Диагноз про сборку, а не про драйвер: иначе юзер пойдёт качать драйверы
        assert_ne!(d.kind, StartupFailureKind::CudaDriverTooOld);
        assert!(!d.user_message.to_lowercase().contains("обновите драйвер"));
    }

    #[test]
    fn driver_too_old_is_reported_as_a_driver_problem_regardless_of_arch() {
        // «driver version is insufficient» — само по себе означает
        // недостаточную свежесть драйвера. Несовместимость состава сборки — это
        // другой маркер (`no kernel image`), и смешивать их нельзя: иначе
        // юзеру скажут «обновите драйвер» там, где нужно сменить вариант.
        let mut ctx = pascal_ctx();
        ctx.variant = "cuda-12.4".into();
        ctx.variant_label = "cuda-12.x".into();
        let log = "E CUDA error: driver version is insufficient for CUDA runtime version";
        let d = diagnose_startup_failure(log, &[], 1, &ctx).expect("диагноз обязателен");
        assert_eq!(d.kind, StartupFailureKind::CudaDriverTooOld);
        assert!(d.user_message.contains("527.41"), "не назван минимальный драйвер: {}", d.user_message);
    }

    #[test]
    fn driver_too_old_on_cuda13_names_r580() {
        let mut ctx = pascal_ctx();
        ctx.cuda13_supported = true;
        ctx.compute_label = "sm_8".into();
        let d = diagnose_startup_failure("E CUDA error: driver version is insufficient", &[], 1, &ctx).unwrap();
        assert!(d.user_message.contains("580"), "для cuda-13.x нужен 580+: {}", d.user_message);
    }

    #[test]
    fn cuda_oom_explains_what_to_shrink() {
        let ctx = pascal_ctx();
        let d = diagnose_startup_failure("E ggml_cuda: out of memory\n  failed to allocate 512 MB", &[], 1, &ctx)
            .expect("диагноз обязателен");
        assert_eq!(d.kind, StartupFailureKind::CudaOom);
        assert!(d.user_message.contains("контекст"), "не сказано про контекст: {}", d.user_message);
    }

    #[test]
    fn crash_without_markers_still_gives_actionable_text() {
        let d = diagnose_startup_failure("", &["boom".to_string()], -1073740791, &pascal_ctx())
            .expect("диагноз обязателен");
        assert_eq!(d.kind, StartupFailureKind::EngineCrash);
        assert!(d.user_message.contains("Переустановить"), "нет действия для юзера: {}", d.user_message);
        assert!(d.user_message.contains("cuda-12.4"), "не подсказан вариант: {}", d.user_message);
    }

    #[test]
    fn unknown_failure_returns_none_so_caller_keeps_its_own_behaviour() {
        let ctx = pascal_ctx();
        assert!(diagnose_startup_failure("I everything is fine", &[], 0, &ctx).is_none());
        assert!(diagnose_startup_failure("I everything is fine", &[], -1, &ctx).is_none());
    }

    #[test]
    fn is_cuda13_matches_variants() {
        assert!(is_cuda13("cuda-13.3"));
        assert!(is_cuda13("cuda-13.7"));
        assert!(!is_cuda13("cuda-12.4"));
        assert!(!is_cuda13("cpu"));
        assert!(!is_cuda13("vulkan"));
    }

    #[test]
    fn build_context_reads_real_gpu_and_model_file_name() {
        let gpu = crate::engine::gpu_detector::GpuInfo {
            has_nvidia: true,
            gpu_name: "NVIDIA GeForce GTX 1060 6GB".into(),
            cuda_major: 13,
            cuda_minor: 0,
            driver_version: "580.00".into(),
            compute_major: 6,
            compute_minor: 1,
        };
        let ctx = build_context(
            &gpu,
            "cuda-13.3",
            "cuda-13.x",
            "ggml-org",
            "Официальный llama.cpp (ggml-org)",
            0,
            "cuda-12.4",
            r"C:\llm_local_ai_models\gemma-4-E4B-it-heretic-QAT-UD-Q4_K_XL.gguf",
        );
        assert_eq!(ctx.compute_label, "sm_6");
        assert!(!ctx.cuda13_supported);
        assert_eq!(ctx.model_name, "gemma-4-E4B-it-heretic-QAT-UD-Q4_K_XL.gguf");
        assert!(!ctx.gpu_name.is_empty());
    }

    #[test]
    fn kv_tail_wins_over_generic_cuda_patterns() {
        // Если в логе есть и KV-tail, и упоминание cuda — причина одна,
        // и правильная — KV-tail (иначе юзеру скажут «проблема с видеокартой»).
        let log = "\
E llama_init_from_model: failed to initialize the context: KV tail has no execution descriptor for layer 24
E common_fit_params: encountered an error while trying to fit params to free device memory
I ggml_cuda_init: found 1 CUDA devices";
        let d = diagnose_startup_failure(log, &[], 1, &beellama_ctx()).unwrap();
        assert_eq!(d.kind, StartupFailureKind::KvTailIncompatible);
    }
}