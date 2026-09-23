//! ImageEngine — генерация/редактирование через ОТДЕЛЬНЫЙ процесс `sd-server.exe`.
//!
//! Контракт (проверен по исходникам leejet/stable-diffusion.cpp):
//! - запуск: `sd-server --diffusion-model … --vae … --llm … [--llm-vision …]
//!   --listen-ip 127.0.0.1 --listen-port <rand> --offload-to-cpu --diffusion-fa`
//!   (модель грузится ДО listen — готовность = успешный HTTP-ответ);
//! - генерация: `POST /sdcpp/v1/img_gen` (нативный async API) → 202 `{id, poll_url}` →
//!   poll `GET /sdcpp/v1/jobs/{id}` до `completed` → `result.images[0].b64_json`;
//! - референсы (`-r` в CLI) в HTTP — массив `ref_images[]` (base64/data-URL),
//!   **порядок массива = порядок conditioning** (маппинг аттачментов чата 1-в-1);
//! - api-key у sd-server НЕТ (httplib без auth) — изоляция только bind 127.0.0.1.
//!
//! Жизнь — per-request spawn: создал → сгенерировал → Drop убил дерево.
//! Зачистка: Drop → kill_process_tree + Windows Job Object KILL_ON_JOB_CLOSE.

use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::engine::models_catalog::ImageBundleEntry;
use crate::engine::preflight::{preflight_check, PreflightVerdict};
use crate::engine::process_util::{
    assign_child_to_kill_job, kill_process_tree, register_engine_pid, unregister_engine_pid,
};
use crate::engine::sdcpp_installer;

/// Диапазон портов image-движка (не пересекается с llama 17800..19300).
const PORT_MIN: u16 = 19400;
const PORT_RANGE: u16 = 1500;
/// Ожидание готовности сервера (загрузка 8B-энкодера + diffusion в память).
const HEALTH_TIMEOUT: Duration = Duration::from_secs(300);
/// Общий таймаут одной генерации (долгая операция, минуты).
const GEN_TIMEOUT: Duration = Duration::from_secs(1800);
/// Интервал опроса job-статуса.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Serialize, Clone, Debug)]
pub struct ImageGenResult {
    /// Путь к сохранённому PNG.
    pub path: String,
    /// Seed (фактический; -1 = сервер выбрал случайный).
    pub seed: i64,
    /// Длительность генерации, сек.
    pub time_sec: f64,
}

#[derive(Deserialize, Debug)]
struct JobSubmit {
    id: String,
    #[allow(dead_code)]
    kind: Option<String>,
    #[allow(dead_code)]
    status: Option<String>,
    #[allow(dead_code)]
    poll_url: Option<String>,
}

#[derive(Deserialize, Debug)]
struct JobStatus {
    #[allow(dead_code)]
    id: Option<String>,
    status: String,
    result: Option<JobResult>,
    error: Option<JobError>,
}

#[derive(Deserialize, Debug)]
struct JobResult {
    images: Option<Vec<JobImage>>,
}

#[derive(Deserialize, Debug)]
struct JobImage {
    b64_json: String,
}

#[derive(Deserialize, Debug)]
struct JobError {
    #[allow(dead_code)]
    code: Option<String>,
    message: Option<String>,
}

/// Найти свободный порт bind-проверкой (не хардкодить — конфликты параллельных запусков).
fn pick_free_port() -> Option<u16> {
    // Простой детерминированный обход со случайным стартом (xorshift от времени+PID).
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E3779B97F4A7C15)
        ^ (std::process::id() as u64).wrapping_mul(0xBF58476D1CE4E5B9);
    let mut xorshift = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let start = (xorshift() % PORT_RANGE as u64) as u16;
    for i in 0..5 {
        let candidate = PORT_MIN + (start + i * 137) % PORT_RANGE;
        if std::net::TcpListener::bind(("127.0.0.1", candidate)).is_ok() {
            return Some(candidate);
        }
    }
    None
}

pub struct ImageEngine {
    child: std::process::Child,
    base_url: String,
    client: reqwest::blocking::Client,
}

impl ImageEngine {
    /// Spawn sd-server с файлами бандла. Preflight — до spawn (понятная ошибка
    /// вместо падения сервера); вердикт Offload → стейджинг-флаги уже в cmd.
    pub fn new<L>(
        engine_dir: &Path,
        bundle_dir: &Path,
        entry: &ImageBundleEntry,
        variant_pref: Option<&str>,
        log_cb: L,
    ) -> Result<Self, String>
    where
        L: Fn(String),
    {
        let source = "leejet";
        let variant = sdcpp_installer::resolve_variant(variant_pref);
        let mut server_exe = sdcpp_installer::variant_dir(engine_dir, source, &variant)
            .join("sd-server.exe");
        if !server_exe.exists() {
            // legacy root fallback
            let legacy = engine_dir.join("sd-server.exe");
            if legacy.exists() {
                server_exe = legacy;
            } else {
                return Err(format!(
                    "Движок изображений не установлен (нет sd-server.exe).\n\
                     Откройте Настройки → «Движок изображений» и нажмите «Установить движок»."
                ));
            }
        }

        // ── Preflight ДО spawn ──
        let (verdict, est, msg) = preflight_check(entry);
        log_cb(format!(
            "💾 Бандл: ~{:.1} ГБ на диске; VRAM full ~{:.1} ГБ / минимум ~{:.1} ГБ; RAM нужно ~{:.1} ГБ.",
            est.disk_mb / 1024.0,
            est.full_mb / 1024.0,
            est.min_mb / 1024.0,
            est.ram_need_mb / 1024.0
        ));
        match verdict {
            PreflightVerdict::Fast => log_cb(format!("✅ {}", msg)),
            PreflightVerdict::Offload => log_cb(format!("⚙️ {}", msg)),
            PreflightVerdict::Insufficient => return Err(msg),
        }

        // ── Файлы бандла обязаны существовать ──
        let weights = crate::engine::models_catalog::bundle_weights_files(bundle_dir, entry);
        for role in ["diffusion", "vae", "llm"] {
            let p = weights.get(role).cloned().unwrap_or_default();
            if !Path::new(&p).exists() {
                return Err(format!(
                    "Файл бандла не найден ({}): {}\nСкачайте бандл: Настройки → «Движок изображений».",
                    role, p
                ));
            }
        }

        let port = pick_free_port()
            .ok_or_else(|| "Не найден свободный порт для sd-server (19400..20900).".to_string())?;

        let mut cmd = std::process::Command::new(&server_exe);
        cmd.args([
            "--diffusion-model",
            &weights["diffusion"],
            "--vae",
            &weights["vae"],
            "--llm",
            &weights["llm"],
        ]);
        if let Some(mmproj) = weights.get("mmproj") {
            if Path::new(mmproj).exists() {
                // Vision-веса для editing (флаг с подчёркиванием — как в docs/qwen_image_2.1.md).
                cmd.args(["--llm_vision", mmproj]);
            }
        }
        cmd.args(["--listen-ip", "127.0.0.1"]);
        cmd.args(["--listen-port", &port.to_string()]);
        // Память: стейджинг весов из RAM + flash attention diffusion (CUDA быстрее и меньше VRAM).
        cmd.args(["--offload-to-cpu", "--diffusion-fa", "--auto-fit", "on"]);
        cmd.current_dir(engine_dir);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW — без мелькания консоли
        }
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| format!("Не удалось запустить sd-server: {}", e))?;
        register_engine_pid(child.id());
        assign_child_to_kill_job(&child);

        // stderr сервера — в лог хоста отдельным потоком (CUDA/ggml-ошибки).
        if let Some(stderr) = child.stderr.take() {
            std::thread::spawn(move || {
                let mut reader = std::io::BufReader::new(stderr);
                let mut line = String::new();
                use std::io::BufRead;
                while reader.read_line(&mut line).is_ok() {
                    if line.is_empty() {
                        break;
                    }
                    log::info!("[sd-server] {}", line.trim_end());
                    line.clear();
                }
            });
        }

        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| format!("Ошибка HTTP-клиента: {}", e))?;
        let engine = Self {
            child,
            base_url: format!("http://127.0.0.1:{}", port),
            client,
        };
        engine.wait_ready(&log_cb)?;
        Ok(engine)
    }

    /// Ожидание готовности: модель грузится ДО listen — первый успешный
    /// `/sdcpp/v1/capabilities` = сервер поднят с весами.
    fn wait_ready<L>(&self, log_cb: L) -> Result<(), String>
    where
        L: Fn(String),
    {
        log_cb("⏳ Загрузка бандла в sd-server…".to_string());
        let start = Instant::now();
        loop {
            match self.client.get(format!("{}/sdcpp/v1/capabilities", self.base_url)).send() {
                Ok(resp) if resp.status().is_success() => {
                    log_cb("✅ sd-server готов.".to_string());
                    return Ok(());
                }
                _ => {}
            }
            if start.elapsed() > HEALTH_TIMEOUT {
                return Err("sd-server не стал готов за 300 сек (см. лог [sd-server]).".to_string());
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    }

    /// Генерация: submit → poll → PNG на диск. `ref_images_b64` — base64/data-URL
    /// в порядке conditioning (пусто = t2i).
    pub fn generate<L>(
        &self,
        prompt: &str,
        width: u32,
        height: u32,
        steps: u32,
        cfg_scale: f32,
        seed: i64,
        ref_images_b64: &[String],
        out_path: &Path,
        cancel_flag: Option<Arc<AtomicBool>>,
        log_cb: L,
    ) -> Result<ImageGenResult, String>
    where
        L: Fn(String),
    {
        let w = round32(width);
        let h = round32(height);
        if w != width || h != height {
            log_cb(format!("ℹ️ Размер скорректирован до кратного 32: {}x{} → {}x{}.", width, height, w, h));
        }
        let body = serde_json::json!({
            "prompt": prompt,
            "width": w,
            "height": h,
            "seed": seed,
            "batch_count": 1,
            "ref_images": ref_images_b64,
            "sample_params": {
                "sample_method": "euler",
                "sample_steps": steps,
                "guidance": { "txt_cfg": cfg_scale },
            },
            "output_format": "png",
        });
        log_cb(format!(
            "🎨 Генерация: {}x{}, steps={}, cfg={}, refs={}…",
            w, h, steps, cfg_scale, ref_images_b64.len()
        ));
        let t0 = Instant::now();

        let submit: JobSubmit = self
            .client
            .post(format!("{}/sdcpp/v1/img_gen", self.base_url))
            .json(&body)
            .send()
            .map_err(|e| format!("Ошибка submit img_gen: {}", e))?
            .error_for_status()
            .map_err(|e| format!("sd-server отклонил запрос: {}", e))?
            .json()
            .map_err(|e| format!("Ошибка парсинга ответа submit: {}", e))?;

        let b64 = self.poll_job(&submit.id, cancel_flag, &log_cb)?;
        let bytes = base64_decode(&b64)?;
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Не удалось создать папку {}: {}", parent.display(), e))?;
        }
        std::fs::write(out_path, &bytes)
            .map_err(|e| format!("Не удалось записать {}: {}", out_path.display(), e))?;
        let time_sec = t0.elapsed().as_secs_f64();
        log_cb(format!(
            "✅ Готово за {:.1} сек → {}",
            time_sec,
            out_path.display()
        ));
        Ok(ImageGenResult {
            path: out_path.to_string_lossy().to_string(),
            seed,
            time_sec,
        })
    }

    /// Poll job до completed/failed/cancelled. Отмена — через cancel_flag.
    fn poll_job<L>(
        &self,
        job_id: &str,
        cancel_flag: Option<Arc<AtomicBool>>,
        log_cb: L,
    ) -> Result<String, String>
    where
        L: Fn(String),
    {
        let start = Instant::now();
        loop {
            if let Some(flag) = &cancel_flag {
                if flag.load(Ordering::SeqCst) {
                    let _ = self.client.post(format!("{}/sdcpp/v1/jobs/{}/cancel", self.base_url, job_id)).send();
                    return Err("CANCELLED".to_string());
                }
            }
            let st: JobStatus = self
                .client
                .get(format!("{}/sdcpp/v1/jobs/{}", self.base_url, job_id))
                .send()
                .map_err(|e| format!("Ошибка poll job: {}", e))?
                .json()
                .map_err(|e| format!("Ошибка парсинга job: {}", e))?;
            match st.status.as_str() {
                "completed" => {
                    let b64 = st
                        .result
                        .and_then(|r| r.images)
                        .and_then(|mut v| v.pop())
                        .map(|img| img.b64_json)
                        .ok_or_else(|| "Job completed, но без изображений.".to_string())?;
                    return Ok(b64);
                }
                "failed" => {
                    let msg = st.error.map(|e| e.message.unwrap_or_default()).unwrap_or_default();
                    return Err(format!("Генерация не удалась: {}", msg));
                }
                "cancelled" => return Err("CANCELLED".to_string()),
                _ => {}
            }
            if start.elapsed() > GEN_TIMEOUT {
                let _ = self.client.post(format!("{}/sdcpp/v1/jobs/{}/cancel", self.base_url, job_id)).send();
                return Err("Таймаут генерации (30 мин) — job отменён.".to_string());
            }
            log_cb(format!("⏳ Генерация… {:.0} сек", start.elapsed().as_secs()));
            std::thread::sleep(POLL_INTERVAL);
        }
    }
}

impl Drop for ImageEngine {
    fn drop(&mut self) {
        let pid = self.child.id();
        kill_process_tree(&mut self.child);
        let _ = self.child.wait();
        unregister_engine_pid(pid);
    }
}

/// Размеры — кратно 32 (требование Qwen Image 2.1).
fn round32(v: u32) -> u32 {
    ((v + 31) / 32 * 32).max(32)
}

fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    // Принимаем и чистый base64, и data-URL.
    let raw = s.split_once(',').map(|(_, b)| b).unwrap_or(s);
    base64::engine::general_purpose::STANDARD
        .decode(raw.trim())
        .map_err(|e| format!("Ошибка декодирования base64 картинки: {}", e))
}

/// One-shot t2i: spawn → generate → Drop. Точка входа per-request архитектуры.
pub fn generate_image_simple<L>(
    engine_dir: &Path,
    bundle_dir: &Path,
    entry: &ImageBundleEntry,
    variant_pref: Option<&str>,
    prompt: &str,
    width: u32,
    height: u32,
    steps: u32,
    cfg_scale: f32,
    seed: i64,
    out_path: &Path,
    cancel_flag: Option<Arc<AtomicBool>>,
    log_cb: L,
) -> Result<ImageGenResult, String>
where
    L: Fn(String) + Clone,
{
    let engine = ImageEngine::new(engine_dir, bundle_dir, entry, variant_pref, log_cb.clone())?;
    engine.generate(prompt, width, height, steps, cfg_scale, seed, &[], out_path, cancel_flag, log_cb)
}

/// One-shot edit: референсы читаются с диска В ПОРЯДКЕ массива → ref_images[].
pub fn edit_image_files<L>(
    engine_dir: &Path,
    bundle_dir: &Path,
    entry: &ImageBundleEntry,
    variant_pref: Option<&str>,
    prompt: &str,
    ref_paths: &[String],
    width: u32,
    height: u32,
    steps: u32,
    cfg_scale: f32,
    seed: i64,
    out_path: &Path,
    cancel_flag: Option<Arc<AtomicBool>>,
    log_cb: L,
) -> Result<ImageGenResult, String>
where
    L: Fn(String) + Clone,
{
    let mut refs_b64 = Vec::with_capacity(ref_paths.len());
    for p in ref_paths {
        let mut f = std::fs::File::open(p).map_err(|e| format!("Не найден референс {}: {}", p, e))?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).map_err(|e| format!("Ошибка чтения {}: {}", p, e))?;
        use base64::Engine;
        refs_b64.push(base64::engine::general_purpose::STANDARD.encode(&buf));
    }
    let engine = ImageEngine::new(engine_dir, bundle_dir, entry, variant_pref, log_cb.clone())?;
    engine.generate(prompt, width, height, steps, cfg_scale, seed, &refs_b64, out_path, cancel_flag, log_cb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round32_snaps_to_multiples() {
        assert_eq!(round32(1024), 1024);
        assert_eq!(round32(1000), 1024);
        assert_eq!(round32(1), 32);
    }

    #[test]
    fn base64_decode_accepts_data_url() {
        use base64::Engine;
        let raw = base64::engine::general_purpose::STANDARD.encode([1u8, 2, 3]);
        let url = format!("data:image/png;base64,{}", raw);
        assert_eq!(base64_decode(&url).unwrap(), vec![1, 2, 3]);
        assert_eq!(base64_decode(&raw).unwrap(), vec![1, 2, 3]);
    }
}
