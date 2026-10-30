//! LoRA-стек image-движка (PRP 29.10.26): единственное место LoRA-логики
//! image-ветки — структура payload-поля, резолвер имён, спавн-аргументы и
//! сканер строки применения. Хост (`media.rs`) и tauri-команды зовут только
//! этот модуль (SSOT); `image_engine.rs` / `commands.rs` — только вызовы.

use std::path::Path;

use serde::Serialize;

/// Одна LoRA тела `POST /sdcpp/v1/img_gen`. `path` — ИМЯ ФАЙЛА относительно
/// каталога `--lora-model-dir` спавна: абсолютные пути и пути вне рута
/// сервер отклоняет; тег `<lora:...>` в промпте сервер принципиально не
/// парсит — только структурные поля (api.md sd.cpp, разделы «LoRA fields»
/// и «LoRA Rules»). Формат разделяется с видео-веткой (`VideoLoraRef`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LoraSpec {
    pub path: String,
    pub multiplier: f32,
}

/// Верхняя граница multiplier: выше — честный отказ на границе резолва;
/// 0..=2 — консервативная вилка (решение юзера: «число, дефолт 1.0»).
pub const LORA_MULTIPLIER_MAX: f32 = 2.0;

/// Приоритет расширений при стем-коллизии (меньший индекс wins):
/// safetensors — стандарт Civitai/HF, прочее — экзотика.
const LORA_PREFERRED_EXT: &[&str] = &[
    ".safetensors", ".sft", ".gguf", ".ckpt", ".pt", ".pth", ".bin",
];

/// Clamp multiplier на границе плагина: NaN/∞ → 0, вне 0..=2 — в вилку.
pub fn clamp_multiplier(m: f32) -> f32 {
    if !m.is_finite() {
        return 0.0;
    }
    m.clamp(0.0, LORA_MULTIPLIER_MAX)
}

/// Спавн-аргументы `--lora-model-dir <dir>`. Папки нет → пустой вектор:
/// генерация без лоры обязана работать как раньше, а заявка на несуществующую
/// лору честно падает в `resolve_lora_names` (не молчание).
pub fn lora_spawn_args(dir: &Path) -> Vec<String> {
    lora_spawn_args_for_base(dir, None)
}

/// Спавн-аргументы базы: `--lora-model-dir <dir>/<base>` (F3, 29.10.26).
/// Подпапку базы создаём автоматически, если корень лор существует: пустая
/// папка — легитимное состояние «лор ещё не клали», а аргумент без папки
/// может приводить сервер к плохому состоянию каталога. Создание
/// идемпотично и не трогает файлы юзера (перенос лор вручную по F3).
pub fn lora_spawn_args_for_base(dir: &Path, base: Option<&str>) -> Vec<String> {
    let target = match base {
        Some(base) if !base.trim().is_empty() => dir.join(base.trim()),
        _ => dir.to_path_buf(),
    };
    if !target.is_dir() && base.is_some() && dir.is_dir() {
        let _ = std::fs::create_dir_all(&target);
    }
    if target.is_dir() {
        vec![
            "--lora-model-dir".to_string(),
            target.to_string_lossy().into_owned(),
        ]
    } else {
        Vec::new()
    }
}

/// Резолв имён-стемов → `LoraSpec` (flat-каталог, стем-матчинг без учёта
/// регистра; коллизия одного стема — по приоритету расширений). Ошибка
/// перечисляет папку и доступные стемы — это текстово-честное сообщение
/// агенту/юзеру, не «тихий пропуск».
pub fn resolve_lora_names(
    dir: &Path,
    names: &[String],
    multiplier: f32,
) -> Result<Vec<LoraSpec>, String> {
    let wanted: Vec<(String, f32)> = names.iter().map(|n| (n.clone(), multiplier)).collect();
    resolve_lora_specs(dir, &wanted)
}

/// Резолв пар (имя, сила) → `LoraSpec`. Индивидуальная сила задаётся на
/// элемент (PRP 29.10.26 D2: слайдер-лоры требуют 1.0–2.5 — `strength` на
/// элементе приоритетнее общей). `resolve_lora_names` — обёртка над этим
/// (единая сила на все имена). Ядро матчинга одно — здесь.
pub fn resolve_lora_specs(
    dir: &Path,
    wanted: &[(String, f32)],
) -> Result<Vec<LoraSpec>, String> {
    resolve_lora_specs_for_base(dir, wanted, None)
}

/// Ключ пунктуационно-нечувствительного сопоставления: нижний регистр,
/// только буквы и цифры. «Minimax_H3_Unlocked_V2.1» и
/// «minimax_h3_unlocked_v2_1» дают один ключ — транскрипционные ошибки
/// LLM (точка↔подчёркивание↔пробел↔регистр) перестают быть отказом.
fn normalized_stem(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

/// Каталог базы моделей: лоры разложены по ПОДПАПКАМ на базу (решение
/// юзера 29.10.26, F3). Лора — набор дельта-тензоров, привязанных к именам и
/// размерностям слоёв конкретного диффузионного бэкбона (Qwen-Image ≠
/// Krea-2). sd.cpp сопоставляет тензоры по именам: чужая лора либо тихо
/// применится частично («шум/искажение»), либо упадёт. Поэтому кросс-база
/// отсекается СТРУКТУРНО: резолв ищет файлы только в подпапке базы активного
/// бандла — лора чужой базы физически отсутствует в каталоге сервера
/// (`--lora-model-dir` = `<dir>/<base>`), отказ невозможен по design.
/// `base = None` — легаси-плоская папка (видео-ветка и старые вызовы).
///
/// Сопоставление двухфазное: точный стем (без учёта регистра), затем —
/// пунктуационно-нормализованный ключ (`normalized_stem`): LLM, переписывая
/// стем из каталога, путает точку с подчёркиванием — такой запрос обязан
/// находить файл, а не валить генерацию (инцидент 30.10.26: запрос
/// «…V2_1» против файла «…V2.1»). Внутри фазы коллизии решаются приоритетом
/// расширения, при равном приоритете — именем файла (детерминизм).
pub fn resolve_lora_specs_for_base(
    dir: &Path,
    wanted: &[(String, f32)],
    base: Option<&str>,
) -> Result<Vec<LoraSpec>, String> {
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let target = match base {
        Some(base) if !base.trim().is_empty() => dir.join(base.trim()),
        _ => dir.to_path_buf(),
    };
    if !target.is_dir() {
        return Err(format!(
            "Папка LoRA не существует: {}. {}",
            target.display(),
            if base.is_some() {
                "Файлы LoRA хранятся по базам моделей — перенесите лоры в подпапку \
                 (например, <image_loras>\\qwen\\ для Qwen-стека или <image_loras>\\krea2\\ \
                 для Krea-2)."
            } else {
                "Проверьте настройку каталога LoRA."
            }
        ));
    }
    // Каталог папки: (stem_lowercase, file_name, приоритет расширения).
    let mut catalog: Vec<(String, String, usize)> = Vec::new();
    let entries = std::fs::read_dir(&target)
        .map_err(|e| format!("Папка LoRA не читается: {}: {}", target.display(), e))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if file_name.is_empty() || stem.is_empty() {
            continue;
        }
        let ext = path
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
            .unwrap_or_default();
        let prio = LORA_PREFERRED_EXT
            .iter()
            .position(|e| *e == ext)
            .unwrap_or(LORA_PREFERRED_EXT.len());
        catalog.push((stem, file_name, prio));
    }
    let mut per_item: Vec<(String, f32)> = wanted
        .iter()
        .map(|(name, mult)| (name.clone(), clamp_multiplier(*mult)))
        .collect();
    let mut out = Vec::with_capacity(per_item.len());
    for (name, mult) in per_item.drain(..) {
        let key = name.trim().to_lowercase();
        if key.is_empty() {
            return Err("LoRA: передано пустое имя".to_string());
        }
        // Фаза 1: точный стем. Фаза 2: нормализованный ключ (пунктуация неважна).
        let mut matches: Vec<&(String, String, usize)> =
            catalog.iter().filter(|(stem, _, _)| *stem == key).collect();
        if matches.is_empty() {
            let key_norm = normalized_stem(&key);
            matches = catalog
                .iter()
                .filter(|(stem, _, _)| normalized_stem(stem) == key_norm)
                .collect();
        }
        if matches.is_empty() {
            let mut known: Vec<String> = catalog.iter().map(|(s, _, _)| s.clone()).collect();
            known.sort();
            known.dedup();
            let list = if known.is_empty() {
                "папка пуста".to_string()
            } else {
                known.join(", ")
            };
            return Err(format!(
                "LoRA «{}» не найдена в {}. Доступны: {}",
                name.trim(),
                target.display(),
                list
            ));
        }
        matches.sort_by(|a, b| a.2.cmp(&b.2).then_with(|| a.1.cmp(&b.1)));
        out.push(LoraSpec {
            path: matches[0].1.clone(),
            multiplier: mult,
        });
    }
    Ok(out)
}

/// Резолв имён с указанием базы (см. `resolve_lora_specs_for_base`).
pub fn resolve_lora_names_for_base(
    dir: &Path,
    names: &[String],
    multiplier: f32,
    base: Option<&str>,
) -> Result<Vec<LoraSpec>, String> {
    let wanted: Vec<(String, f32)> = names.iter().map(|n| (n.clone(), multiplier)).collect();
    resolve_lora_specs_for_base(dir, &wanted, base)
}

/// F4 (29.10.26): гарантированная edit-лора ref-режима.
///
/// Krea2-edit: серверный ref-конвейер (`preset=krea2_edit`) патчит latents
/// референса — БЕЗ обученной под это лоры выход даёт «checkerboard»-артефакт
/// (ostris/ComfyUI-Krea2-Ostris-Edit issue #1: те же серые квадраты). Поэтому
/// контракт `ref_edit_mode: edit_lora` обязан дать движку лору ВСЕГДА:
/// юзер, заявивший ту же лору сам, получает стек без дубликата (матч по
/// пути файла, регистр игнорируется), иначе edit-лора идёт ПЕРВОЙ позицией
/// (выше стайл-лор) со своей силой из каталога.
///
/// `edit_path` — имя файла из резолвера (`resolve_lora_specs_for_base`),
/// `strength` — сила контракта (клампится 0..=2 на этой же границе).
pub fn inject_edit_lora(stack: Vec<LoraSpec>, edit_path: &str, strength: f32) -> Vec<LoraSpec> {
    let key = edit_path.trim().to_lowercase();
    if stack.iter().any(|s| s.path.trim().to_lowercase() == key) {
        return stack;
    }
    let mut out = Vec::with_capacity(stack.len() + 1);
    out.push(LoraSpec {
        path: edit_path.to_string(),
        multiplier: clamp_multiplier(strength),
    });
    out.extend(stack);
    out
}

/// Строка лога sd-server — отчёт применения LoRA-тензоров? Возвращает пару
/// `(applied, total)` из «X / N … tensors … applied». Контракты формата:
/// видео-бенч 26.10.26 («600/600 … tensors … applied») и цитата юзера
/// («X / N LoRA tensors have been applied»); фильтры `applied` + `tensor`
/// отсекают случайные пары чисел в других строках сервера.
pub fn scan_lora_applied_line(line: &str) -> Option<(usize, usize)> {
    let lower = line.to_lowercase();
    if !(lower.contains("applied") && lower.contains("tensor")) {
        return None;
    }
    let bytes = line.as_bytes();
    let slash = line.find('/')?;
    let applied = digits_before(bytes, slash)?;
    let total = digits_after(bytes, slash)?;
    Some((applied, total))
}

/// Целое число, заканчивающееся перед позицией `/` (пробелы допустимы).
fn digits_before(bytes: &[u8], slash: usize) -> Option<usize> {
    let mut end = slash;
    while end > 0 && bytes[end - 1] == b' ' {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && bytes[start - 1].is_ascii_digit() {
        start -= 1;
    }
    if start == end {
        return None;
    }
    std::str::from_utf8(&bytes[start..end])
        .ok()?
        .parse::<usize>()
        .ok()
}

/// Целое число, начинающееся после позиции `/` (пробелы допустимы).
fn digits_after(bytes: &[u8], slash: usize) -> Option<usize> {
    let mut start = slash + 1;
    while start < bytes.len() && bytes[start] == b' ' {
        start += 1;
    }
    let mut end = start;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if start == end {
        return None;
    }
    std::str::from_utf8(&bytes[start..end])
        .ok()?
        .parse::<usize>()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    /// Уникальная temp-подпапка теста (параллельные тесты не пересекаются
    /// за счёт наносекундной метки + tag).
    fn sandbox(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "ko_lora_tests_{}_{:?}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("temp-подпапка создана");
        d
    }

    #[test]
    fn spawn_args_when_dir_exists() {
        let d = sandbox("spawn_exists");
        assert_eq!(
            lora_spawn_args(&d),
            vec![
                "--lora-model-dir".to_string(),
                d.to_string_lossy().into_owned()
            ]
        );
    }

    #[test]
    fn spawn_args_skipped_when_dir_missing() {
        let d = std::env::temp_dir().join("ko_lora_tests_dir_oboznachenno_ne_suschestvuet_872");
        assert!(lora_spawn_args(&d).is_empty());
    }

    #[test]
    fn resolve_matches_stem_case_insensitive() {
        let d = sandbox("resolve_ci");
        std::fs::write(d.join("FemaledVaginus NSFW.safetensors"), b"x").expect("стаб-файл");
        let out =
            resolve_lora_names(&d, &["femaledvaginus nsfw".to_string()], 1.0).expect("резолв");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].path, "FemaledVaginus NSFW.safetensors");
        assert_eq!(out[0].multiplier, 1.0);
    }

    #[test]
    fn resolve_prefers_safetensors_on_stem_collision() {
        let d = sandbox("resolve_prio");
        std::fs::write(d.join("mix.safetensors"), b"x").expect("стаб-файл");
        std::fs::write(d.join("mix.gguf"), b"x").expect("стаб-файл");
        let out = resolve_lora_names(&d, &["MIX".to_string()], 0.8).expect("резолв");
        assert_eq!(out[0].path, "mix.safetensors");
        assert!((out[0].multiplier - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn resolve_unknown_name_lists_available() {
        let d = sandbox("resolve_unknown");
        std::fs::write(d.join("only_one.safetensors"), b"x").expect("стаб-файл");
        let err = resolve_lora_names(&d, &["ghost".to_string()], 1.0)
            .expect_err("неизвестное имя — ошибка");
        assert!(
            err.contains("ghost") && err.contains("only_one"),
            "сообщение содержит имя запроса и доступные: {}",
            err
        );
    }

    #[test]
    fn resolve_empty_list_is_noop() {
        let d = sandbox("resolve_empty");
        assert!(
            resolve_lora_names(&d, &[], 1.0)
                .expect("пустой список — без ошибки")
                .is_empty()
        );
    }

    #[test]
    fn resolve_missing_dir_is_honest_error() {
        let err = resolve_lora_names(
            &std::env::temp_dir().join("ko_lora_tests_dir_oboznachenno_ne_suschestvuet_873"),
            &["x".to_string()],
            1.0,
        )
        .expect_err("нет папки — ошибка");
        assert!(err.contains("не существует"), "сообщение: {}", err);
    }

    #[test]
    fn multiplier_clamped_to_honest_armor() {
        assert_eq!(clamp_multiplier(5.0), LORA_MULTIPLIER_MAX);
        assert_eq!(clamp_multiplier(-3.0), 0.0);
        assert_eq!(clamp_multiplier(f32::NAN), 0.0);
        assert_eq!(clamp_multiplier(f32::INFINITY), 0.0);
    }

    // ───────────── edit-лора ref-режима (F4 29.10.26) ─────────────

    /// Krea2-edit: юзерские стайл-лоры не отменяют требование контракта —
    /// edit-лора становится ПЕРВОЙ позицией стека со своей силой.
    #[test]
    fn inject_edit_lora_prepend_respectless_of_style_stack() {
        let stack = vec![
            LoraSpec { path: "hotShot.safetensors".to_string(), multiplier: 0.6 },
            LoraSpec { path: "realism.safetensors".to_string(), multiplier: 1.2 },
        ];
        let out = inject_edit_lora(stack, "krea2_identity_edit_v2_r128.safetensors", 1.0);
        assert_eq!(out.len(), 3, "стайл-лоры не вытесняются");
        assert_eq!(out[0].path, "krea2_identity_edit_v2_r128.safetensors", "edit-лора первая");
        assert_eq!(out[0].multiplier, 1.0);
        assert_eq!(out[1].path, "hotShot.safetensors");
        assert_eq!(out[2].path, "realism.safetensors");
    }

    /// Юзер уже заявил ту же edit-лору сам — дубликат не тиражируется
    /// (double-patch-ref-conditioning: слабое место патч-стека).
    #[test]
    fn inject_edit_lora_dedupe_when_user_already_declared_it() {
        let stack = vec![
            LoraSpec { path: "Krea2_Identity_Edit_v2_r128.safetensors".to_string(), multiplier: 0.9 },
        ];
        let out = inject_edit_lora(
            stack,
            "krea2_identity_edit_v2_r128.safetensors",
            1.0,
        );
        assert_eq!(out.len(), 1, "повтор edit-лоры запрещён");
        assert_eq!(out[0].multiplier, 0.9, "сила юзера сохраняется");
    }

    /// Сила контракта клампится той же вилкой 0..=2 (граница плагина).
    #[test]
    fn inject_edit_lora_clamps_strength() {
        let out = inject_edit_lora(Vec::new(), "edit.safetensors", 9.0);
        assert_eq!(out[0].multiplier, LORA_MULTIPLIER_MAX);
        let out = inject_edit_lora(Vec::new(), "edit.safetensors", -4.0);
        assert_eq!(out[0].multiplier, 0.0);
    }

    /// Некогерентный формат: NaN/∞/вне вилки на границе — честный кламп.
    #[test]
    fn resolve_multiplies_clamped() {
        let d = sandbox("resolve_clamp");
        std::fs::write(d.join("a.safetensors"), b"x").expect("стаб-файл");
        let out = resolve_lora_names(&d, &["a".to_string()], 9.0).expect("резолв");
        assert_eq!(out[0].multiplier, LORA_MULTIPLIER_MAX);
    }

    // ───────────── per-item силы (PRP 29.10.26 фаза 2, D2) ─────────────

    /// Разные силы на разных элементах — тело запроса честно per-item.
    #[test]
    fn resolve_specs_applies_per_item_multiplier() {
        let d = sandbox("specs_per_item");
        std::fs::write(d.join("slider.safetensors"), b"x").expect("стаб-файл");
        std::fs::write(d.join("realism.safetensors"), b"x").expect("стаб-файл");
        let wanted = vec![
            ("slider".to_string(), 1.8),
            ("realism".to_string(), 0.75),
        ];
        let out = resolve_lora_specs(&d, &wanted).expect("резолв");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].path, "slider.safetensors");
        assert!((out[0].multiplier - 1.8).abs() < f32::EPSILON, "сила элемента 0");
        assert_eq!(out[1].path, "realism.safetensors");
        assert!((out[1].multiplier - 0.75).abs() < f32::EPSILON, "сила элемента 1");
    }

    /// Легаси-обёртка (`resolve_lora_names`) — одна сила на все имена, то же ядро.
    #[test]
    fn resolve_lora_names_wrapper_matches_specs_core() {
        let d = sandbox("wrapper_matches_core");
        std::fs::write(d.join("a.safetensors"), b"x").expect("стаб-файл");
        std::fs::write(d.join("b.safetensors"), b"x").expect("стаб-файл");
        let via_wrapper =
            resolve_lora_names(&d, &["a".to_string(), "b".to_string()], 0.6).expect("резолв");
        let via_specs = resolve_lora_specs(
            &d,
            &[
                ("a".to_string(), 0.6),
                ("b".to_string(), 0.6),
            ],
        )
        .expect("резолв");
        assert_eq!(via_wrapper, via_specs, "обёртка и ядро дают одинаковый результат");
    }

    /// Кламп per-item: сила элемента за границей честно сжимается на границе плагина.
    #[test]
    fn resolve_specs_clamps_per_item_multiplier() {
        let d = sandbox("specs_clamp");
        std::fs::write(d.join("a.safetensors"), b"x").expect("стаб-файл");
        let wanted = vec![("a".to_string(), 5.0), ("a".to_string(), -1.0)];
        let out = resolve_lora_specs(&d, &wanted).expect("резолв");
        assert_eq!(out[0].multiplier, LORA_MULTIPLIER_MAX);
        assert_eq!(out[1].multiplier, 0.0);
    }

    // ───────────── папки по базам (F3, 29.10.26) ─────────────

    /// Резолв scoped на подпапку базы: свои стемы ищутся, чужие (корень и
    /// другая база) — нет, ошибка перечисляет стемы только этой базы.
    #[test]
    fn resolve_base_scopes_to_subfolder() {
        let d = sandbox("resolve_base_scopes");
        std::fs::write(d.join("flat_root.safetensors"), b"x").expect("стаб-файл");
        let qwen = d.join("qwen");
        std::fs::create_dir_all(&qwen).expect("подпапка qwen");
        std::fs::write(qwen.join("femaled.safetensors"), b"x").expect("стаб-файл");
        let krea2 = d.join("krea2");
        std::fs::create_dir_all(&krea2).expect("подпапка krea2");
        std::fs::write(krea2.join("krea_only.safetensors"), b"x").expect("стаб-файл");

        let own = resolve_lora_specs_for_base(
            &d,
            &[("femaled".to_string(), 1.0)],
            Some("qwen"),
        )
        .expect("свой стем");
        assert_eq!(own.len(), 1);
        assert_eq!(own[0].path, "femaled.safetensors");

        let cross = resolve_lora_specs_for_base(
            &d,
            &[("krea_only".to_string(), 1.0)],
            Some("qwen"),
        )
        .expect_err("кросс-база — честный отказ");
        assert!(cross.contains("krea_only"), "имя запроса: {}", cross);
        assert!(cross.contains("femaled"), "стемы своей базы: {}", cross);
        assert!(
            !cross.contains("stray") && !cross.contains("flat_root"),
            "чужие стемы не просачиваются: {}",
            cross
        );
        // Отказ адресован в каталог именно этой базы (подпапка существует,
        // ошибка ссылается на неё; инструкция «перенесите в подпапку» — из
        // кейса отсутствующей подпапки, см. resolve_base_missing_subfolder).
        assert!(cross.contains("qwen"), "каталог базы в сообщении: {}", cross);
    }

    /// Подпапка базы отсутствует — честный отказ с путём и инструкцией.
    #[test]
    fn resolve_base_missing_subfolder_is_honest_error() {
        let d = sandbox("resolve_base_missing");
        std::fs::write(d.join("flat.safetensors"), b"x").expect("стаб-файл");
        let err = resolve_lora_specs_for_base(
            &d,
            &[("flat".to_string(), 1.0)],
            Some("ghost_base"),
        )
        .expect_err("подпапки нет");
        assert!(err.contains("не существует"), "сообщение: {}", err);
        assert!(err.contains("ghost_base"), "имя базы в сообщении: {}", err);
        assert!(err.contains("подпапку"), "инструкция о структуре: {}", err);
    }

    /// base = None — легаси-плоский путь (старые вызовы и тесты).
    #[test]
    fn resolve_base_none_keeps_legacy_flat() {
        let d = sandbox("resolve_base_none");
        std::fs::write(d.join("flat.safetensors"), b"x").expect("стаб-файл");
        let out = resolve_lora_specs_for_base(&d, &[("flat".to_string(), 1.0)], None)
            .expect("легаси-плоский");
        assert_eq!(out[0].path, "flat.safetensors");
    }

    /// Спавн по базе: путь `dir/<base>`; подпапка создаётся автоматически,
    /// если корень существует (пустая база — легитимное состояние).
    /// Корня нет — пустой вектор (как легаси),args не выдумываются.
    #[test]
    fn spawn_args_for_base_targets_and_creates_subfolder() {
        let ghost = std::env::temp_dir().join("ko_lora_tests_spawn_base_ghost_dir_821");
        assert!(lora_spawn_args_for_base(&ghost, Some("qwen")).is_empty());

        let d = sandbox("spawn_base_ok");
        let expected_sub = d.join("qwen");
        assert_eq!(
            lora_spawn_args_for_base(&d, Some("qwen")),
            vec![
                "--lora-model-dir".to_string(),
                expected_sub.to_string_lossy().into_owned()
            ],
            "подпапка базы создана автоматически"
        );
        assert!(expected_sub.is_dir(), "подпапка qwen существует");
        // Легаси: None → корень.
        assert_eq!(lora_spawn_args_for_base(&d, None), lora_spawn_args(&d));
    }

    /// Фаза 2 нормализации: LLM переписала стем «…V2.1» как «…v2_1»
    /// (точка → подчёркивание, инцидент 30.10.26) — файл обязан найтись.
    /// Пробелы и дефисы нормализуются так же.
    #[test]
    fn resolve_matches_normalized_punctuation_variants() {
        let d = sandbox("resolve_normalized");
        std::fs::write(d.join("Minimax_H3_Unlocked_V2.1.safetensors"), b"x").expect("стаб-файл");
        for wanted in [
            "Minimax_H3_Unlocked_V2_1",
            "minimax_h3_unlocked_v2_1",
            "minimax-h3-unlocked-v2-1",
            "minimax h3 unlocked v2 1",
        ] {
            let out = resolve_lora_names(&d, &[wanted.to_string()], 0.7).expect("нормализ-матч");
            assert_eq!(out[0].path, "Minimax_H3_Unlocked_V2.1.safetensors", "запрос: {wanted}");
            assert!((out[0].multiplier - 0.7).abs() < f32::EPSILON);
        }
    }

    /// Точный стем сильнее нормализованного: «alpha_v2» бьёт «alpha.v2»-запрос
    /// только если точного нет; при нормализованной коллизии двух файлов
    /// выбор детерминирован (приоритет расширения, затем имя файла).
    #[test]
    fn resolve_exact_stem_wins_and_collision_is_deterministic() {
        let d = sandbox("resolve_exact_vs_norm");
        std::fs::write(d.join("alpha_v2.safetensors"), b"x").expect("стаб-файл");
        std::fs::write(d.join("alpha.v2.safetensors"), b"x").expect("стаб-файл");
        // Точный запрос: только alpha_v2.safetensors.
        let out = resolve_lora_names(&d, &["alpha_v2".to_string()], 1.0).expect("точный");
        assert_eq!(out[0].path, "alpha_v2.safetensors");
        // Нормализованная коллизия (запрос «alpha v2» не совпадает точно ни с
        // одним стемом): оба файла дают "alphav2" — побеждает меньшее имя
        // файла (детерминизм независимо от порядка read_dir).
        let out = resolve_lora_names(&d, &["alpha v2".to_string()], 1.0).expect("нормализ-коллизия");
        assert_eq!(out[0].path, "alpha.v2.safetensors", "a.alpha... < alpha_v2... по имени");
    }

    /// Совсем чужое имя — прежний честный отказ со списком стемов
    /// (нормализация не должна превращать опечатку в «какую-нибудь» лору).
    #[test]
    fn resolve_unknown_name_still_errors_after_normalization() {
        let d = sandbox("resolve_norm_unknown");
        std::fs::write(d.join("alpha.safetensors"), b"x").expect("стаб-файл");
        let err = resolve_lora_names(&d, &["alpha_v9".to_string()], 1.0)
            .expect_err("нормализация не матчит не-ту лору");
        assert!(err.contains("alpha_v9") && err.contains("alpha"), "ошибка: {}", err);
    }

    #[test]
    fn scan_matches_applied_tensor_lines() {
        assert_eq!(
            scan_lora_applied_line("lora: 2 / 2 tensors have been applied"),
            Some((2, 2))
        );
        assert_eq!(
            scan_lora_applied_line("lora: 42 / 88 LoRA tensors have been applied"),
            Some((42, 88))
        );
        // Видео-бенч 26.10.26: «600/600 tensors applied» (без пробелов).
        assert_eq!(
            scan_lora_applied_line("600/600 tensors applied"),
            Some((600, 600))
        );
    }

    #[test]
    fn scan_ignores_lines_without_tensor_applied() {
        assert_eq!(scan_lora_applied_line("gen time 10 / 20 seconds"), None);
        assert_eq!(scan_lora_applied_line("found 3 / 5 files applied"), None);
        assert_eq!(scan_lora_applied_line("lora 1 applied"), None);
        assert_eq!(scan_lora_applied_line(""), None);
    }
}
