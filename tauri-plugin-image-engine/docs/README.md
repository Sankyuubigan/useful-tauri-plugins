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

## Preflight

Три вердикта: `fast` (всё в VRAM), `offload` (стейджинг из RAM, честная запись
в лог), `insufficient` (понятная ошибка). CPU-машина без NVML → offload по RAM.

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
