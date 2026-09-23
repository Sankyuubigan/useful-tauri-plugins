//! Единый источник правды по оценке VRAM-потребления модели.
//!
//! Все прогнозы (pre-flight запуска, пик-линия после генерации, счётчик под
//! полем ввода в чате) считаются ТОЛЬКО здесь. Отдельные формулы в llm.rs —
//! запрещены: любое расхождение между «что юзер видит в интерфейсе» и «что
//! писал движку в лог» — баг (см. фикс фейкового VRAM-диалога, tasks/).
//!
//! KV-кэш считается по слоям внимания с учётом:
//!   - массивов `attention.head_count_kv` (per-layer GQA — gemma4 и др.);
//!   - массивов `attention.sliding_window_pattern` — НАСТОЯЩИЙ признак SWA-слоёв.
//!     У gemma4 наоборот, чем в классических гибридах: SWA-слои несут БОЛЬШЕ
//!     KV-голов (8), а dense — меньше (1). Поэтому граница «SWA vs global» не
//!     выводится из числа голов, а читается из GGUF-паттерна;
//!   - SWA-размерностей `attention.key_length_swa`/`value_length_swa`;
//!   - legacy-скаляров + `full_attention_interval` (гибриды qwen35).
//!
//! KV-кэш двухкомпонентный (повторяет llama-kv-cache-iswa.cpp):
//!   - dense (non-SWA) слои видят ПОЛНЫЙ контекст: `PAD(ctx, 256)` ячеек;
//!   - SWA-слои ограничены sliding_window: `PAD(min(ctx, n_swa·n_seq_max + n_ubatch), 256)`.
//!     Приложение запускает llama-server с авто-параметрами (n_parallel=4,
//!     kv_unified=true), поэтому n_seq_max=4, n_ubatch=512.
//!
//! `ctx_size` — ЭФФЕКТИВНЫЙ контекст (промпт + запас генерации). Вопреки более
//! ранним комментариям llama.cpp выделяет KV-кэш НЕ лениво: при создании
//! контекста резервируются все ячейки `n_ctx_seq`. Pre-flight считает по полному
//! лимиту, счётчик под полем ввода — по занятому промпту + запасу (оценка UX).
//!
//! KV-квант источника (BeeLlama KVarN `kvarn5/kvarn4` + precision tail 1024)
//! учитывается через `KvQuantSpec` / `kv_spec_from_source`: оценка должна
//! знать РЕАЛЬНЫЙ cache-type иначе pre-flight занижает `-ngl` до уровня
//! vanilla llama.cpp и выгрыш KVarN в GPU-слоях не работает.

use crate::engine::llm_gguf::{extract_gguf_arch, extract_u32_with_arch, extract_i64_array_with_arch};
use crate::engine::sources::SourceSpec;

const MIB: f64 = 1024.0 * 1024.0;

/// Оценка VRAM-потребления для контекста `ctx_size`.
#[derive(Debug, Clone, Copy)]
pub struct VramEstimate {
    /// Размер GGUF-файла (веса модели) в МБ.
    pub model_mb: f64,
    /// KV-кэш на 1 токен для dense (non-SWA) слоёв, МБ.
    pub kv_per_token_mb: f64,
    /// KV-кэш на указанный контекст (dense + SWA), МБ.
    pub kv_mb: f64,
    /// SWA-часть KV-кэша (ячейки ограничены sliding_window), МБ.
    pub kv_swa_mb: f64,
    /// Workspace CUDA-буферы (compute), МБ.
    pub buffers_mb: f64,
    /// Итого (модель + KV + буферы), МБ.
    pub total_mb: f64,
    /// Число блоков модели (для линейного пересчёта по -ngl).
    pub num_layers: u32,
    /// Число слоёв, несущих KV (≤ num_layers для гибридов).
    pub num_attn_layers: u32,
}

/// Байт-вес на элемент KV-кэша: FP16 = 2.0, q8_0 = 34/32 = 1.0625 (как в llama.cpp).
fn kv_bytes_per_elem(quant: bool) -> f64 {
    if quant { 34.0 / 32.0 } else { 2.0 }
}

/// Спецификация KV-кванта для оценки VRAM (реальный cache-type источника).
///
/// `k_bytes`/`v_bytes` — байт на элемент тела кэша; `tail_tokens` — число
/// последних токенов, которые движок хранит в F16 (`--kv-tail-tokens`).
/// Для BeeLlama: `kvarn5/kvarn4` ≈ 5.375/4.375 bpv, tail 1024 (см.
/// `engine_sources.json` → `runtime`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KvQuantSpec {
    pub k_bytes: f64,
    pub v_bytes: f64,
    pub tail_tokens: u32,
}

impl KvQuantSpec {
    /// Без кванта (vanilla llama.cpp / чекбокс выключен).
    pub fn fp16() -> Self {
        Self { k_bytes: 2.0, v_bytes: 2.0, tail_tokens: 0 }
    }

    /// Дефолтный KV-spec источника: legacy-чекбоксы → q8_0/f16, без tail.
    pub fn from_legacy_flags(kv_quant_keys: bool, kv_quant_values: bool) -> Self {
        Self {
            k_bytes: kv_bytes_per_elem(kv_quant_keys),
            v_bytes: kv_bytes_per_elem(kv_quant_values),
            tail_tokens: 0,
        }
    }
}

/// Байт на элемент для cache-type строки llama.cpp/BeeLlama.
/// bpv (bits per value) из README Anbeeld → байты = bpv / 8.
/// Неизвестный тип → None (вызывающий берёт консервативный F16).
pub fn cache_type_bytes(cache_type: &str) -> Option<f64> {
    let ct = cache_type.trim().to_ascii_lowercase();
    let bpv: f64 = match ct.as_str() {
        "f16" | "bf16" => 16.0,
        "q8_0" => 8.5,
        "q6_0" => 6.5,
        "q5_0" => 5.5,
        "q4_1" => 5.0,
        "q4_0" => 4.5,
        "q3_1" => 4.0,
        "q3_0" => 3.5,
        "q2_1" => 3.0,
        "q2_0" => 2.5,
        "kvarn8" => 8.375,
        "kvarn6" => 6.375,
        "kvarn5" => 5.375,
        "kvarn4" => 4.375,
        "kvarn3" => 3.375,
        "kvarn2" => 2.375,
        _ => return None,
    };
    Some(bpv / 8.0)
}

/// Резолв KV-spec: runtime источника (KVarN для BeeLlama) ПЕРЕКРЫВАЕТ
/// legacy-чекбоксы `kv_quant_*` — тот же приоритет, что в `build_cmd`
/// (`llm.rs`: `--cache-type-k` из `source.runtime` иначе q8_0).
pub fn kv_spec_from_source(
    source: Option<&SourceSpec>,
    kv_quant_keys: bool,
    kv_quant_values: bool,
) -> KvQuantSpec {
    if let Some(spec) = source {
        let rt = &spec.runtime;
        if rt.cache_type_k.is_some() || rt.cache_type_v.is_some() {
            let k_bytes = rt
                .cache_type_k
                .as_deref()
                .and_then(cache_type_bytes)
                .unwrap_or(2.0);
            let v_bytes = rt
                .cache_type_v
                .as_deref()
                .and_then(cache_type_bytes)
                .unwrap_or(2.0);
            return KvQuantSpec {
                k_bytes,
                v_bytes,
                tail_tokens: rt.kv_tail_tokens.unwrap_or(0),
            };
        }
    }
    KvQuantSpec::from_legacy_flags(kv_quant_keys, kv_quant_values)
}

/// KV-spec для текущего `engine_source` из `app_config.json` (без AppHandle).
/// Для call-site'ов, где `SourceSpec` уже в скоупе, — предпочитать
/// `kv_spec_from_source` (без повторного чтения конфига).
pub fn current_kv_spec(kv_quant_keys: bool, kv_quant_values: bool) -> KvQuantSpec {
    let cfg = crate::engine::config::load_config_early();
    let sid = crate::engine::sources::resolve_source(cfg.engine_source.as_deref());
    kv_spec_from_source(
        crate::engine::sources::source_spec(&sid).as_ref(),
        kv_quant_keys,
        kv_quant_values,
    )
}

/// Раскладка KV-кэша на две компоненты (dense — полный ctx, SWA — окно).
struct KvLayout {
    /// Байт на токен для dense (non-SWA) слоёв.
    global_bytes_per_token: f64,
    /// Байт на ЯЧЕЙКУ для SWA-слоёв.
    swa_bytes_per_cell: f64,
    /// Число attention-слоёв.
    attn_layers: u32,
    /// sliding_window (n_swa) — если есть SWA-слои.
    swa_window: Option<u32>,
}

/// PAD до ближайшего кратного 256 (llama-context.cpp: n_ctx = GGML_PAD(n_ctx, 256)).
fn pad256(v: u32) -> u32 {
    v.saturating_add(255) / 256 * 256
}

/// Число ячеек SWA-кэша (llama-kv-cache-iswa.cpp): SWA капится окном, а не полным
/// контекстом. Приложение запускает llama-server с авто-параметрами
/// (n_parallel=4, kv_unified=true), значит n_seq_max=4, n_ubatch=512.
fn swa_cells_count(n_ctx_seq: u32, swa_window: u32) -> u32 {
    const SERVER_N_SEQ: u32 = 4; // llama-server авто n_parallel
    const N_UBATCH: u32 = 512;   // дефолт llama.cpp ubatch
    let size = (swa_window * SERVER_N_SEQ + N_UBATCH).min(n_ctx_seq);
    pad256(size)
}

/// KV-раскладка (Байт/токен для dense + Байт/ячейку для SWA) по данным GGUF.
///
/// Порядок приоритета:
/// 1. per-layer массив `head_count_kv` — признак SWA берётся из массива
///    `attention.sliding_window_pattern` (истинный источник у gemma4: SWA-слои
///    могут иметь БОЛЬШЕ голов, чем dense), при отсутствии паттерна — fallback
///    «local-слой имеет меньше KV-голов»;
/// 2. `full_attention_interval` (гибрид) — attention только в каждом iv-слое;
/// 3. классика — все слои, скалярный heads_kv.
fn kv_layout_gguf(
    model_path: &str,
    arch: Option<&str>,
    num_layers: u32,
    heads_kv_scalar: u32,
    heads_scalar: u32,
    key_len_global: u32,
    value_len_global: u32,
    key_len_swa: Option<u32>,
    value_len_swa: Option<u32>,
    interval: Option<u32>,
    swa_window: Option<u32>,
    swa_pattern: Option<Vec<i64>>,
    b_k: f64,
    b_v: f64,
) -> KvLayout {
    let _ = heads_scalar; // legacy-параметр, значение не влияет на расчёт (heads_kv_scalar).
    let per = |heads: u32, klen: u32, vlen: u32| -> f64 {
        heads as f64 * (klen as f64 * b_k + vlen as f64 * b_v)
    };

    if let Some(arr) = extract_i64_array_with_arch(model_path, arch, "attention.head_count_kv") {
        // Per-layer (gemma4-style). «Глобальная» норма KV-голов — максимальное
        // значение массива; но SWA vs global определяется sliding_window_pattern,
        // а НЕ числом голов.
        let global_kv = arr.iter().copied().max().unwrap_or(heads_kv_scalar as i64).max(1) as u32;
        let has_swa_dims = key_len_swa.is_some() && value_len_swa.is_some();
        let mut global_bytes: f64 = 0.0;
        let mut swa_bytes: f64 = 0.0;
        let mut attn = 0u32;
        for l in 0..num_layers {
            let hkv = arr
                .get(l as usize)
                .copied()
                .unwrap_or(global_kv as i64)
                .clamp(1, 1_000_000) as u32;
            let is_swa = match swa_pattern.as_deref() {
                // Истинный признак SWA: per-layer bool-массив из GGUF.
                Some(p) => p.get(l as usize).copied().unwrap_or(0) != 0,
                // Fallback для гибридов без паттерна: local-слой = с меньшим числом голов.
                None => has_swa_dims && hkv < global_kv,
            };
            let (klen, vlen) = if is_swa {
                (key_len_swa.unwrap_or(key_len_global), value_len_swa.unwrap_or(value_len_global))
            } else {
                (key_len_global, value_len_global)
            };
            let bytes = per(hkv, klen, vlen);
            if is_swa { swa_bytes += bytes; } else { global_bytes += bytes; }
            attn += 1;
        }
        return KvLayout {
            global_bytes_per_token: global_bytes,
            swa_bytes_per_cell: swa_bytes,
            attn_layers: attn,
            swa_window: if swa_bytes > 0.0 { swa_window } else { None },
        };
    }

    if let Some(iv) = interval.filter(|iv| *iv >= 1 && num_layers % iv == 0) {
        let attn_layers = num_layers / iv;
        return KvLayout {
            global_bytes_per_token: per(heads_kv_scalar, key_len_global, value_len_global) * attn_layers as f64,
            swa_bytes_per_cell: 0.0,
            attn_layers,
            swa_window: None,
        };
    }

    KvLayout {
        global_bytes_per_token: per(heads_kv_scalar, key_len_global, value_len_global) * num_layers as f64,
        swa_bytes_per_cell: 0.0,
        attn_layers: num_layers,
        swa_window: None,
    }
}

/// Оценка VRAM для заданного (эффективного) контекста.
///
/// Legacy-API: `kv_quant_*` = false → F16, true → q8_0, tail = 0.
/// Для BeeLlama (KVarN + precision tail) — `estimate_vram_with_spec`.
pub fn estimate_vram(
    model_path: &str,
    ctx_size: u32,
    kv_quant_keys: bool,
    kv_quant_values: bool,
) -> VramEstimate {
    estimate_vram_with_spec(
        model_path,
        ctx_size,
        &KvQuantSpec::from_legacy_flags(kv_quant_keys, kv_quant_values),
    )
}

/// Оценка VRAM с явной спецификацией KV-кванта (KVarN, q8_0, F16 + tail).
///
/// Precision tail (`--kv-tail-tokens`): последние N токенов хранятся в F16,
/// остальное тело — в `k_bytes`/`v_bytes`. Для tail считаем второй layout
/// с F16-байтами; SWA-ячейки режутся так же.
pub fn estimate_vram_with_spec(
    model_path: &str,
    ctx_size: u32,
    spec: &KvQuantSpec,
) -> VramEstimate {
    let model_mb = std::fs::metadata(model_path)
        .map(|m| m.len() as f64 / MIB)
        .unwrap_or(0.0);

    let arch = extract_gguf_arch(model_path);
    let num_layers = extract_u32_with_arch(model_path, arch.as_deref(), "block_count").unwrap_or(32);
    let embd = extract_u32_with_arch(model_path, arch.as_deref(), "embedding_length").unwrap_or(4096);
    let heads = extract_u32_with_arch(model_path, arch.as_deref(), "attention.head_count").unwrap_or(32);
    let head_dim = embd / heads.max(1);
    let heads_kv_scalar = extract_u32_with_arch(model_path, arch.as_deref(), "attention.head_count_kv").unwrap_or(heads);
    let key_len = extract_u32_with_arch(model_path, arch.as_deref(), "attention.key_length").unwrap_or(head_dim);
    let value_len = extract_u32_with_arch(model_path, arch.as_deref(), "attention.value_length").unwrap_or(head_dim);
    let key_len_swa = extract_u32_with_arch(model_path, arch.as_deref(), "attention.key_length_swa");
    let value_len_swa = extract_u32_with_arch(model_path, arch.as_deref(), "attention.value_length_swa");
    let interval = extract_u32_with_arch(model_path, arch.as_deref(), "full_attention_interval");
    let swa_window = extract_u32_with_arch(model_path, arch.as_deref(), "attention.sliding_window");
    let swa_pattern = extract_i64_array_with_arch(model_path, arch.as_deref(), "attention.sliding_window_pattern");

    let layout = kv_layout_gguf(
        model_path,
        arch.as_deref(),
        num_layers,
        heads_kv_scalar,
        heads,
        key_len,
        value_len,
        key_len_swa,
        value_len_swa,
        interval,
        swa_window,
        swa_pattern.clone(),
        spec.k_bytes,
        spec.v_bytes,
    );

    // Precision tail: те же слои/размерности, но F16-байта (k_bytes=v_bytes=2.0).
    // Если хвоста нет или тело уже F16 — второй layout не нужен.
    let tail_active = spec.tail_tokens > 0
        && (spec.k_bytes != 2.0 || spec.v_bytes != 2.0);
    let layout_f16 = if tail_active {
        Some(kv_layout_gguf(
            model_path,
            arch.as_deref(),
            num_layers,
            heads_kv_scalar,
            heads,
            key_len,
            value_len,
            key_len_swa,
            value_len_swa,
            interval,
            swa_window,
            swa_pattern,
            2.0,
            2.0,
        ))
    } else {
        None
    };

    let n_ctx_seq = pad256(ctx_size);
    let tail_cells_total = if tail_active {
        (spec.tail_tokens as u64).min(n_ctx_seq as u64) as u32
    } else {
        0
    };
    let body_cells = n_ctx_seq - tail_cells_total;

    let swa_cells = layout
        .swa_window
        .map(|w| swa_cells_count(n_ctx_seq, w))
        .unwrap_or(n_ctx_seq);
    let swa_tail_cells = if layout_f16.is_some() {
        (spec.tail_tokens as u64).min(swa_cells as u64) as u32
    } else {
        0
    };
    let swa_body_cells = swa_cells - swa_tail_cells;

    // Dense (non-SWA): тело квантованное + хвост F16.
    let mut kv_dense_bytes = layout.global_bytes_per_token * body_cells as f64;
    if let Some(f16) = &layout_f16 {
        kv_dense_bytes += f16.global_bytes_per_token * tail_cells_total as f64;
    }
    // SWA: ячейки окна, тело/хвост аналогично.
    let mut kv_swa_bytes = layout.swa_bytes_per_cell * swa_body_cells as f64;
    if let Some(f16) = &layout_f16 {
        kv_swa_bytes += f16.swa_bytes_per_cell * swa_tail_cells as f64;
    }

    let kv_per_token_mb = layout.global_bytes_per_token / MIB;
    let kv_swa_mb = kv_swa_bytes / MIB;
    let kv_mb = kv_dense_bytes / MIB + kv_swa_mb;
    // Workspace CUDA-буферов (промежуточные тензоры) ~10%, не более 256 МБ.
    let buffers_mb = ((model_mb + kv_mb) * 0.10).min(256.0);
    VramEstimate {
        model_mb,
        kv_per_token_mb,
        kv_mb,
        kv_swa_mb,
        buffers_mb,
        total_mb: model_mb + kv_mb + buffers_mb,
        num_layers,
        num_attn_layers: layout.attn_layers,
    }
}

/// VRAM при оффлоаде ровно `ngl` слоёв на GPU (остальные — в ОЗУ).
/// Линейная аппроксимация: и веса, и KV-слоты offloaded-слоёв лежат в VRAM,
/// CPU-слои (и их KV) — в ОЗУ.
pub fn vram_for_ngl(
    model_path: &str,
    ngl: u32,
    ctx_size: u32,
    kv_quant_keys: bool,
    kv_quant_values: bool,
) -> f64 {
    let est = estimate_vram(model_path, ctx_size, kv_quant_keys, kv_quant_values);
    vram_for_ngl_estimate(&est, ngl)
}

/// `vram_for_ngl` с явным KV-spec (BeeLlama KVarN и др.).
pub fn vram_for_ngl_with_spec(model_path: &str, ngl: u32, ctx_size: u32, spec: &KvQuantSpec) -> f64 {
    let est = estimate_vram_with_spec(model_path, ctx_size, spec);
    vram_for_ngl_estimate(&est, ngl)
}

/// Численное ядро `vram_for_ngl` (отдельно — для тестов).
pub fn vram_for_ngl_estimate(est: &VramEstimate, ngl: u32) -> f64 {
    let layers = est.num_layers.max(1) as f64;
    let frac = (ngl.min(est.num_layers) as f64) / layers;
    est.model_mb * frac + est.kv_mb * frac + est.buffers_mb
}

/// Максимальное число слоёв, помещающихся в бюджет VRAM.
///
/// `budget_mb` — свободная VRAM (или лимит), `reserve_mb` — запас на буферы/
/// фрагментацию/базу. Возвращает 0, если даже минимальный оффлоад не влезает
/// (тогда остаётся ngl=0). Формула: `floor((бюджет - резерв) / (МБ на слой))`,
/// где МБ на слой = (веса + KV) / block_count.
pub fn max_fitting_ngl(
    model_path: &str,
    ctx_size: u32,
    kv_quant_keys: bool,
    kv_quant_values: bool,
    budget_mb: f64,
    reserve_mb: f64,
) -> u32 {
    let est = estimate_vram(model_path, ctx_size, kv_quant_keys, kv_quant_values);
    max_fitting_ngl_estimate(&est, budget_mb, reserve_mb)
}

/// Численное ядро `max_fitting_ngl` (отдельно — для тестов без файла).
pub fn max_fitting_ngl_estimate(est: &VramEstimate, budget_mb: f64, reserve_mb: f64) -> u32 {
    max_fitting_ngl_safe_estimate(est, budget_mb, reserve_mb, 1.0)
}

/// Максимальное число слоёв с запасом безопасности `safety_factor` (>1): бюджет
/// делится на (МБ на слой × фактор). Фактор покрывает пиковые compute-буферы и
/// фрагментацию СВЕРХ `buffers_mb` — на реальной карте пик может превысить
/// линейную оценку «веса + KV» (буферы промпт-процессинга под ubatch 512 на
/// 12B-моделях далеко за 256 МБ). Используется в pre-flight запуска и авто-повторе.
pub fn max_fitting_ngl_safe_estimate(est: &VramEstimate, budget_mb: f64, reserve_mb: f64, safety_factor: f64) -> u32 {
    if est.num_layers == 0 {
        return 0;
    }
    let usable = (budget_mb - reserve_mb).max(0.0);
    let per_layer = ((est.model_mb + est.kv_mb) * safety_factor.max(1.0)) / est.num_layers as f64;
    if per_layer <= 0.0 {
        return est.num_layers;
    }
    (usable / per_layer).floor().min(est.num_layers as f64) as u32
}

/// Максимальное число слоёв с запасом безопасности (обёртка с чтением GGUF).
pub fn max_fitting_ngl_safe(
    model_path: &str,
    ctx_size: u32,
    kv_quant_keys: bool,
    kv_quant_values: bool,
    budget_mb: f64,
    reserve_mb: f64,
    safety_factor: f64,
) -> u32 {
    let est = estimate_vram(model_path, ctx_size, kv_quant_keys, kv_quant_values);
    max_fitting_ngl_safe_estimate(&est, budget_mb, reserve_mb, safety_factor)
}

/// `max_fitting_ngl_safe` с явным KV-spec — pre-flight/OOM-ретрай BeeLlama.
pub fn max_fitting_ngl_safe_with_spec(
    model_path: &str,
    ctx_size: u32,
    spec: &KvQuantSpec,
    budget_mb: f64,
    reserve_mb: f64,
    safety_factor: f64,
) -> u32 {
    let est = estimate_vram_with_spec(model_path, ctx_size, spec);
    max_fitting_ngl_safe_estimate(&est, budget_mb, reserve_mb, safety_factor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn est(model_mb: f64, kv_mb: f64, layers: u32) -> VramEstimate {
        VramEstimate {
            model_mb,
            kv_per_token_mb: kv_mb / 2048.0,
            kv_mb,
            kv_swa_mb: 0.0,
            buffers_mb: ((model_mb + kv_mb) * 0.10).min(256.0),
            total_mb: model_mb + kv_mb,
            num_layers: layers,
            num_attn_layers: layers,
        }
    }

    #[test]
    fn vram_for_ngl_scales_linearly_by_layers() {
        // 20 слоёв, веса 10000 МБ, KV 2000 МБ, буферы = min(0.1*12000, 256)=256.
        let e = est(10_000.0, 2_000.0, 20);
        // Полный оффлоад: веса+KV+буферы.
        let full = vram_for_ngl_estimate(&e, 20);
        assert!((full - 12_256.0).abs() < 0.01, "full: {}", full);
        // Половина слоёв: (веса+KV)/2 + буферы.
        let half = vram_for_ngl_estimate(&e, 10);
        assert!((half - (6000.0 + 256.0)).abs() < 0.01, "half: {}", half);
        // Больше слоёв, чем есть — кап на num_layers (полный оффлоад).
        assert_eq!(vram_for_ngl_estimate(&e, 999), full);
        // Ноль слоёв — только буферы.
        assert!((vram_for_ngl_estimate(&e, 0) - 256.0).abs() < 0.01, "zero: {}", vram_for_ngl_estimate(&e, 0));
    }

    #[test]
    fn max_fitting_ngl_computes_floor_of_budget() {
        // МБ на слой = (10000+2000)/20 = 600. Бюджет 10 500 − резерв 500 = 10 000 → 16 слоёв.
        let e = est(10_000.0, 2_000.0, 20);
        assert_eq!(max_fitting_ngl_estimate(&e, 10_500.0, 500.0), 16);
        // Safety-фактор 1.25 урезает число слоёв (пиковые буферы сверх оценки).
        // 600 МБ/слой × 1.25 = 750 → 10 000 / 750 = 13 слоёв.
        assert_eq!(max_fitting_ngl_safe_estimate(&e, 10_500.0, 500.0, 1.25), 13);
        // Фактор никогда не расширяет бюджет.
        assert!(max_fitting_ngl_safe_estimate(&e, 10_500.0, 500.0, 1.25) <= max_fitting_ngl_estimate(&e, 10_500.0, 500.0));
        // Резерв больше бюджета → 0 (не влезает даже минимальный оффлоад).
        assert_eq!(max_fitting_ngl_estimate(&e, 400.0, 500.0), 0);
        // Бюджет больше полного оффлоада → кап на число слоёв.
        assert_eq!(max_fitting_ngl_estimate(&e, 99_000.0, 0.0), 20);
        // Нулевое число слоёв → 0.
        assert_eq!(max_fitting_ngl_estimate(&est(100.0, 100.0, 0), 1000.0, 0.0), 0);
    }

    #[test]
    fn kv_bytes_quant_ratio() {
        // q8_0 (34/32) против FP16 (2.0) — соотношение ровно (34/32)/2.
        let mk = |qk: bool, qv: bool| {
            let b_k = kv_bytes_per_elem(qk);
            let b_v = kv_bytes_per_elem(qv);
            16.0 * (128.0 * b_k + 128.0 * b_v)
        };
        assert!((mk(true, true) / mk(false, false) - (34.0 / 32.0) / 2.0).abs() < 1e-9);
    }

    #[test]
    fn cache_type_bytes_parses_kvarn_and_standards() {
        // bpv/8: kvarn5 = 5.375/8, kvarn4 = 4.375/8 (README Anbeeld).
        assert!((cache_type_bytes("kvarn5").unwrap() - 5.375 / 8.0).abs() < 1e-12);
        assert!((cache_type_bytes("kvarn4").unwrap() - 4.375 / 8.0).abs() < 1e-12);
        assert!((cache_type_bytes("q8_0").unwrap() - 8.5 / 8.0).abs() < 1e-12);
        assert_eq!(cache_type_bytes("f16").unwrap(), 2.0);
        assert_eq!(cache_type_bytes("bf16").unwrap(), 2.0);
        assert!(cache_type_bytes("unknown_type").is_none());
    }

    #[test]
    fn kv_spec_from_source_prefers_beellama_kvarn_over_flags() {
        use crate::engine::sources::source_spec;
        let bee = source_spec("beellama").expect("beellama in registry");
        // Даже legacy-флаги true → KVarN из runtime перекрывает (приоритет как в build_cmd).
        let spec = kv_spec_from_source(Some(&bee), true, true);
        assert!((spec.k_bytes - 5.375 / 8.0).abs() < 1e-12, "k: {}", spec.k_bytes);
        assert!((spec.v_bytes - 4.375 / 8.0).abs() < 1e-12, "v: {}", spec.v_bytes);
        assert_eq!(spec.tail_tokens, 1024);

        let gg = source_spec("ggml-org").expect("ggml-org");
        // Официальный llama.cpp: runtime без cache-type → legacy flags.
        let spec_gg = kv_spec_from_source(Some(&gg), false, false);
        assert_eq!(spec_gg, KvQuantSpec::fp16());
        let spec_gg_q = kv_spec_from_source(Some(&gg), true, true);
        assert_eq!(spec_gg_q.k_bytes, 34.0 / 32.0);

        // None → legacy.
        assert_eq!(kv_spec_from_source(None, false, false), KvQuantSpec::fp16());
    }

    #[test]
    fn estimate_kvarn_kv_smaller_than_fp16() {
        // KV-часть kvarn5/kvarn4 (без tail) ≈ (5.375+4.375)/(16+16) ≈ 30.5% от F16.
        // С tail 1024 при большом контексте — чуть больше (хвост F16), но ≪ F16.
        let path = "nonexistent.gguf"; // metadata → 0, GGUF-ключи → дефолты (32 слоя и т.д.)
        let fp16 = estimate_vram_with_spec(path, 8192, &KvQuantSpec::fp16());
        let kvarn = estimate_vram_with_spec(
            path,
            8192,
            &KvQuantSpec { k_bytes: 5.375 / 8.0, v_bytes: 4.375 / 8.0, tail_tokens: 0 },
        );
        assert!(kvarn.kv_mb > 0.0 && fp16.kv_mb > 0.0);
        let ratio = kvarn.kv_mb / fp16.kv_mb;
        let expected = (5.375 + 4.375) / (16.0 + 16.0);
        assert!(
            (ratio - expected).abs() < 0.02,
            "kvarn/fp16 kv ratio {} vs expected {}",
            ratio,
            expected
        );

        // С precision tail 1024: тело kvarn, хвост F16 → между чистым kvarn и F16.
        let with_tail = estimate_vram_with_spec(
            path,
            8192,
            &KvQuantSpec { k_bytes: 5.375 / 8.0, v_bytes: 4.375 / 8.0, tail_tokens: 1024 },
        );
        assert!(
            with_tail.kv_mb > kvarn.kv_mb && with_tail.kv_mb < fp16.kv_mb,
            "tail kv {} not in (kvarn {}, fp16 {})",
            with_tail.kv_mb, kvarn.kv_mb, fp16.kv_mb
        );
    }

    #[test]
    fn max_fitting_ngl_kvarn_allows_more_layers_than_fp16() {
        // Один budget: меньший KV (kvarn) → больше слоёв на GPU. Это и есть цель BeeLlama.
        let path = "nonexistent.gguf";
        let budget = 6_000.0;
        let reserve = 1_024.0;
        let safety = 1.25;
        let ngl_fp16 = max_fitting_ngl_safe_with_spec(path, 8192, &KvQuantSpec::fp16(), budget, reserve, safety);
        let ngl_kvarn = max_fitting_ngl_safe_with_spec(
            path,
            8192,
            &KvQuantSpec { k_bytes: 5.375 / 8.0, v_bytes: 4.375 / 8.0, tail_tokens: 1024 },
            budget,
            reserve,
            safety,
        );
        // F16-KV на дефолтном layout (32 слоя, 8K ctx) съедает бюджет почти весь
        // (см. ratio-тест) — kvarn обязан дать строго больше слоёв либо оба капа
        // на num_layers (тогда равны — допускаем только строгий ≥).
        assert!(
            ngl_kvarn >= ngl_fp16,
            "kvarn ngl {} < fp16 ngl {}",
            ngl_kvarn,
            ngl_fp16
        );
        if ngl_fp16 < 32 {
            assert!(
                ngl_kvarn > ngl_fp16,
                "kvarn must fit strictly more layers when fp16 is truncated: {} vs {}",
                ngl_kvarn,
                ngl_fp16
            );
        }
    }

    #[test]
    fn estimate_vram_bool_wrapper_matches_spec_api() {
        let path = "nonexistent.gguf";
        let a = estimate_vram(path, 4096, false, false);
        let b = estimate_vram_with_spec(path, 4096, &KvQuantSpec::fp16());
        assert_eq!(a.kv_mb.to_bits(), b.kv_mb.to_bits());
        assert_eq!(a.total_mb.to_bits(), b.total_mb.to_bits());

        let c = estimate_vram(path, 4096, true, true);
        let d = estimate_vram_with_spec(path, 4096, &KvQuantSpec::from_legacy_flags(true, true));
        assert_eq!(c.kv_mb.to_bits(), d.kv_mb.to_bits());
    }

    #[test]
    fn swa_cells_uses_windows_not_full_ctx() {
        // llama-server: n_parallel=4, kv_unified=true → n_seq_max=4, ubatch=512.
        // window 1024: cells = min(ctx, 1024·4+512=4608), PAD 256.
        assert_eq!(swa_cells_count(pad256(18_432), 1024), 4_608);
        // Меньше окна — полный ctx (PAD 256).
        assert_eq!(swa_cells_count(pad256(1024), 1024), 1024);
        // Промежуточный ctx тоже паддится до 256 (1800 → 2048).
        assert_eq!(swa_cells_count(pad256(1_800), 1024), 2_048);
    }
}