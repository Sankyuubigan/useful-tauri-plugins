//! Рефайн видео H3 — путь B: spawn `sd-cli.exe` с флагами `--hires*`
//! (нативный «Latent hires second pass», PR #2249, движок ≥ master-1049).
//!
//! HTTP-путь закрыт проверяемо: sd-server отклоняет hires-объект в vid_gen
//! 400 «invalid generation parameters» (зонд R0.1, 05.11.26,
//! test/bench/video_hires/probe — изоляция переменной: тот же payload без
//! hires принят 202). Единственная дорога — CLI-флаги.
//!
//! Механика replay (основа «рефайн последнего видео»): движок не умеет
//! хранить видео-латенты, поэтому рефайн = ПОЛНАЯ перегенерация драфта с тем
//! же сидом и теми же параметрами драфта + латент-апскейл 1.4× + второй
//! проход. Реплей-фиделити доказана бенчем R0.4 (E vs G14 визуально
//! идентичны; MD5 различается — GPU-недетерминизм reductions).
//!
//! Конфиг-паритет с победителем бенча E (test/bench/video_hires/):
//! драфт-план каталога (turbo: 4 шага lcm/simple/shift12 + DMAD) →
//! `--hires --hires-width 1216 --hires-height 672 --hires-steps 4
//! --hires-denoising-strength 0.4` + learned-апскейлер (если файл есть в
//! bundle_dir; иначе встроенный — бенч C14: работает, чуть «рисованнее»).
//! 412 с на 5-сек клип, идентичность персонажа сохранена.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::engine::gen_guard::MediaGenGuard;
use crate::engine::models_catalog::{
    bundle_files_weights, resolve_diffusion_file, resolve_placement, VideoBundleEntry,
    VideoRefinePreset,
};
use crate::engine::preflight;
use crate::engine::process_util::{
    assign_child_to_kill_job, kill_process_tree, register_engine_pid,
};
use crate::engine::sdcpp_installer;
use crate::engine::video_engine::{
    free_ram_gb, pipe_log, pipe_log_with_tail, preflight_video, sampling_plan_for_quality,
    video_lora_spawn_dir, VideoGenResult, VIDEO_GEN_TIMEOUT,
};

/// Маркер Err-строки рефайна: после него идут последние строки stderr движка.
/// Хост (vram.rs::is_honest_tool_error) считает такие ошибки честными и не
/// маскирует их «внутренней ошибкой media-движка» (БАГ-3, 06.11.26).
pub const ENGINE_LOG_REASON: &str = "Причина по логу движка:";

/// Размер хвоста stderr, хранимого для Err-строки (строк движка).
const ENGINE_LOG_TAIL_LINES: usize = 8;

/// Ресолв sd-cli.exe: зеркало VideoEngine::new — вариант-каталог инсталлера
/// (backends/<source>/<variant>/sd-cli.exe), фоллбэк — легаси рядом с
/// engine_dir. sd-cli лежит рядом с sd-server.exe в том же каталоге.
fn sd_cli_path(engine_dir: &Path) -> Result<PathBuf, String> {
    let source = "leejet";
    let variant = sdcpp_installer::resolve_variant(None);
    let mut cli = sdcpp_installer::variant_dir(engine_dir, source, &variant).join("sd-cli.exe");
    if !cli.exists() {
        let legacy = engine_dir.join("sd-cli.exe");
        if legacy.exists() {
            cli = legacy;
        } else {
            return Err(
                "Движок видео не установлен (нет sd-cli.exe).\n\
                 Откройте Настройки → «Движок запуска ImageGEN моделей» и нажмите «Установить движок»."
                    .to_string(),
            );
        }
    }
    Ok(cli)
}

/// Структурный гейт движка (§00.2): фича `--hires*` детектируется по
/// `sd-cli --help` — старый билд даёт честный отказ с советом обновить,
/// а не runtime-падение посреди генерации. Возвращает путь к проверенному CLI.
pub fn refine_engine_supported(engine_dir: &Path) -> Result<PathBuf, String> {
    let cli = sd_cli_path(engine_dir)?;
    let mut probe = std::process::Command::new(&cli);
    probe.arg("--help");
    // Без CREATE_NO_WINDOW пробник мелькает пустым консольным окном при
    // каждом рефайне (баг-репорт 06.11.26); паритет боевому спавну ниже.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        probe.creation_flags(0x0800_0000);
    }
    let out = probe
        .output()
        .map_err(|e| format!("Не удалось опросить {} --help: {}", cli.display(), e))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !text.contains("--hires-width") {
        return Err(
            "Установленный движок не умеет рефайн (нет флагов --hires*): фича появилась \
             в сборках от 04.11.2026 (master-1049+). Обновите движок: Настройки → \
             «Движок запуска ImageGEN моделей» → «Проверить обновления» → «Обновить»."
                .to_string(),
        );
    }
    Ok(cli)
}

/// Имя learned-апскейлера (стем) → файл в bundle_dir. Стем может быть задан
/// без расширения (канон каталога) или с ним; файла нет → None (встроенный
/// апскейлер движка — гейт G3: не отказ).
fn learned_upscaler_file(bundle_dir: &Path, filename: Option<&String>) -> Option<PathBuf> {
    let name = filename?;
    let direct = bundle_dir.join(name);
    if direct.exists() {
        return Some(direct);
    }
    let with_ext = bundle_dir.join(format!("{}.safetensors", name));
    with_ext.exists().then_some(with_ext)
}

/// БАГ-3 (06.11.26, сессия 1793919569600): learned-апскейлер загружается
/// `set_loader`'ом посреди выполнения, а audio-VAE раннер после кодирования
/// голоса жив до конца генерации (апстрим minimax_h3_audio_vae.hpp —
/// `compute(..., auto_runner_end=false)`; `end_runners` только в конце
/// пайплайна) → страж model_manager_files.cpp:80 отказывает «cannot update
/// model sources during execution» → вся генерация в мусор (12 мин на 124
/// кадрах). Замер probe_bug3.sh (гейт до прогона): P1 голос — guard-отказ,
/// P2 без голоса (то же всё остальное) — успех. Встроенный апскейлер не
/// делает set_loader вовсе (интерполяция ~0.02с, лог бенч-кейса C14) → при
/// голосе работаем встроенным: рефайн завершается, голос и сид-реплей
/// сохраняются, детализация чуть проще learned. Вернуть learned-путь можно
/// будет после фикса апстрима (ранер audio-VAE гасить перед set_loader).
fn voice_forces_builtin_upscaler(learned: Option<&Path>, ref_audios: &[String]) -> bool {
    learned.is_some() && !ref_audios.is_empty()
}

/// Единый текст отказа движка рефайна с хвостом stderr (честные ошибки,
/// core §2.2): маркер ENGINE_LOG_REASON делает ошибку проходной для
/// is_honest_tool_error на хосте. Приоритет — строки [E]/[W] движка.
fn refine_failure_error(code: Option<i32>, tail: &[String]) -> String {
    let base = format!("sd-cli (рефайн) завершился с ошибкой (код {:?}).", code);
    if tail.is_empty() {
        return format!("{} Детали — в логе выше.", base);
    }
    let errors: Vec<&str> = tail
        .iter()
        .map(String::as_str)
        .filter(|l| l.contains("[E]") || l.contains("[W]"))
        .collect();
    let cause = if errors.is_empty() {
        tail.join(" | ")
    } else {
        errors.join(" | ")
    };
    format!("{} {} {}", base, ENGINE_LOG_REASON, cause)
}

/// Снимок хвоста stderr для Err-строки: короткая пауза на дренаж канала
/// (процесс уже завершился, читатель-поток добирает буфер до EOF), затем
/// копия накопленного. Пустой хвост (движок молчал) → пустой Vec.
fn drain_stderr_tail(tail: &Arc<std::sync::Mutex<Vec<String>>>) -> Vec<String> {
    std::thread::sleep(Duration::from_millis(250));
    tail.lock().map(|buf| buf.clone()).unwrap_or_default()
}

/// Аргументы CLI-вызова рефайна (чистая функция — герметичный тест).
///
/// Канва/кадры/seed/сэмплинг = параметры ДРАФТА (replay), hires-цели =
/// `refine` каталога. Промпт идёт через `--prompt-file` (UTF-8): позиционный
/// промпт и `-p` в этом семействе билдов глотают UTF-8 с искажениями
/// (бенч-инцидент R0). Голос карточки — `--ref-audio <путь к WAV>`
/// (CLI принимает файл напрямую, без b64-конверта HTTP-пути).
pub(crate) fn build_refine_args(
    diffusion: &Path,
    llm: &Path,
    vae: &Path,
    audio_vae: &Path,
    placement_flags: &str,
    lora_dir: Option<&Path>,
    prompt_file: &Path,
    out_path: &Path,
    seconds: u32,
    fps: u32,
    width: u32,
    height: u32,
    cfg_scale: f32,
    steps: u32,
    sample_method: &str,
    scheduler: &str,
    flow_shift: Option<f32>,
    seed: i64,
    refs: &[String],
    ref_image_size_max: bool,
    ref_audios: &[String],
    refine: &VideoRefinePreset,
    learned_upscaler: Option<&Path>,
) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    args.push("-M".into());
    args.push("vid_gen".into());
    // Весы бандла (паритет build_spawn_args: роли llm/vae/audio_vae + diffusion
    // по режиму запроса).
    args.push("--diffusion-model".into());
    args.push(diffusion.to_string_lossy().into_owned());
    args.push("--llm".into());
    args.push(llm.to_string_lossy().into_owned());
    args.push("--vae".into());
    args.push(vae.to_string_lossy().into_owned());
    args.push("--audio-vae".into());
    args.push(audio_vae.to_string_lossy().into_owned());
    // Placement-флаги из каталога (SSOT): "--params-backend ... --rng cpu --max-vram -2".
    args.extend(placement_flags.split_whitespace().map(str::to_string));
    // Канва драфта + кадры (движок сам выравнивает на сетку 4n+1).
    args.push("--video-frames".into());
    args.push((seconds * fps).to_string());
    args.push("--fps".into());
    args.push(fps.to_string());
    args.push("-W".into());
    args.push(width.to_string());
    args.push("-H".into());
    args.push(height.to_string());
    args.push("--cfg-scale".into());
    args.push(cfg_scale.to_string());
    args.push("--vae-tiling".into());
    // Драфт-план (turbo или euler-фоллбэк — replay боевого драфта, не бенч-константа).
    args.push("--steps".into());
    args.push(steps.to_string());
    args.push("--sampling-method".into());
    args.push(sample_method.to_string());
    args.push("--scheduler".into());
    args.push(scheduler.to_string());
    if let Some(shift) = flow_shift {
        args.push("--flow-shift".into());
        args.push(shift.to_string());
    }
    args.push("-s".into());
    args.push(seed.to_string());
    // Референсы Ref2VA (кадры карточки / вложения).
    for r in refs {
        args.push("-r".into());
        args.push(r.clone());
    }
    if ref_image_size_max && !refs.is_empty() {
        args.push("--ref-image-args".into());
        args.push("ref_image_size=max".into());
    }
    // Голос персонажа (voice cloning): voice.wav RefMod-карточки.
    for a in ref_audios {
        args.push("--ref-audio".into());
        args.push(a.clone());
    }
    // Turbo-LoRA драфта (--lora-model-dir грузит ВСЕ лоры каталога —
    // единственный механизм CLI; одиночного флага в билде нет).
    if let Some(dir) = lora_dir {
        args.push("--lora-model-dir".into());
        args.push(dir.to_string_lossy().into_owned());
    }
    // Промпт — только через файл (UTF-8, см. доки модуля).
    args.push("--prompt-file".into());
    args.push(prompt_file.to_string_lossy().into_owned());
    // Рефайн: латент-апскейл + второй проход (числа из каталога, гейт G5).
    args.push("--hires".into());
    args.push("--hires-width".into());
    args.push(refine.width.to_string());
    args.push("--hires-height".into());
    args.push(refine.height.to_string());
    args.push("--hires-steps".into());
    args.push(refine.steps.to_string());
    args.push("--hires-denoising-strength".into());
    args.push(refine.denoising_strength.to_string());
    if let Some(up) = learned_upscaler {
        args.push("--hires-upscalers-dir".into());
        args.push(up.parent().unwrap_or(Path::new(".")).to_string_lossy().into_owned());
        args.push("--hires-upscaler".into());
        // CLI ждёт стем без расширения.
        args.push(
            up.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| up.to_string_lossy().into_owned()),
        );
    }
    args.push("-o".into());
    args.push(out_path.to_string_lossy().into_owned());
    args
}

/// Рефайн последнего видео: spawn sd-cli с флагами --hires (путь B).
///
/// Все параметры = параметры сохранённого драфта (prompt/seconds/refs/voice/
/// seed) + hires-цели каталога. Пользовательские LoRA НЕ принимаются: CLI
/// этого билда не умеет одиночный мультипликатор, а replay драфта с другим
/// LoRA-стеком — уже другое видео (гейт честности; боевые драфты без лор —
/// RefMod-правило «лор не нужно», обычные запросы лор не получают).
#[allow(clippy::too_many_arguments)]
pub fn generate_video_refine<L>(
    engine_dir: &Path,
    bundle_dir: &Path,
    entry: &VideoBundleEntry,
    refine: &VideoRefinePreset,
    prompt: &str,
    seconds: u32,
    seed: i64,
    ref_paths: &[String],
    ref_image_size_max: bool,
    ref_audio_paths: &[String],
    out_path: &Path,
    cancel_flag: Option<Arc<AtomicBool>>,
    log_cb: L,
) -> Result<VideoGenResult, String>
where
    L: Fn(String),
{
    // Единовременность media-генерации (зеркало generate_video_simple).
    let _guard = MediaGenGuard::acquire()?;
    let cli = refine_engine_supported(engine_dir)?;
    log_cb(format!(
        "🔁 Рефайн: движок умеет hires ({}), конфиг каталога {}x{} шагов {} s{}",
        cli.display(),
        refine.width,
        refine.height,
        refine.steps,
        refine.denoising_strength
    ));

    let mut secs = seconds;
    if secs == 0 {
        log_cb("⚠️ seconds=0 → дефолт 5 сек.".to_string());
        secs = 5;
    }
    if secs > entry.preset.max_seconds {
        log_cb(format!(
            "⚠️ Клип {} сек выше лимита каталога «{}» ({} сек) — клампим.",
            secs, entry.name, entry.preset.max_seconds
        ));
        secs = entry.preset.max_seconds;
    }
    let refs: Vec<String> = ref_paths
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    // Рефайн проверен бенчем только для t2va/ref2va. i2va-драфт (первый кадр)
    // сюда не доходит: хост-гейт отказывает раньше (init не хранится для рефайна).
    let mode = if refs.is_empty() { "t2va" } else { "ref2va" };

    // Один замер ОЗУ для preflight и placement (паритет VideoEngine::new).
    let free_ram = free_ram_gb();
    let msg = preflight_video(entry, bundle_dir, free_ram)?;
    log_cb(format!("✅ {}", msg));
    // Политика размещения — зеркало build_spawn_args (video_engine.rs):
    // лестница placement; легаси-бандл без лестницы — строка spawn_flags
    // каталога (resolve_placement на пустой лестнице честно ошибается).
    let placement_flags: String = if entry.placement.is_empty() {
        let legacy = entry
            .spawn_flags
            .as_deref()
            .map(str::trim)
            .unwrap_or_default();
        if legacy.is_empty() {
            return Err(format!(
                "spawn_flags бандла «{}» пуст — флаги видео-спавна обязаны идти из каталога (SSOT), не из кода.",
                entry.name
            ));
        }
        legacy.to_string()
    } else {
        resolve_placement(entry, free_ram)
            .map(|v| {
                log_cb(format!(
                    "🧩 placement «{}» ({}): {}",
                    v.policy, v.label, v.spawn_flags
                ));
                v.spawn_flags.clone()
            })
            .map_err(|e| format!("{}{}", preflight::resource_markers::RAM, e))?
    };

    let diffusion = resolve_diffusion_file(entry, mode)?;
    let weights = bundle_files_weights(bundle_dir, &entry.files);
    let llm = weights.get("llm").ok_or("в бандле нет роли llm")?;
    let vae = weights.get("vae").ok_or("в бандле нет роли vae")?;
    let audio_vae = weights
        .get("audio_vae")
        .ok_or("в бандле нет роли audio_vae")?;

    // Драфт-план каталога (турбо-точка или euler-фоллбэк): replay боевого
    // драфта, не бенч-константа.
    let plan = sampling_plan_for_quality(entry, bundle_dir, false);
    log_cb(format!(
        "🎬 драфт-план рефайна: steps={}, {} / {}, shift={:?}, turbo-LoRA {}",
        plan.steps,
        plan.sample_method,
        plan.scheduler,
        plan.flow_shift,
        if plan.lora.is_empty() { "нет" } else { "есть" }
    ));
    let lora_dir = video_lora_spawn_dir(
        entry,
        bundle_dir,
        &crate::engine::config::video_lora_dir_early(),
    )?;

    let mut learned = learned_upscaler_file(bundle_dir, refine.upscaler_filename.as_ref());
    // БАГ-3: голос + learned-апскейлер = гарантированный отказ стража движка
    // (voice_forces_builtin_upscaler — там вся цепочка доказательств).
    let voice_builtin = voice_forces_builtin_upscaler(learned.as_deref(), ref_audio_paths);
    if voice_builtin {
        learned = None;
    }
    match (&refine.upscaler_filename, &learned) {
        (Some(name), Some(path)) => log_cb(format!(
            "🔼 апскейлер рефайна: learned «{}» ({})",
            name,
            path.display()
        )),
        (Some(name), None) if voice_builtin => log_cb(format!(
            "⚠️ learned-апскейлер «{}» пропущен намеренно: в этом движке загрузка learned \
             при озвучке падает («cannot update model sources during execution» — ранер \
             голоса жив во время hires-фазы, апстрим f5df5f8). Работаем встроенным \
             апскейлером: рефайн завершится, голос и сид сохранятся; детализация чуть \
             проще learned.",
            name
        )),
        (Some(name), None) => log_cb(format!(
            "⚠️ learned-апскейлер «{}» не найден в {} — берётся встроенный апскейлер движка \
             (детали чуть проще бенча E). Файл скачается с бандлом при следующем \
             обновлении набора моделей.",
            name,
            bundle_dir.display()
        )),
        _ => log_cb("🔼 апскейлер рефайна: встроенный движка".to_string()),
    }

    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Не удалось создать папку {}: {}", parent.display(), e))?;
        }
    }
    let prompt_file = out_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("refine_prompt.txt");
    std::fs::write(&prompt_file, prompt)
        .map_err(|e| format!("Не удалось записать промпт-файл: {}", e))?;

    let args = build_refine_args(
        &bundle_dir.join(&diffusion.filename),
        Path::new(llm),
        Path::new(vae),
        Path::new(audio_vae),
        &placement_flags,
        lora_dir.as_deref(),
        &prompt_file,
        out_path,
        secs,
        entry.preset.fps,
        entry.preset.width,
        entry.preset.height,
        entry.preset.cfg_scale,
        plan.steps,
        &plan.sample_method,
        &plan.scheduler,
        plan.flow_shift,
        seed,
        &refs,
        ref_image_size_max,
        ref_audio_paths,
        refine,
        learned.as_deref(),
    );
    log_cb(format!("🎬 spawn sd-cli (рефайн): {} {}", cli.display(), args.join(" ")));

    let mut cmd = std::process::Command::new(&cli);
    cmd.args(&args);
    // CUDA-DLL: общая папка backends/_cudart (паритет spawn sd-server).
    if let Some(p) = sdcpp_installer::child_path_with_cudart(engine_dir) {
        cmd.env("PATH", p);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS — паритет sd-server.
        cmd.creation_flags(0x0800_0000 | 0x0000_4000);
    }
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Не удалось запустить sd-cli (рефайн): {}", e))?;
    register_engine_pid(child.id());
    assign_child_to_kill_job(&child);
    log_cb(format!("✅ sd-cli (рефайн) запущен (PID {})", child.id()));
    let stderr_tail: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
    pipe_log(child.stdout.take(), "sd-cli-refine");
    pipe_log_with_tail(child.stderr.take(), "sd-cli-refine", Some(stderr_tail.clone()));

    let t0 = Instant::now();
    loop {
        if let Some(flag) = &cancel_flag {
            if flag.load(Ordering::Relaxed) {
                kill_process_tree(&mut child);
                let _ = std::fs::remove_file(&prompt_file);
                return Err("Генерация отменена пользователем.".to_string());
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                let _ = std::fs::remove_file(&prompt_file);
                if status.success() {
                    break;
                }
                return Err(refine_failure_error(
                    status.code(),
                    &drain_stderr_tail(&stderr_tail),
                ));
            }
            Ok(None) => {}
            Err(e) => {
                let _ = std::fs::remove_file(&prompt_file);
                return Err(format!("Ошибка ожидания sd-cli: {}", e));
            }
        }
        if t0.elapsed() > VIDEO_GEN_TIMEOUT {
            kill_process_tree(&mut child);
            let _ = std::fs::remove_file(&prompt_file);
            let tail = drain_stderr_tail(&stderr_tail);
            let timeout = "Таймаут рефайна (60 мин) — процесс остановлен.".to_string();
            if tail.is_empty() {
                return Err(timeout);
            }
            return Err(format!(
                "{} {} {}",
                timeout,
                ENGINE_LOG_REASON,
                tail.join(" | ")
            ));
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    if !out_path.exists() {
        let tail = drain_stderr_tail(&stderr_tail);
        let base =
            "sd-cli отчитался успехом, но файл клипа не создан — смотри лог движка выше.".to_string();
        if tail.is_empty() {
            return Err(base);
        }
        return Err(format!(
            "{} {} {}",
            base,
            ENGINE_LOG_REASON,
            tail.join(" | ")
        ));
    }
    let time_sec = t0.elapsed().as_secs_f64();
    log_cb(format!(
        "✅ Рефайн готов за {:.1} сек → {}",
        time_sec,
        out_path.display()
    ));
    // frame_count номинальный (seconds × fps): фактическое число после сетки
    // 4n+1 CLI-лог не отдаёт структурированно; хост это поле не потребляет.
    Ok(VideoGenResult {
        path: out_path.to_string_lossy().into_owned(),
        fps: entry.preset.fps,
        frame_count: secs as u64 * entry.preset.fps as u64,
        mime_type: "video/webm".to_string(),
        time_sec,
        seed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refine_preset() -> VideoRefinePreset {
        VideoRefinePreset {
            width: 1216,
            height: 672,
            steps: 4,
            denoising_strength: 0.4,
            upscaler_filename: Some("minimax_h3_latent_upscaler_3d_conv_v1_bf16".to_string()),
        }
    }

    /// Индексы вхождений ключа (пары «ключ → значение» проверяются позиционно).
    fn positions(args: &[String], key: &str) -> Vec<usize> {
        args.iter()
            .enumerate()
            .filter(|(_, a)| *a == key)
            .map(|(i, _)| i)
            .collect()
    }

    fn value_after<'a>(args: &'a [String], key: &str) -> Option<&'a str> {
        let i = *positions(args, key).first()?;
        args.get(i + 1).map(|s| s.as_str())
    }

    /// Полный набор флагов рефайна: replay-параметры драфта (канва, кадры,
    /// турбо-план, сид, рефы, голос) + hires-цели каталога + learned-апскейлер.
    #[test]
    fn refine_args_replay_draft_and_hires_from_catalog() {
        let refine = refine_preset();
        let up = Path::new("C:/models/upscaler.safetensors");
        let args = build_refine_args(
            Path::new("C:/b/diffusion.gguf"),
            Path::new("C:/b/llm.gguf"),
            Path::new("C:/b/vae.safetensors"),
            Path::new("C:/b/audio_vae.safetensors"),
            "--params-backend te=disk,diffusion=cpu,vae=disk --rng cpu --max-vram -2",
            Some(Path::new("C:/vlora")),
            Path::new("C:/out/refine_prompt.txt"),
            Path::new("C:/out/clip.webm"),
            5,   // seconds
            24,  // fps
            864, // width (драфт)
            480, // height (драфт)
            1.0, // cfg
            4,   // steps (турбо)
            "lcm",
            "simple",
            Some(12.0),
            777, // seed
            &["C:/refs/f1.png".to_string(), "C:/refs/f2.png".to_string()],
            true,
            &["C:/card/voice.wav".to_string()],
            &refine,
            Some(up),
        );
        // Режим и весы.
        assert_eq!(positions(&args, "-M"), vec![0]);
        assert_eq!(value_after(&args, "-M"), Some("vid_gen"));
        assert_eq!(value_after(&args, "--diffusion-model"), Some("C:/b/diffusion.gguf"));
        assert_eq!(value_after(&args, "--llm"), Some("C:/b/llm.gguf"));
        assert_eq!(value_after(&args, "--vae"), Some("C:/b/vae.safetensors"));
        assert_eq!(value_after(&args, "--audio-vae"), Some("C:/b/audio_vae.safetensors"));
        // Placement-флаги каталога — разрезаны по пробелам.
        assert!(args.iter().any(|a| a == "--params-backend"));
        assert_eq!(value_after(&args, "--max-vram"), Some("-2"));
        // Канва и кадры ДРАФТА (864×480, 5с × 24fps), не hires-цели.
        assert_eq!(value_after(&args, "--video-frames"), Some("120"));
        assert_eq!(value_after(&args, "-W"), Some("864"));
        assert_eq!(value_after(&args, "-H"), Some("480"));
        // Турбо-план драфта + сид.
        assert_eq!(value_after(&args, "--steps"), Some("4"));
        assert_eq!(value_after(&args, "--sampling-method"), Some("lcm"));
        assert_eq!(value_after(&args, "--scheduler"), Some("simple"));
        assert_eq!(value_after(&args, "--flow-shift"), Some("12"));
        assert_eq!(value_after(&args, "-s"), Some("777"));
        // Референсы: по -r на каждый + ref_image_size=max.
        assert_eq!(positions(&args, "-r").len(), 2);
        assert_eq!(value_after(&args, "--ref-image-args"), Some("ref_image_size=max"));
        // Голос карточки — файл напрямую (CLI-путь).
        assert_eq!(value_after(&args, "--ref-audio"), Some("C:/card/voice.wav"));
        // Turbo-LoRA каталогом (единственный механизм CLI).
        assert_eq!(value_after(&args, "--lora-model-dir"), Some("C:/vlora"));
        // Промпт — только через файл (UTF-8-квирка позиционного промпта).
        assert_eq!(value_after(&args, "--prompt-file"), Some("C:/out/refine_prompt.txt"));
        // Hires-цели — ТОЛЬКО из пресета каталога (гейт G5).
        assert!(args.iter().any(|a| a == "--hires"));
        assert_eq!(value_after(&args, "--hires-width"), Some("1216"));
        assert_eq!(value_after(&args, "--hires-height"), Some("672"));
        assert_eq!(value_after(&args, "--hires-steps"), Some("4"));
        assert_eq!(value_after(&args, "--hires-denoising-strength"), Some("0.4"));
        // Learned-апскейлер: каталог + стем БЕЗ расширения.
        assert_eq!(
            value_after(&args, "--hires-upscalers-dir"),
            Some("C:/models")
        );
        assert_eq!(
            value_after(&args, "--hires-upscaler"),
            Some("upscaler"),
            "CLI ждёт стем без расширения"
        );
        assert_eq!(value_after(&args, "-o"), Some("C:/out/clip.webm"));
    }

    /// Опциональные части не подставляются пустыми: без learned-апскейлера нет
    /// --hires-upscaler*; без shift нет --flow-shift; без рефов нет -r и
    /// ref-image-args; без lora-dir нет --lora-model-dir.
    #[test]
    fn refine_args_omit_absent_optionals() {
        let refine = VideoRefinePreset {
            width: 1216,
            height: 672,
            steps: 4,
            denoising_strength: 0.4,
            upscaler_filename: None,
        };
        let args = build_refine_args(
            Path::new("d.gguf"),
            Path::new("l.gguf"),
            Path::new("v.safetensors"),
            Path::new("a.safetensors"),
            "--offload-to-cpu --rng cpu",
            None,
            Path::new("p.txt"),
            Path::new("c.webm"),
            4,
            24,
            864,
            480,
            1.0,
            6, // euler-фоллбэк без лор
            "euler",
            "discrete",
            None,
            42,
            &[],
            false,
            &[],
            &refine,
            None,
        );
        assert!(positions(&args, "--hires-upscaler").is_empty());
        assert!(positions(&args, "--hires-upscalers-dir").is_empty());
        assert!(positions(&args, "--flow-shift").is_empty());
        assert!(positions(&args, "-r").is_empty());
        assert!(positions(&args, "--ref-image-args").is_empty());
        assert!(positions(&args, "--ref-audio").is_empty());
        assert!(positions(&args, "--lora-model-dir").is_empty());
        // Placement легаси-бандла разрезан по пробелам.
        assert!(args.iter().any(|a| a == "--offload-to-cpu"));
        // Hires-цели всё равно из пресета (не опциональны).
        assert_eq!(value_after(&args, "--hires-width"), Some("1216"));
        assert_eq!(value_after(&args, "-s"), Some("42"));
    }

    /// Learned-апскейлер: стем без расширения и с расширением резолвятся в
    /// файл bundle_dir; отсутствующий файл → None (встроенный, гейт G3).
    #[test]
    fn learned_upscaler_file_resolves_stem_or_direct() {
        let dir = std::env::temp_dir().join(format!(
            "ko_refine_up_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        // Стем без расширения (канон каталога).
        std::fs::write(dir.join("up.safetensors"), b"x").unwrap();
        let got = learned_upscaler_file(&dir, Some("up".to_string()).as_ref()).expect("стем → файл");
        assert!(got.ends_with("up.safetensors"), "{}", got.display());
        // Имя с расширением — прямое чтение.
        let got =
            learned_upscaler_file(&dir, Some("up.safetensors".to_string()).as_ref())
                .expect("прямое имя → файл");
        assert!(got.ends_with("up.safetensors"));
        // Файла нет → None (не отказ: встроенный апскейлер движка).
        assert!(learned_upscaler_file(&dir, Some("missing".to_string()).as_ref()).is_none());
        assert!(learned_upscaler_file(&dir, None).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// БАГ-3 (06.11.26): голос + learned-апскейлер → встроенный (страж
    /// set_loader в движке); без голоса learned остаётся; без learned гейта нет.
    #[test]
    fn voice_forces_builtin_upscaler_only_with_voice() {
        let learned = Path::new("up.safetensors");
        assert!(voice_forces_builtin_upscaler(Some(learned), &["voice.wav".to_string()]));
        assert!(!voice_forces_builtin_upscaler(Some(learned), &[]));
        assert!(!voice_forces_builtin_upscaler(None, &["voice.wav".to_string()]));
    }

    /// Отказ движка с хвостом stderr несёт маркер ENGINE_LOG_REASON и сами
    /// строки [E]; приоритет ошибочных строк перед информационными.
    #[test]
    fn refine_failure_error_carries_engine_tail() {
        let tail = vec![
            "[I] loading tensors completed, taking 0.21s".to_string(),
            "[E] cannot update model sources during execution --- model_manager_files.cpp:80"
                .to_string(),
            "[E] generate failed --- main.cpp:926".to_string(),
        ];
        let err = refine_failure_error(Some(1), &tail);
        assert!(err.contains(ENGINE_LOG_REASON), "{}", err);
        assert!(err.contains("model_manager_files.cpp:80"), "{}", err);
        assert!(err.contains("generate failed"), "{}", err);
        assert!(!err.contains("loading tensors"), "{}", err);
        // Боевой кейс БАГ-3 (12 мин ожидания, глухая ошибка) теперь честный.
        let silent = refine_failure_error(Some(1), &[]);
        assert!(!silent.contains(ENGINE_LOG_REASON), "{}", silent);
        assert!(silent.contains("Детали — в логе выше"), "{}", silent);
    }
}
