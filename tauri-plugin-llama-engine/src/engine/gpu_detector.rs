//! Детекция NVIDIA GPU, версии CUDA драйвера и вычислительной способности (compute capability).
//! Порядок: NVML (уже используется проектом) → nvidia-smi → нет GPU.

use std::process::Command;

/// Минимальная мажорная версия CUDA драйвера для работы нашего движка
/// (exe слинкован с cublas64_12.dll — нужен драйвер с поддержкой CUDA 12).
pub const MIN_CUDA_MAJOR: u32 = 12;

/// Минимальная архитектура (sm_XX), для которой в сборке `cuda-13.x`
/// вообще есть вычислительные ядра.
///
/// Обоснование (ggml/src/ggml-cuda/CMakeLists.txt в llama.cpp):
/// ```cmake
/// if (CUDAToolkit_VERSION VERSION_LESS "13")
///     list(APPEND CMAKE_CUDA_ARCHITECTURES 50-virtual 61-virtual 70-virtual)
/// endif ()
/// list(APPEND CMAKE_CUDA_ARCHITECTURES 75-virtual 80-virtual 86-real 89-real 90-virtual 120a-real)
/// ```
/// Тулкит CUDA 13 выпилил Maxwell (sm_5x), Pascal (sm_6x) и Volta (sm_70);
/// минимальный arch в его наборе — `75-virtual` (Turing). Релизные бинари
/// `llama-*-bin-win-cuda-13.*` собираются именно с этим дефолтным набором
/// (см. `.github/workflows/release.yml`: "no CMAKE_CUDA_ARCHITECTURES: use
/// the broad default arch set").
///
/// Следствие: на карте младше Turing движок `cuda-13.x` падает с
/// `CUDA error: no kernel image is available for execution on the device`,
/// а сборка `cuda-12.4` там работает (в ней есть `61-virtual` PTX).
pub const MIN_COMPUTE_MAJOR_CUDA13: u32 = 7;

/// Минимальная минорная версия compute capability, поддерживаемая
/// сборкой `cuda-13.x` при `compute_major == 7`.
///
/// sm_70 — это Volta, и её CUDA 13 тоже удалил, поэтому для major 7 нужна
/// минорная часть ≥ 5 (то есть sm_75 = Turing). Проверять только major
/// нельзя: карта major 7 / minor 0 получила бы cuda-13.x и упала бы.
pub const MIN_COMPUTE_MINOR_CUDA13: u32 = 5;

/// Поколение CUDA-сборки движка llama.cpp, требуемое для GPU.
/// Сборка `cuda-12.4` НЕ содержит ядер Blackwell (sm_120, RTX 50xx) —
/// они появились только в сборках CUDA >= 12.8 (см. ggml/src/ggml-cuda/CMakeLists.txt:
/// "# 120 == Blackwell, needs CUDA v12.8"). Для Blackwell нужен вариант `cuda-13.x`.
/// Сборка `cuda-13.x` (в отличие от мифа «только для 50xx») содержит ядра
/// sm_75..sm_120 — включая RTX 40xx (sm_89). Выбор между cuda-12/13 на не-Blackwell
/// определяется свежестью драйвера, как в Jan (см. janhq/jan backend.rs,
/// `get_supported_features`: на Windows cuda-13 доступен при драйвере >= 580).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CudaGen {
    /// Сборка cuda-12.4 — для драйверов CUDA 12.x (sm_5x..sm_11x, все RTX до 40xx)
    Cuda12,
    /// Сборка cuda-13.x — для Blackwell (sm_120) и свежих драйверов CUDA 13+ (R580+)
    Cuda13,
}

impl CudaGen {
    pub fn label(self) -> &'static str {
        match self {
            CudaGen::Cuda12 => "cuda-12.x",
            CudaGen::Cuda13 => "cuda-13.x",
        }
    }
}

#[derive(Debug, Clone)]
pub struct GpuInfo {
    pub has_nvidia: bool,
    pub gpu_name: String,
    /// Мажорная версия CUDA драйвера (0 = не определена)
    pub cuda_major: u32,
    /// Минорная версия CUDA драйвера
    pub cuda_minor: u32,
    pub driver_version: String,
    /// Мажорный номер compute capability (sm_XX), 0 = не определён.
    /// Blackwell (RTX 50xx) = 12, RTX 30/40xx = 8/9, GTX 10xx = 6.
    pub compute_major: u32,
    /// Минорный номер compute capability (sm_XX.Y)
    pub compute_minor: u32,
}

impl Default for GpuInfo {
    fn default() -> Self {
        Self {
            has_nvidia: false,
            gpu_name: String::new(),
            cuda_major: 0,
            cuda_minor: 0,
            driver_version: String::new(),
            compute_major: 0,
            compute_minor: 0,
        }
    }
}

/// Определяет видеокарту: NVML → nvidia-smi.
pub fn detect_gpu() -> GpuInfo {
    if let Ok(info) = detect_via_nvml() {
        return info;
    }
    if let Ok(info) = detect_via_nvidia_smi() {
        return info;
    }
    GpuInfo::default()
}

/// Драйвер поддерживает CUDA 12+ (т.е. наш движок сможет загрузиться)
pub fn supports_cuda12(info: &GpuInfo) -> bool {
    info.has_nvidia && info.cuda_major >= MIN_CUDA_MAJOR
}

/// Какую сборку движка (cuda-12.4 / cuda-13.x) требует установленная GPU.
/// None = CUDA не используется (нет NVIDIA / старый драйвер / нет данных).
///
/// Правила:
/// - Blackwell (sm_120): только cuda-13.x — сборка cuda-12.4 не имеет ядер;
/// - **архитектура младше Turing (sm_50/61/70): только cuda-12.x** —
///   в сборке cuda-13.x ядер для них нет вообще (`MIN_COMPUTE_MAJOR_CUDA13`).
///   Это правило приоритетнее свежести драйвера: драйвер R580+ одинаково
///   хорошо работает и с Maxwell, и с Blackwell, но бинарник cuda-13.x без
///   ядер sm_61 на GTX 10xx падает с `CUDA_ERROR_NO_KERNEL_IMAGE`;
/// - свежий драйвер CUDA 13+ (R580+): cuda-13.x — она содержит ядра
///   sm_75..sm_120, включая RTX 40xx (sm_89), и является приоритетным
///   выбором (как в Jan);
/// - архитектура не определилась: cuda-12.4 — он совместим с заметно более
///   широким набором карт, поэтому при неопределённости берём его, а не гадаем;
/// - остальные NVIDIA с драйвером CUDA 12: cuda-12.4.
pub fn required_cuda_gen(info: &GpuInfo) -> Option<CudaGen> {
    if !supports_cuda12(info) {
        return None;
    }
    // Blackwell (sm_120): сборка cuda-12.4 не имеет ядер для этих GPU.
    if info.compute_major >= 12 {
        return Some(CudaGen::Cuda13);
    }
    // Драйвер новый, а карта — нет: ядер cuda-13.x для Maxwell/Pascal/Volta
    // не существует. Отдаём cuda-12.x независимо от версии драйвера.
    if !cuda13_arch_supported(info) {
        return Some(CudaGen::Cuda12);
    }
    if info.compute_major == 0 {
        // Архитектура неизвестна (NVML её не отдала) — берём самый
        // совместимый вариант, чтобы не уводить юзера в гарантированный сбой.
        return Some(CudaGen::Cuda12);
    }
    // Драйвер CUDA 13+ → cuda-13.x (предпочтителен, как в Jan).
    if info.cuda_major >= 13 {
        return Some(CudaGen::Cuda13);
    }
    // Драйвер CUDA 12.x: cuda-12.4 — самый совместимый вариант.
    Some(CudaGen::Cuda12)
}

/// Есть ли в сборке `cuda-13.x` ядра для этой видеокарты.
///
/// `compute_major == 0` → считаем, что есть: архитектура неизвестна, и
/// блокировать юзера из-за отсутствия данных нельзя (сам авто-подбор в этом
/// случае тоже ведёт в cuda-12.x — см. `required_cuda_gen`).
pub fn cuda13_arch_supported(info: &GpuInfo) -> bool {
    if info.compute_major == 0 {
        return true;
    }
    if info.compute_major != MIN_COMPUTE_MAJOR_CUDA13 {
        return info.compute_major > MIN_COMPUTE_MAJOR_CUDA13;
    }
    // major 7: sm_70 (Volta) удалён из CUDA 13, sm_75 (Turing) — нет.
    info.compute_minor >= MIN_COMPUTE_MINOR_CUDA13
}

/// Короткое имя архитектуры GPU для сообщений: `sm_61`, `sm_86`,
/// `не определена`. Строка «cuda-13.x на sm_61» сразу показывает юзеру,
/// что не так с его видеокартой.
pub fn compute_label(info: &GpuInfo) -> String {
    if info.compute_major == 0 {
        "не определена".to_string()
    } else {
        format!("sm_{}", info.compute_major)
    }
}

/// Драйвер есть, но слишком старый (CUDA 11.x) — нужен апгрейд драйвера
pub fn requires_driver_update(info: &GpuInfo) -> bool {
    info.has_nvidia && info.cuda_major > 0 && info.cuda_major < MIN_CUDA_MAJOR
}

fn detect_via_nvml() -> Result<GpuInfo, String> {
    let nvml = nvml_wrapper::Nvml::init().map_err(|e| format!("NVML init: {}", e))?;
    let device = nvml.device_by_index(0).map_err(|e| format!("NVML device: {}", e))?;
    let name = device.name().unwrap_or_else(|_| "NVIDIA GPU".to_string());
    let driver = nvml
        .sys_driver_version()
        .map_err(|e| format!("NVML driver: {}", e))?;
    let cuda_ver = nvml
        .sys_cuda_driver_version()
        .map_err(|e| format!("NVML cuda: {}", e))?;
    let major = (cuda_ver / 1000) as u32;
    let minor = ((cuda_ver % 1000) / 10) as u32;
    // compute capability: главный признак — есть ли в сборке движка ядра для GPU.
    let (compute_major, compute_minor) = match device.cuda_compute_capability() {
        Ok(cap) => (cap.major.max(0) as u32, cap.minor.max(0) as u32),
        Err(_) => (0, 0),
    };
    Ok(GpuInfo {
        has_nvidia: true,
        gpu_name: name,
        cuda_major: major,
        cuda_minor: minor,
        driver_version: driver,
        compute_major,
        compute_minor,
    })
}

fn detect_via_nvidia_smi() -> Result<GpuInfo, String> {
    let mut cmd = Command::new("nvidia-smi");
    #[cfg(target_os = "windows")]
    { use std::os::windows::process::CommandExt; cmd.creation_flags(0x08000000); }
    let output = cmd
        .output()
        .map_err(|e| format!("nvidia-smi not found: {}", e))?;
    if !output.status.success() {
        return Err(format!("nvidia-smi exit: {}", output.status));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut cuda_major = 0u32;
    let mut cuda_minor = 0u32;
    for line in stdout.lines() {
        if let Some(idx) = line.find("CUDA Version:") {
            let ver_str = line[idx + "CUDA Version:".len()..].trim();
            let parts: Vec<&str> = ver_str.split('.').collect();
            cuda_major = parts
                .first()
                .and_then(|s| s.trim().parse::<u32>().ok())
                .unwrap_or(0);
            cuda_minor = parts
                .get(1)
                .and_then(|s| s.trim().parse::<u32>().ok())
                .unwrap_or(0);
            break;
        }
    }
    if cuda_major == 0 {
        return Err("CUDA version not found in nvidia-smi output".to_string());
    }
    let (compute_major, compute_minor) = query_compute_capability();
    Ok(GpuInfo {
        has_nvidia: true,
        gpu_name: "NVIDIA GPU (nvidia-smi)".to_string(),
        cuda_major,
        cuda_minor,
        driver_version: String::new(),
        compute_major,
        compute_minor,
    })
}

/// Compute capability через nvidia-smi --query-gpu=compute_cap (формат "12.0")
fn query_compute_capability() -> (u32, u32) {
    let mut cmd = Command::new("nvidia-smi");
    cmd.args(["--query-gpu=compute_cap", "--format=csv,noheader"]);
    #[cfg(target_os = "windows")]
    { use std::os::windows::process::CommandExt; cmd.creation_flags(0x08000000); }
    let output = match cmd.output() {
        Ok(o) if o.status.success() => o,
        _ => return (0, 0),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let cap = line.trim();
        if cap.is_empty() {
            continue;
        }
        let parts: Vec<&str> = cap.split('.').collect();
        let major = parts
            .first()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(0);
        let minor = parts
            .get(1)
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(0);
        return (major, minor);
    }
    (0, 0)
}

/// Человекочитаемое описание статуса GPU для логов/UI
pub fn describe_gpu(info: &GpuInfo) -> String {
    if !info.has_nvidia {
        "NVIDIA GPU не обнаружен. Будет использован CPU-режим.".to_string()
    } else if requires_driver_update(info) {
        format!(
            "Обнаружен NVIDIA GPU ({}), но драйвер поддерживает только CUDA {}.{}.\n\
             Для GPU-ускорения обновите драйвер NVIDIA до версии >= 527.41 (CUDA 12+).\n\
             Пока работаем в CPU-режиме.",
            info.gpu_name, info.cuda_major, info.cuda_minor
        )
    } else {
        let cc = if info.compute_major > 0 {
            format!(", compute {}.{}", info.compute_major, info.compute_minor)
        } else {
            String::new()
        };
        let variant = required_cuda_gen(info)
            .map(|g| format!("нужен вариант {}", g.label()))
            .unwrap_or_else(|| "".to_string());
        // Свежий драйвер CUDA 13+ на карте младше Turing: подчёркиваем, что
        // cuda-13.x тут не подойдёт, иначе авто-подбор бьётся об отсутствие ядер.
        let arch_note = if info.cuda_major >= 13 && !cuda13_arch_supported(info) {
            format!(
                " Внимание: карта архитектуры {}, а в сборке cuda-13.x ядра только от sm_75 (Turing) — вариант cuda-13.x на ней не запустится.",
                compute_label(info)
            )
        } else {
            String::new()
        };
        format!(
            "NVIDIA GPU: {} (драйвер CUDA {}.{}{}) — GPU-ускорение доступно ({}).{}",
            info.gpu_name, info.cuda_major, info.cuda_minor, cc, variant, arch_note
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu(has_nvidia: bool, cuda_major: u32, compute_major: u32, compute_minor: u32) -> GpuInfo {
        GpuInfo {
            has_nvidia,
            gpu_name: "Test GPU".to_string(),
            cuda_major,
            cuda_minor: 0,
            driver_version: String::new(),
            compute_major,
            compute_minor,
        }
    }

    #[test]
    fn blackwell_requires_cuda13() {
        // RTX 5070 Ti: compute 12.0, драйвер CUDA 13.x
        let info = gpu(true, 13, 12, 0);
        assert_eq!(required_cuda_gen(&info), Some(CudaGen::Cuda13));
        assert!(supports_cuda12(&info));
    }

    #[test]
    fn rtx30_40_requires_cuda12() {
        // RTX 4060: compute 8.9, драйвер CUDA 12.x
        let info = gpu(true, 12, 8, 9);
        assert_eq!(required_cuda_gen(&info), Some(CudaGen::Cuda12));
        // RTX 3080: compute 8.6
        let info = gpu(true, 12, 8, 6);
        assert_eq!(required_cuda_gen(&info), Some(CudaGen::Cuda12));
        // GTX 1660: compute 7.5
        let info = gpu(true, 12, 7, 5);
        assert_eq!(required_cuda_gen(&info), Some(CudaGen::Cuda12));
    }

    #[test]
    fn rtx40_with_cuda13_driver_prefers_cuda13() {
        // RTX 4070 Ti Super: compute 8.9, но свежий драйвер CUDA 13 (R580+) —
        // сборка cuda-13.x содержит ядра sm_89, поэтому она предпочтительна (как в Jan).
        let info = gpu(true, 13, 8, 9);
        assert_eq!(required_cuda_gen(&info), Some(CudaGen::Cuda13));
        // RTX 3080 с драйвером CUDA 13 — тоже cuda-13.x
        let info = gpu(true, 13, 8, 6);
        assert_eq!(required_cuda_gen(&info), Some(CudaGen::Cuda13));
    }

    #[test]
    fn unknown_compute_cap_falls_back_to_cuda12() {
        let info = gpu(true, 12, 0, 0);
        assert_eq!(required_cuda_gen(&info), Some(CudaGen::Cuda12));
        // Архитектура не определилась — даже при свежем драйвере CUDA 13 берём
        // cuda-12.4: он совместим с заметно более широким набором карт, а
        // cuda-13.x на неизвестной архитектуре — лотерея с падением.
        let info = gpu(true, 13, 0, 0);
        assert_eq!(required_cuda_gen(&info), Some(CudaGen::Cuda12));
    }

    #[test]
    fn no_gpu_or_old_driver_means_cpu() {
        assert_eq!(required_cuda_gen(&gpu(false, 0, 0, 0)), None);
        // Драйвер CUDA 11.x — слишком старый для движка CUDA 12
        assert_eq!(required_cuda_gen(&gpu(true, 11, 12, 0)), None);
        assert!(requires_driver_update(&gpu(true, 11, 12, 0)));
    }

    // ─── Регрессия: инцидент у юзера с 4 ГБ GPU ───
    // Юзер поставил движок, авто-подбор выбрал `cuda-13.x` (драйвер R580+
    // сообщает CUDA 13), а карта — Pascal. Релизный бинарь cuda-13.x собран
    // только под sm_75+ (ggml-cuda/CMakeLists.txt добавляет 50/61/70 только
    // при CUDAToolkit_VERSION < 13), поэтому движок упал с
    // `CUDA error: no kernel image is available for execution on the device`.
    // Проверяем, что свежесть драйвера больше не перевешивает архитектуру.

    #[test]
    fn pascal_with_cuda13_driver_never_gets_cuda13() {
        // GTX 1050 Ti / 1060 / 1080: compute 6.1, драйвер R580+ (CUDA 13.x)
        for compute in [5u32, 6] {
            let info = gpu(true, 13, compute, 0);
            assert_eq!(
                required_cuda_gen(&info),
                Some(CudaGen::Cuda12),
                "compute sm_{} не должен получать cuda-13.x даже на драйвере CUDA 13",
                compute
            );
            assert!(
                !cuda13_arch_supported(&info),
                "sm_{} не поддержан сборкой cuda-13.x",
                compute
            );
        }
        // sm_61 с минорной частью — тот же вывод
        assert_eq!(required_cuda_gen(&gpu(true, 13, 6, 1)), Some(CudaGen::Cuda12));
    }

    #[test]
    fn volta_sm70_is_also_excluded_from_cuda13() {
        // Регрессия: проверка только по major пропускала Volta. major 7 / minor 0
        // — это sm_70 (Volta), которую CUDA 13 тоже удалила, тогда как major 7 /
        // minor 5 — sm_75 (Turing), и она поддержана.
        let volta = gpu(true, 13, 7, 0);
        assert!(
            !cuda13_arch_supported(&volta),
            "Volta sm_70 выпилена из CUDA 13 — major 7 сам по себе не подходит"
        );
        assert_eq!(required_cuda_gen(&volta), Some(CudaGen::Cuda12));
        assert_eq!(variant_for_gen(&volta), CudaGen::Cuda12);

        let turing = gpu(true, 13, 7, 5);
        assert!(
            cuda13_arch_supported(&turing),
            "sm_75 (Turing) — первый arch, поддержанный в cuda-13.x"
        );
        assert_eq!(required_cuda_gen(&turing), Some(CudaGen::Cuda13));
    }

    /// Маленький локальный хелпер: cuda_gen_variant живёт в llamacpp_installer,
    /// здесь проверяем только само решение `required_cuda_gen`.
    fn variant_for_gen(info: &GpuInfo) -> CudaGen {
        required_cuda_gen(info).expect("NVIDIA с драйвером CUDA 13 обязан получить вариант")
    }

    #[test]
    fn maxwell_with_cuda13_driver_never_gets_cuda13() {
        // GTX 960 / 970 / 980: compute 5.2, драйвер R580+ (CUDA 13.x)
        let info = gpu(true, 13, 5, 2);
        assert_eq!(required_cuda_gen(&info), Some(CudaGen::Cuda12));
        assert!(!cuda13_arch_supported(&info));
        // Старый драйвер CUDA 12 на той же карте — тоже cuda-12.4
        assert_eq!(required_cuda_gen(&gpu(true, 12, 5, 2)), Some(CudaGen::Cuda12));
    }

    #[test]
    fn turing_and_newer_still_get_cuda13_on_fresh_driver() {
        // GTX 1650 / RTX 2060: compute 7.5 — ядра в cuda-13.x есть
        let info = gpu(true, 13, 7, 5);
        assert_eq!(required_cuda_gen(&info), Some(CudaGen::Cuda13));
        assert!(cuda13_arch_supported(&info));
        // sm_86 / sm_89 / sm_90 — тоже
        for compute in [8u32, 9, 10] {
            assert!(cuda13_arch_supported(&gpu(true, 13, compute, 0)));
        }
    }

    #[test]
    fn compute_label_and_arch_note_expose_the_reason() {
        let info = gpu(true, 13, 6, 1);
        assert_eq!(compute_label(&info), "sm_6");
        let text = describe_gpu(&info);
        assert!(
            text.contains("sm_6") && text.contains("cuda-12.x"),
            "describe_gpu должен называть архитектуру и рекомендованный вариант: {}",
            text
        );
        // Неизвестная архитектура — не врём в тексте
        assert_eq!(compute_label(&gpu(true, 13, 0, 0)), "не определена");
        // Turing+ без предупреждения
        let text = describe_gpu(&gpu(true, 13, 8, 6));
        assert!(!text.contains("Внимание"), "для Turing лишнее предупреждение: {}", text);
    }
}
