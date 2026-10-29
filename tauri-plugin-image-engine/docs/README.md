# tauri-plugin-image-engine

Переиспользуемый движок изображений (SSOT): установка/обновление `sd-server.exe`
(stable-diffusion.cpp), бандл Qwen Image 2.1, VRAM-preflight, генерация/редактирование
по HTTP. Приложение НЕ линкует stable-diffusion.cpp нативно.

## Архитектура

```
King Orch (Rust/Tauri exe)          HTTP (localhost)              sd-server.exe
  ImageEngine                        POST /sdcpp/v1/img_gen       (релиз sd.cpp)
   - spawn subprocess                 GET  /sdcpp/v1/jobs/{id}      --diffusion-model
   - random port (19400..20900)  ──▶  poll до completed      ──▶    --vae --llm [--llm_vision]
   - Drop → kill дерева           ◀── result.images[0].b64_json     --listen-ip/port
```

## Бандл (image_models_catalog.json)

Два turbo-бандла: unsloth Q5_K_S (дефолт) и Viggle Q4_K_M; состав
diffusion turbo + TE Q4_K_M + mmproj F16 + VAE bf16 (~11.2 ГБ). Viggle
Q6_K удалён вердиктом юзера 20.10.2026 (нестабилен, edit 0/17). Пресеты
генерации (`cfg_scale 1.0`, `euler`, `1024x1024`, `steps 6`,
`custom_sigmas`, `seed -1`) — только в каталоге. Пороги: `vram_fast_gb` /
`vram_min_gb` / `ram_min_gb` — в каждом entry.

## Флаги запуска `sd-server` (критично для скорости)

```
--fa --diffusion-fa --auto-fit on --max-vram -1
--model-args qwen_image_2_1_prefix_cache_type=q8_0
[--lora-model-dir <KingOrchData>/image_loras]   // только если папка существует
```

- **НЕЛЬЗЯ передавать `--offload-to-cpu`.** Он подставляет `--params-backend '*=cpu'`,
  а любой явный `--params-backend` **отключает auto-fit** (docs/backend.md). Без
  auto-fit веса diffusion + TE + mmproj + VAE живут в RAM, GPU-копии вытесняются
  на каждом шаге, деноизинг идёт сегментированно со стримингом по PCIe.
  Замер: 28 шагов 1024x1024 → 272 сек. С auto-fit diffusion остаётся резидентным.
- `--max-vram -1` — зарезервировать ~1 ГБ от стартового свободного VRAM, чтобы
  auto-fit не посчитал TE/VAE резидентными и оставил запас под prefix-кэш.
- `qwen_image_2_1_prefix_cache_type=q8_0` — prefix KV-кэш (текст + vision-токены
  референсов) в 8 бит: 1.06 ГБ вместо 2 ГБ (f16) на префикс ~4096 токенов.
- `--llm_vision` обязателен: без vision-весов Qwen3-VL редактирование не работает
  (sd.cpp: «Qwen Image 2.1 editing requires Qwen3-VL vision weights»).

## Canvas при редактировании

`edit_image_files` берёт пропорции **первого** референса (`<image 1>` — тот, чья
композиция выживает) и сохраняет площадь пресета; стороны кратны 32. Пресет
1024x1024 у портретного референса тянул бы и сдвиг композиции, и лишние шаги.

## LoRA-стек (PRP 29.10.26; F3 29.10.26 — папки по базам)

- Каталог: ключ `image_lora_dir` в `app_config.json` хоста; дефолт
  `<KingOrchData>/image_loras` (`engine::image_lora_dir_early()`).
  Спавн передаёт `--lora-model-dir` ТОЛЬКО если папка существует — генерация
  без лоры обязана работать как раньше.
- **F3 29.10.26 — папки по базам**: лора привязана к именам/размерностям
  слоёв конкретного диффузионного бэкбона (Qwen-Image ≠ Krea-2) —
  кросс-базное применение sd.cpp распознаёт как частичное совпадение тензоров
  (тихий шум). Поэтому каталог разложен на ПОДПАПКИ базы:
  `<image_loras>/qwen/`, `<image_loras>/krea2/…`; бандл каталога несёт
  поле `lora_base` (`ImageBundleEntry.lora_base`). Резолв ищет файлы ТОЛЬКО
  в подпапке базы активного бандла, спавн передаёт `--lora-model-dir
  <dir>/<base>` (подпапка создаётся автоматически, если корень есть).
  `base = None` — легаси-плоский корень (видео-ветка и старые вызовы).
- Тело `/sdcpp/v1/img_gen`: структурное поле `lora: [{path, multiplier,
  is_high_noise: false}]`; `path` — имя файла ОТНОСИТЕЛЬНО `--lora-model-dir`
  (абсолютные пути и пути вне рута сервер отклоняет; теги `<lora:...>` в
  промпте сервер принципиально не парсит — api.md sd.cpp, «LoRA Rules»).
  Та же сериализация, что у видео-ветки (`build_payload`).
- Резолв имён (SSOT): `engine::resolve_lora_specs_for_base(dir,
  &[(name, multiplier)], base)` → `Vec<LoraSpec>` — per-item сила на каждый
  элемент (кламп 0..2 внутри ядра); `resolve_lora_names_for_base(dir, names,
  mult, base)` — тонкая обёртка (одна сила на весь список — легаси-форма).
  Плоские `resolve_lora_specs`/`resolve_lora_names` = обёртки над `base=None`.
  Стем-матчинг без учёта регистра, коллизия стема — по приоритету
  расширений (`.safetensors` первым). Несуществующее имя — честный отказ
  с перечнем стемов ИМЕННО этой базы; отсутствующая подпапка базы — честный
  отказ с инструкцией о структуре; пустой список = «без лоры».
- Лог-контроль: stdout-сканер `engine::scan_lora_applied_line` ловит строку
  применения «X / N … tensors … applied»; при X < N — `log::warn!` (симптом
  несовместимости варианта модели, leejet issue #2083). Гейт результата —
  сама картинка, лог процесс не валит.

## Preflight

Три вердикта: `fast` (всё в VRAM), `offload` (auto-fit разнесёт веса по RAM/VRAM,
честная запись в лог), `insufficient` (понятная ошибка). CPU-машина без NVML → offload по RAM.

## Rust API (для хоста/тулов)

```ignore
tauri_plugin_image_engine::engine::generate_image_simple(.., &loras, ..)?; // &[LoraSpec]
tauri_plugin_image_engine::engine::edit_image_files(.., &loras, ..)?;    // ref_paths в порядке conditioning
tauri_plugin_image_engine::engine::resolve_lora_specs_for_base(&dir, &[(name.to_string(), 1.0)], Some("qwen"))?; // SSOT-резолв базы, per-item силы
tauri_plugin_image_engine::engine::resolve_lora_names_for_base(&dir, &names, mult, bundle.lora_base.as_deref())?; // обёртка: одна сила на список
tauri_plugin_image_engine::engine::image_lora_dir_early();               // без AppHandle
tauri_plugin_image_engine::engine::engine_dir_early();                   // без AppHandle
tauri_plugin_image_engine::engine::bundle_dir_early();                   // без AppHandle
```

## JS API

`guest-js/index.ts` — типизированные invoke-обёртки + `<image-engine-panel>` /
`<image-bundle-panel>` (web-components.ts). Оба канала из одного источника:
`dist-js/` (npm) и `api-iife.js` (vanilla, esbuild). Артефакты коммитятся.
