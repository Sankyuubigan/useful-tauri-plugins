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

Один entry по умолчанию: diffusion Q6_K + TE Q4_K_M + mmproj F16 + VAE bf16
(~12.9 ГБ). Пресеты генерации (`cfg_scale 6.0`, `euler`, `1024x1024`, `steps 28`,
`seed -1`) — только в каталоге. Пороги: `vram_fast_gb 16` / `vram_min_gb 8` /
`ram_min_gb 16`.

## Флаги запуска `sd-server` (критично для скорости)

```
--fa --diffusion-fa --auto-fit on --max-vram -1
--model-args qwen_image_2_1_prefix_cache_type=q8_0
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

## Preflight

Три вердикта: `fast` (всё в VRAM), `offload` (auto-fit разнесёт веса по RAM/VRAM,
честная запись в лог), `insufficient` (понятная ошибка). CPU-машина без NVML → offload по RAM.

## Rust API (для хоста/тулов)

```ignore
tauri_plugin_image_engine::engine::generate_image_simple(..)?;
tauri_plugin_image_engine::engine::edit_image_files(..)?; // ref_paths в порядке conditioning
tauri_plugin_image_engine::engine::engine_dir_early();    // без AppHandle
tauri_plugin_image_engine::engine::bundle_dir_early();    // без AppHandle
```

## JS API

`guest-js/index.ts` — типизированные invoke-обёртки + `<image-engine-panel>` /
`<image-bundle-panel>` (web-components.ts). Оба канала из одного источника:
`dist-js/` (npm) и `api-iife.js` (vanilla, esbuild). Артефакты коммитятся.
