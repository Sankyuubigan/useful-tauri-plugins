# BUNDLES — сборки моделей для image-engine (Qwen-Image-2.1 и родня)

Документ — «качественная» связка: где берём веса, чем сборки отличаются, что наш
стек умеет, как проверять новые наборы. Точные факты о файлах/размерах/пресетах
живут ТОЛЬКО в `image_models_catalog.json` (SSOT); VRAM-пороги считает `preflight`
(тоже SSOT). Здесь цифры приводятся как справка — при расхождении правом является
каталог/замер, а не эта таблица.

## 0. Принципы

- **GGUF-first:** наш стек — официальный `sd-server.exe` (leejet/stable-diffusion.cpp).
  Собираем бандлы из GGUF-весов: diffusion (кванты) + VAE (.safetensors bf16)
  + text encoder (LLM GGUF) + mmproj (для edit).
- **«Estimates, not tested minimums»** (правило unsloth): все RAM/VRAM-числа —
  оценки; точное поведение подтверждает только замер (`compute buffer` из лога
  сервера + nvml). Каталог не обещает «4/8/9 ГБ» без пометки.
- **Общие файлы не перекачиваются:** одна общая папка `image_bundle_dir` для ВСЕХ
  бандлов; имена файлов общих ролей (TE/VAE/mmproj) совпадают → downloader
  пропускает существующие («Уже скачан, пропуск»). Новый бандл качает только свой
  diffusion-файл.
- **Edit требует mmproj:** генерация t2i работает и без mmproj; редактирование
  (референсы) — нет (`--llm_vision`). Инструмент обязан отказывать edit-запросам
  на бандле без mmproj с понятной ошибкой.

## 1. LPIPS 101

LPIPS — метрика похожести пары картинок: измеряет визуальное различие
(меньше = схоже). В таблицах unsloth LPIPS считается относительно
базовой несжатой bf16/Turbo-модели на фиксированных промптах при 1024×1024.
Для сравнения квантов между собой — общепринятая метрика; для оценки
«эстетичности» — непригодна. Программного измерения качества без reference нет:
проверка «лучше/хуже» в конце — ручной A/B юзера.

## 2. Матрица сборок Qwen-Image-2.1

### Официальный Turbo-чекпоинт (Qwen/Qwen-Image-2.1-Turbo, релиз 09.10.2026)

Turbo = fine-tune базы: та же архитектура 7B DiT / те же TE и VAE. Для Turbo:
8 шагов, CFG 1.0, euler. Кванты unsloth (`unsloth/Qwen-Image-2.1-Turbo-GGUF`),
LPIPS против bf16-Turbo (mean / held-out, 1024², 8 шагов, CFG 1):

| Файл GGUF | Размер (HF API) | LPIPS mean | LPIPS held-out | Комментарий |
|---|---|---|---|---|
| qwen-image-2.1-turbo-Q2_K.gguf | 2.30 ГБ | — | — | без публик. LPIPS |
| qwen-image-2.1-turbo-Q3_K_XL.gguf | 3.37 ГБ | — | — | |
| qwen-image-2.1-turbo-Q4_K_S.gguf | 3.64 ГБ | 0.201 | 0.186 | «спасательный» |
| qwen-image-2.1-turbo-Q4_K_M.gguf | 3.91 ГБ | 0.153 | 0.141 | «recommended» у unsloth |
| qwen-image-2.1-turbo-Q5_K_S.gguf | 4.19 ГБ | 0.103 | **0.083** | лучший LPIPS/байт для карт ~8 ГБ |
| qwen-image-2.1-turbo-Q5_K_M.gguf | 5.02 ГБ | 0.074 | 0.087 | |
| qwen-image-2.1-turbo-Q6_K.gguf | 5.84 ГБ | 0.055 | 0.076 | |
| qwen-image-2.1-turbo-Q6_K_XL.gguf | 6.26 ГБ | 0.050 | 0.069 | |
| qwen-image-2.1-turbo-Q8_0.gguf | 7.12 ГБ | 0.036 | 0.030 | |
| qwen-image-2.1-turbo-F16.gguf | 13.25 ГБ | 0.012 | 0.016 | reference-качество |

(Гуман. ГБ = байты API / 1e9. Точные байты — в каталоге `size_bytes`.)

### База (40 шагов) — `leejet/Qwen-Image-2.1-GGUF`

Q2_K … Q8_0; дефолт каталога ДО 19.10.2026 — `qwen_image_2.1-Q6_K.gguf`
(5 996 851 232 Б, 28 шагов, CFG 6.0, euler). GGUF-сеть leejet —
«community»-конверты sd.cpp-ориентированные; unsloth — крупные официальные.
**19.10.2026: base-бандл удалён из каталога по вердикту юзера после ручного
A/B и замера 78.7 с/ген против 12–14 с у turbo** («ничем не лучше, по
скорости ужасен»). PNG-артефакты base остаются в `test/bench/qwen-image-2.1/`
как референс pHash. Файлы base-диффузии на диске можно вручную освободить.

### safetensors-стек (не наш, для справки)

- `Comfy-Org/Qwen-Image-2.1`: fp8 / int8_convrot (7.26 ГБ). LPIPS int8_convrot
  0.064 против fp8 0.112 (unsloth) — int8 точнее fp8. Наш стек — GGUF: эти файлы
  НЕ используются напрямую (нужна конвертация через city96/ComfyUI-GGUF).

### Viggle Turbo — Turbo-LoRA-семейство (few-step дистилляция НА БАЗУ)

`Viggle/Qwen-Image-2.1-viggle-turbo` (v0.3, 29.09.2026; base_model =
Qwen/Qwen-Image-2.1, adapter):
- Режим LoRA: r128 = 679 604 800 Б; r256 = 1 359 147 904 Б.
- **Merged GGUF (LoRA вшита):** v0.3-6step-Q4_K_M = 4 335 931 552 Б;
  Q5_K_M = 5 141 237 920 Б; Q6_K = 5 996 875 936 Б (ровно класс нашей текущей
  диффузии!); Q8_0 = 7 687 180 448 Б.
- Стартовый шаблон: **6 шагов, CFG 1**; заявка Viggle «~5× быстрее 40-шага,
  competitive».
- **Known limitations (цитата README):** «мелкий текст и сложные правки —
  слабое место».
- Старый файл `...-4step-lora-r64.safetensors` (339 832 808 Б, v0.x) —
  предшественник; не рекомендуем для новых бандлов.

Другие Turbo-LoRA (civitai/HF): aimann «Turbo 8 Step» (~608 МБ), lenML r128
(~640 МБ — экстракт официального Turbo, применяется ПОВЕРХ чекпоинта Turbo),
Pruna 5/8-шаг adapters (по ~320 МБ), _Envy_ «Fix v2.0» (~52/106 МБ — LoRA
детализации, не ускоритель).

### Turbo-чекпоинт vs Turbo-LoRA — честное резюме

**Прямых публичных замеров НЕТ** (orcarouter дословно: «will load, but nobody
has published a measurement — including the vendor»). Что известно достоверно:
1) Turbo-чекпоинт — официальный дистиллят со **опубликованной** таблицей LPIPS
по квантам и Day-0 поддержкой sd.cpp; 2) Viggle — комьюнити/vend-файл с
опубликованным шедулем и merged-GGUF, но БЕЗ LPIPS. Вердиктов «лучше/хуже» не
выносим до наших замеров (bench-харнесс, см. §7).

### Стек Krea-2-Turbo — в каталоге с 21.10.2026

Отдельная архитектура 12.9B (НЕ Qwen-Image-2.1): diffusion `realrebelai/KREA-2_GGUFs/TURBO` +
**Wan2.1 VAE** (Comfy-Org repackaged) + TE **Qwen3-VL-4B** + mmproj
Qwen3-VL-4B-F16 (edit). Пресет — официальный Turbo-чекпоинт: 8 шагов, CFG 1.0,
mu=1.15, сигма-сетка из `sampling.py` README `krea-ai/krea-2`
(значения сверены с `krea_compare.rs`, формула в коде). Ref-режим редактирования
подтверждён исходником sd.cpp (`diffusion_engine.cpp:3219` — auto-preset
`krea2_ostris_edit`); живой смок-эдит — обязательный шаг валидации.

Специфика против Qwen-стека:

1. **model-args не посылается** (`krea2.hpp` не имеет парсера model-args —
   qwen-ключ `qwen_image_2_1_prefix_cache_type` молча игнорировался бы; теперь
   `--model-args` задаётся каталогом per-bundle, Krea2-записи поля не имеют).
2. **Теги `<image1>/<image2>` не применяются** — конвенция Qwen-стека:
   референс описывается словами («первая картинка — вторая картинка»).
3. **8 ГБ — только Q3_K_M** (weights 5.5 ГБ diffusion; статический минимум
   ~8.5 ГБ — впритык; применимость решит замер по методике §4 и запишет
   фактический `compute_buffers_mb` в каталог; пороги не подгоняются).
   Q4_K_M (7.2 ГБ diffusion) на 8-гб карту не влезает физически — квант
   для 12+ ГБ VRAM (замер на 16 ГБ: пик 13.5–14.9 ГБ при img2img 2048²).

Каталог: `krea2-turbo-q4km`, `krea2-turbo-q3km` (оба — полный функционал:
генерация + редактирование + апскейл img2img; по правило mmproj у каждого).

## 3. Сигма-шедули (обязательно к точности)

Turbo-семейство живёт по **фиксированным сигма-профилям**. Контракт sd.cpp
(`src/pipeline/request.cpp`, `SamplePlan::resolve`): массив `custom_sigmas`
обязан кончаться терминальным `0.0`, а шаги = **len(sigmas) − 1** (движок сам
правит `total_steps` и пишет WARN, если рассинхрон).

- Официальный Turbo (`--sigmas qwen-image-2.1-turbo`, пресет встроен в sd.cpp):
  `1.0, 0.978453, 0.95418, 0.926626, 0.89508, 0.845148, 0.704534, 0.414568, 0.0`
  → 9 значений = **8 шагов** (+ терминальный 0.0).
- Viggle v0.3 (6 шагов): `1.0, 0.9375, 0.875, 0.75, 0.5, 0.25, 0.0` — 7 значений;
  `scheduler.shift_terminal: null` — базовый `0.02` ЛОМАЕТ последний шаг.
- Few-step правки Viggle: «шаги убираются НЕ с low-noise конца» (README).

В API `POST /sdcpp/v1/img_gen` шлём `sample_params.custom_sigmas` (массив
с терминальным 0.0) и `sample_steps = len(custom_sigmas) − 1`. При заданных
custom_sigmas поле `scheduler` НЕ влияет на шаги (игнорируется шедулером).

## 4. Что наш стек уже поддерживает

- `sd-server.exe`: native async `/sdcpp/v1/…`, совместимые `/v1`,`/sdapi/v1`,
  Web UI на `:1234/`.
- Спавн (image_engine.rs): `--diffusion-model --vae --llm [--llm_vision mmproj]
  --listen-ip --listen-port --fa --diffusion-fa --auto-fit on --max-vram -1 [--model-args <ключ из каталога>]`.
  Флаг `--model-args` задаётся полем `model_args` записи каталога (per-bundle):
  qwen-бандлы — `qwen_image_2_1_prefix_cache_type=q8_0`, Krea2 — без флага.
- `--offload-to-cpu` в текущей версии sd.cpp сохраняет скорость (см.
  performance.md); авто-fit сам решает сегментацию — в интеграции флаги
  размещения не передачаны (см. комментарий в `image_engine.rs` и PRP).
- Compute-буфер сервер печатает в лог: строка вида
  `compute buffer size: 650.00 MB(VRAM) on CUDA0` — используется preflight как
  фактическое число (fallback — константа).
- Prefix KV-cache квантован q8_0 (≈1.06 ГБ на 4096 токенов против ~4 ГБ f32).
- Flash attention (`--fa`, `--diffusion-fa`) экономит сотни МБ буферов.

## 5. Общие файлы и выбор бандла

- Все бандлы живут в одной папке `image_bundle_dir`; каждый entry каталога
  перечисляет свои файлы (роль ИМЯ файла — уникально глобально).
- Панель настроек — медиа-блок `<image-media-panel>` (вкладки «Картинки»/«Видео»,
  каждая вкладка — `<image-bundle-tab>`): показывает список бандлов каталога с
  метками «скачан/не скачан» и переключает **активный** бандл
  (`set_selected_image_bundle` / `set_selected_video_bundle`); выбор сохраняется
  в конфиг плагина.
- Выбор выравнивается правилом 19.10.2026: у КАЖДОГО набора полный функционал
  — генерация И редактирование (в том числе mmproj). Дефолт каталога —
  Turbo unsloth Q5_K_S; Viggle Q4_K_M — эконом-вариант. Viggle Q6_K удалён
  вердиктом юзера 20.10.2026 (edit 0/17, hires 13/17 — нестабилен,
  см. «Что отложено / снято»).
- Координация VRAM с llama-движком: на 8-ГБ картах media-пайплайн перед
  генерацией выгружает llama (см. PRP координации); по факту для полного
  turbo-бандла (~13 ГБ пик по замеру 19.10.2026) выгрузка llama нужна даже
  на 16-ГБ картах.

## 6. VRAM-честность

Формула preflight — единственный источник требований; `vram_*_gb` в каталоге —
«эстимейт»-столбик. Оценка Turbo Q5_K_S на 1024²: ~4.3 ГБ diffusion + буферы
(≈0.8–1.4 ГБ по факту лога) + VAE ≈0.6 ГБ → «влезает на карте с ~6.3 ГБ
свободными» — подтверждать замером на конкретной карте. Для 8-ГБ карт с живым
LLM-движком media-пайплайн сначала выгружает llama (см. PRP координации).

## 7. Как проверять новые сборки (протокол)

1. HF API tree (`/api/models/<repo>/tree/main`) — точные имена/байты.
2. LPIPS из README/статьи автора (если опубликован) — записать сюда.
3. Наш замер: compute buffer из лога + nvml-пик VRAM + время (bench-харнесс
   8 промптов × 2 seeds, методика unsloth).
4. Ручной A/B юзера (`ab_votes.json`) — финальное слово.
5. После решения: entry в `image_models_catalog.json` + строка в этой таблице.

## 8. Бенч-харнесс плагина (Шаг 6/7 PRP — реализован 19.10.2026)

Код: `src/engine/bench.rs` плагина. Запуск ТОЛЬКО вручную:

```
cmd /c "D:\Projects\king_orch_3\test\test_plugin_tests.bat"   # соберёт и прогонит юнит-тесты
cargo test -p tauri-plugin-image-engine -- --ignored           # сам бенч (по явной просьбе)
```

Это спавнит sd-server и генерирует **8 промптов × 2 сидов (42, 1337) на КАЖДОМ
бандле** (+ warm-up первый прогон, не входит в средние). Папки переопределяются
env-переменными, иначе дефолты: `IMG_BENCH_ENGINE_DIR` (движок),
`IMG_BENCH_BUNDLE_DIR` (общая папка весов — из `app_config.json`
`image_bundle_dir`), `IMG_BENCH_OUT` (default `king_orch_3/test/bench`).

Артефакты на бандл: `test/bench/<bundle>/report.json` + PNG
(`P{idx}_S{seed}.png`, `warmup_S<seed>.png`). В отчёте: модель, resolution,
steps/cfg, load_time_s, free/total/peak/free-after VRAM, число провалов,
заметки протокола. Интеграционный тест печатает сравнительную таблицу и
(Шаг 7.2) pHash каждого Turbo-бандла против base Q6_K.

**Честность метрик:**
- пик VRAM — занято ВСЕЙ картой (NVML раз в ~2 сек; чужие процессы входят);
  пик коротких всплесков может не пойматься;
- RAM-пик НЕ замеряется (оценка по размеру весов бандла);
- pHash — индикация «картинки по-разному меняются», НЕ метрика качества;
- LPIPS программно — только вручную (`test/lpips_bench.py`,
  `pip install lpips torch pillow`, пары по имени файла, mean/max);
  доверенный источник published LPIPS unsloth (таблица §2);
- финальное слово — ручной A/B юзера в `test/bench/ab_votes.json`
  (candidate/base/prompt/seed → verdict), см. Шаг 7.4 PRP.

## 9. Видео-бандлы MiniMax H3 (PRP 21.10.26, Фаза 1)

День-1 поддержка sd.cpp (2026/08/04, `docs/minimax_h3.md`). SSOT фактов —
`video_models_catalog.json` рядом с `image_models_catalog.json`; здесь —

### Стек и спавн

Четыре компонента на ярус спавна: `--diffusion-model` (pruned GGUF) + `--vae`
(video VAE fp16 `minimax_h3_video_vae_fp16.safetensors`) + `--audio-vae`
(fp32, 605 МБ) + `--llm` `qwen3vl_32b_minimax_h3-Q2_K_M.gguf` (12.2 ГБ).
Спавн-инварианты (`build_spawn_args`): **ровно ОДНА политика размещения весов**
(docs/backend.md, бенч 27.10.26) — `--offload-to-cpu --rng cpu` (все params в
RAM) ИЛИ `--auto-fit on --max-vram -1 --rng cpu` (params diffusion/te в RAM,
VAE на disk; замер: RSS-пик 24.65 ГБ, VRAM-пик 15.26 ГБ, время 1.17× offload).
Смесь политик и «ни одной» — дефект каталога, честный отказ; `--fa`/
`--diffusion-fa` видео не копирует. q4km использует auto-fit (`ram_min_gb`
28.5 — из замера), q3k-бандлы — offload.

### Режимы и ограничения vid_gen

- T2VA / I2VA (`--init-img`) / **FL2VA** (init+end, последний кадр) /
  **Ref2VA** (референсы). `ref_images` нельзя смешивать с
  `init_image`/`end_image`; `control_frames` у H3 не работает — всегда пусто.
- **Uni-бандл (30.10.26)**: Q3_K-набор несёт ОБА diffusion-файла (fl2va + ref2va)
  с картой «режим → файл» (`BundleFile.modes` + `is_default`). Файл выбирается
  при спавне по режиму запроса (`resolve_diffusion_file`): t2va-пересечение
  обоих файлов → детерминированно is_default (fl2va-файл); i2va/fl2va →
  fl2va-файл; ref2va → ref2va-файл. Ровно один `--diffusion-model` на спавн —
  VRAM/RAM не меняются, цена только на диске (+8.12 ГиБ). Валидатор делает
  неоднозначный выбор структурно невозможным: union modes файлов == entry.modes,
  ровно один is_default, пустой modes у файла при дублированной роли — отказ.
- **Лимит референсов — структурный**: хост-тул принимает 1–3 (schema maxItems
  + счётный отказ до resolve); по субтитрам роликов >6 референсов ломает
  следование. character sheet 4-view сильно помогает; видео-референсы не
  советуют (медленно, плохо). Аудио-референс тембра голоса работает.
- Дефолт вывода: 864×480, **24 fps**, CFG 1.0, euler, ≤15 сек.
- **Кадры: сетка 17k+5; сервер сам нормализует `video_frames` к `4n+1`.**
  Клиент кадры НЕ нормализует — отдаёт seconds×fps как есть (инвариант
  `seconds_to_frames` — только арифметика, без сетки).
- **`hires` в vid_gen НЕ поддерживается** (он только у img_gen): «draft →
  final» = ДВА вызова vid_gen, а не один.
- **Draft/final — один стек**: шаговый пресет (draft 4 при Turbo-LoRA DMAD,
  fallback `steps_draft_nolora` 6; final 20), НЕ две разные модели.
- **Turbo-LoRA DMAD — подтверждена бенчем 26.10.26** (гипотеза снята): LoRA
  грузится и применяется в vid_gen runtime (`lora.hpp:1046 — (600 / 600) LoRA
  tensors have been applied`, без Python/конвертаций). Скорость на 5-сек клипе
  864×480 (seed 42): dmad4 медиана **130.8 с** против ctrl20f (final, euler 20
  шагов) **505.6 с** → **ускорение 3.87×** (гейт ≥2.5× PASS); dmad8 255.8 с.
  Операционная точка DMAD — **4 шага, lcm/simple/shift 12** (euler — НЕ
  операционная точка дистиллята, README DMAD); euler/discrete остаётся путём
  final и draft-без-LoRA. Турбо-параметры — поля `turbo_sampler` /
  `turbo_scheduler` / `turbo_flow_shift` пресета каталога (SSOT), в коде
  payload их нет; draft берёт их из сэмплинг-плана при наличии файла роли
  `lora_draft` в папке бандла. Нюанс: 24 тензора token_refiner не видны в
  счётчике применения (600 из 624) — влияние на стиль закрывает A/B юзера.
  Замер сделан на q4km; для Q3_K замеров нет (файл LoRA общий).
- **`flow_shift: 0.0` у H3 = CUDA crash** (изолировано матрицей 2×2 в
  `test/bench/video_h3_turbo/diag_report.json`): shift 0 вырождает сигма-сетку
  → CUDA illegal memory access на первом шаге сэмплинга, НЕЗАВИСИМО от
  сэмплера (падали euler и lcm). Payload обязан ОПУСКАТЬ ключ при отсутствии
  явного shift (модельный дефолт) — регресс-тест `payload_omits_flow_shift_when_none`;
  валидатор каталога запрещает неположительный `turbo_flow_shift` всегда.
- Русская речь — нативно: озвучка приходит из текстового промпта (модель
  T2VAaudio-архитектура), отдельного audio-флага не нужно (see test `H3
  прямо говорит` docs/minimax_h3.md, T2VA audio-guide).

### Выбор бандла

Каталог `video_models_catalog.json` (обновлён 30.10.2026 — два q3k-бандла
слиты в uni; lora_draft добавлен 26.10.2026):

| Бандл | Diffusion | Размер | VRAM (fast/min) | RAM | Кому |
|---|---|---|---|---|---|
| `minimax_h3_fl2va_pruned-q4km` (дефолт) | fl2va Q4_K_M | 31.7 ГБ | 28 / 12 ГБ | ≥30 ГБ (лестница) | карты ≥12 ГБ |
| `minimax_h3_uni-q3k` | fl2va Q3_K + ref2va Q3_K (по режиму) | 37.8 ГБ | 27 / 8.5 ГБ | ≥28 ГБ | карты ~8 ГБ, все режимы |

В комплекте каждого бандла — роль `lora_draft` (`dmad_minimax_h3_4step_full_critic.safetensors`,
1.29 ГБ, `ZhengmingYu/DMAD`); имя одинаково, поэтому в общей папке
она скачивается/adopt-ится один раз. Дефолт draft = 4 шага (турбо-точка DMAD).
Q3_K квант мягче по качеству, чем Q4_K — предупреждение честное (частично
снимется замером draft/final). Замер бенча 26.10.26 сделан на q4km; для
Q3_K замеров нет (файл LoRA общий). Общие файлы
(llm/VAE/audio-VAE/lora_draft) — одинаковые имена у обоих бандлов: в общей
папке скачивания downloader пропускает существующее («Уже скачан»), новый бандл
качает только свои diffusion-файлы (у uni их два: второй догружается тем же
механизмом).
Равнозначность fl2va- и ref2va-файла для чистого t2va бенчем НЕ подтверждена
(доказательств нет); t2va детерминированно идёт на fl2va-файле (is_default).

### Preflight и память

`preflight_video` — те же два гейта, что у image (§6): NVML недоступен →
VRAM-проверка честно пропускается (`vram_txt: "N/A (NVML недоступен)"`), RAM
и диск проверяются всегда. **Диск считается ТОЛЬКО по отсутствующим файлам**
(предзаданная папка не пугает двойным размером). VRAM-числа каталога —
эстимейты: подтвердит bench фазы 1; пороги не подгоняются.

## Источники

- unsloth/Qwen-Image-2.1-GGUF и unsloth/Qwen-Image-2.1-Turbo-GGUF (таблицы LPIPS,
  дата 10/2026)
- leejet/Qwen-Image-2.1-GGUF (community GGUF, sd.cpp-целевые)
- Viggle/Qwen-Image-2.1-viggle-turbo (README v0.3, merge-дистилляция)
- PrunaAI/Pruna-Qwen-Image-2.1 (few-step adapters)
- Comfy-Org/Qwen-Image-2.1 (safetensors fp8/int8_convrot)
- city96/ComfyUI-GGUF (конвертация safetensors→GGUF)
- sd.cpp: performance.md, docs/qwen_image_2.1.md, examples/server/api.md,
  examples/server/README.md
- civitai api: `https://civitai.com/api/v1/models?types=LORA&query=...`
- unsloth.ai/docs/models/qwen-image-2.1 (гайд + RAG-чат `?ask=...`)
- krea-ai/krea-2 (README + sampling.py: Turbo-пресет 8 шагов, CFG 1.0, mu=1.15,
  сигма-формула)
- realrebelai/KREA-2_GGUFs (HF API tree TURBO, размеры 20.10.2026)
- Qwen/Qwen3-VL-4B-Instruct-GGUF (HF API tree, TE + mmproj)
- Comfy-Org/Wan_2.1_ComfyUI_repackaged (Wan2.1 VAE)
- sd.cpp: docs/krea2.md (стек + ref-режим krea2_ostris_edit)

## Что отложено / снято (не в текущем каталоге)

- **Viggle turbo Q6_K:** удалён из каталога вердиктом юзера 20.10.2026
  (bench 19.10.2026 с `--vae-tiling`: edit 0/17, hires 13/17 — нестабилен;
  unsloth q5_ks и q4km — 17/17). Файл с диска и entry каталога снесены.
- **hires-fix:** нативный `hires`-объект `/sdcpp/v1/img_gen` реализован и
  замерен bench'ом (report.hires.json 19.10.2026, ~100 с до 2048²), НО в
  продукте ОТМЕНЁН вердиктом юзера 19.10.2026: «разницы по качеству не
  вижу, а медленнее». Продуктовый путь апскейла — img2img ×2
  (`init_image` + `strength`, ~50 с, report.upscale_ab.json); hires
  остаётся только bench-инфраструктурой (test_bench.bat).
- **MiniMax H3 (видео):** ветка по теме в отдельном документе
  `docs/SOURCES_VIDEO.md` (PRP 21.10.26); бандлы — §9. Turbo-LoRA DMAD
  внедрена 26.10.2026 (бенч + роль `lora_draft` в каталоге, §9); `lora_swap`
  остаётся отложенной до bench.
- **RGBA-прозрачность** — prompt-конвенция
  `This is an RGBA image with transparency. ...` (нативная фича модели).
- **Krea 2 tiled-diffusion («хитрый апскейл» до ~40 МП):** в sd.cpp tiled
  diffusion НЕ нативен; нужна собственная реализация тайлов в плагине.
  Референс метода: `RandyHaylor/krea2-tiled-diffusion-comfyui` («модель видит
  только один тайл», «тайлы сливаются каждый шаг», рецепты 2×2/3×3, overlap
  256px) и обзор myaiforce (3 прохода: iterative upscale + super-res + tiled
  refinement). Krea 2 — отдельный стек весов: diffusion Krea-2-Turbo/Raw
  (`realrebelai/KREA-2_GGUFs`, 12.9B) + **Wan2.1 VAE** + TE **Qwen3-VL-4B**
  (документ sd.cpp `docs/krea2.md`). **Обновление 21.10.2026:** базовые
  Turbo-бандлы (Q4_K_M, Q3_K_M) уже в продуктовом каталоге (см. §2);
  заявлений «лучшие апскейлы» публичные замеры не подтверждают — сравнение
  остаётся за нашим bench (img2img-upscale vs tiled), tiled по-прежнему deferred.
