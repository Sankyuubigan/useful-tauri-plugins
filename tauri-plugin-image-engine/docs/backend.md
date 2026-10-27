# Политики размещения весов `sd-server` (auto-fit / offload)

Единая точка фактов о placement-флагах движка `sd-server.exe` (leejet
stable-diffusion.cpp). На неё ссылаются `build_spawn_args` обоих движков
(`image_engine.rs`, `video_engine.rs`) и preflight (`preflight.rs`).

## Два режима

| Режим | Флаги | Что делает |
|---|---|---|
| **auto-fit** (дефолт движка: `on`) | `--auto-fit on --max-vram <бюджет>` | Планировщик сам раскладывает weights/params по устройствам по приоритету `diffusion > te > vae` с учётом свободной VRAM/RAM (`backend_fit.cpp`). |
| **offload-to-cpu** | `--offload-to-cpu` | Подставляет `--params-backend '*=cpu'`: ВСЕ веса живут в RAM, GPU-копии стримятся по PCIe на каждом шаге. |

## Ключевые инварианты (обязательные для обоих стеков)

1. **Явный `--params-backend` отключает auto-fit.** Первоисточник sd.cpp:
   «Explicit --params-backend assignments disable auto-fit». Поэтому
   `--offload-to-cpu` и `--auto-fit` — ВЗАИМОИСКЛЮЧАЮЩИЕ политики: смесь даёт
   неопределимое состояние размещения. `build_spawn_args` видео запрещает смесь
   и отсутствие обеих политик (отказ называет флаги).
2. **`--max-vram` обязателен вместе с `--auto-fit`** в видео-спавне: бюджет
   VRAM задаётся явно (замер 27.10.26 — `--max-vram -1`, резерв ~1 ГБ от
   стартового свободного VRAM; рекомендация sd.cpp для «модели, которая
   влезает в карту»).
3. **`--fa` / `--diffusion-fa`** — image-специфичные флаги скорости;
   видео-спавн их не копирует.

## Замеры (проверяемые артефакты)

### Image-Qwen-стек (Krea2/turbo, 4070 Ti SUPER 16 ГБ)

- Без auto-fit (все веса в RAM, стриминг PCIe): 28 шагов 1024² → 272 с.
- С auto-fit diffusion резидентен в VRAM → ~10 с/шаг вместо ~1 (`image_engine.rs`).
- Урок: для image-стека offload-to-cpu НЕ передавать никогда.

### Видео MiniMax H3 q4km (бенч 27.10.26, draft 4 шага + DMAD, 124 кадра, 864×480)

Артефакты: `king_orch_3/test/bench/video_h3_autofit/` (report_*.json,
server_*.log, clip_*.webm); offload-контроль времени —
`king_orch_3/test/bench/video_h3_turbo/report.json` (конфиг dmad4).

| Политика | RSS-пик | VRAM-пик | Время (124 кадра) |
|---|---|---|---|
| `--offload-to-cpu` (контроль 26.10.26) | 26.8–28.6 ГБ (dmad8; dmad4 6.2 — сэмпл-артефакт) | 15.26 ГБ | 130.8 с (медиана) |
| `--auto-fit on --max-vram -1` | **24.65 ГБ** | **15.26 ГБ** | **153.6 с** (1.17×) |

Раскладка auto-fit по логу сервера (факты, не догадки):

```
auto-fit: --backend "diffusion=CUDA0,te=CUDA0,vae=CUDA0"
          --params-backend "diffusion=cpu,te=cpu,vae=disk"
total params memory size = 29031.05MB (VRAM 5558.10MB, RAM 23472.95MB):
  text_encoders 12497.02MB(RAM), diffusion_model 10975.93MB(RAM), vae 5558.10MB(VRAM)
```

То есть у видео auto-fit экономит RAM не «резидентностью diffusion в VRAM»
(диффузион params остаются в RAM), а выгрузкой VAE params на disk (mmap) —
отсюда порог RAM 28.5 ГБ (24.65 × 1.15) против 30.0 у offload. Время дороже
на 17% (paging VAE с SSD при декоде) — в пределах гейта ≤1.5×.

## Где живут пороги

`ram_min_gb` / `vram_min_gb` / `vram_fast_gb` — в `video_models_catalog.json` /
`image_models_catalog.json` (SSOT каталога), из ЗАМЕРА (пик × 1.15 маржа,
округление вверх), не с потолка. Preflight сравнивает их с фактами машины
(NVML/sysinfo) и отказывает канонами `preflight::resource_markers`
(«Недостаточно RAM/VRAM/места на диске/памяти»).
