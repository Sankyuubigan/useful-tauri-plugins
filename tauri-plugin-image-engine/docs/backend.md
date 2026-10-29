# Политики размещения весов `sd-server` (auto-fit / offload / disk)

Единая точка фактов о placement-флагах движка `sd-server.exe` (leejet
stable-diffusion.cpp). На неё ссылаются `build_spawn_args` обоих движков
(`image_engine.rs`, `video_engine.rs`) и preflight (`preflight.rs`).

## Три режима

| Режим | Флаги | Что делает |
|---|---|---|
| **auto-fit** (дефолт движка: `on`) | `--auto-fit on --max-vram <бюджет>` | Планировщик сам раскладывает weights/params по устройствам по приоритету `diffusion > te > vae` с учётом свободной VRAM/RAM (`backend_fit.cpp`). В RAM резервирует лишь max(2 ГиБ, 10%) — поэтому вливает туда ВСЕ веса, куда влезают. |
| **offload-to-cpu** | `--offload-to-cpu` | Подставляет `--params-backend '*=cpu'`: ВСЕ веса живут в RAM, GPU-копии стримятся по PCIe на каждом шаге. |
| **disk** (29.10.26) | явный `--params-backend te=disk,…` | Явное per-module размещение со спец-значением `disk`: веса читаются из файла модели ЛЕНИВО и **освобождаются после использования** (upstream docs/backend.md «reloads parameters from the model file as needed, and releases those parameter buffers after use»). Явный `--params-backend` отключает auto-fit сам, независимо от порядка аргументов. |

## Ключевые инварианты (обязательные для обоих стеков)

1. **Ровно ОДИН класс политики на спавн** (проверяет `build_spawn_args`
   видео): offload / auto-fit / явный `--params-backend`. Смесь — отказ:
   `--offload-to-cpu` подставляет `--params-backend`, а явный
   `--params-backend` отключает auto-fit — вместе дают неопределимое
   состояние размещения.
2. **`--max-vram` обязателен вместе с `--auto-fit`** в видео-спавне: бюджет
   VRAM задаётся явно (замер 27.10.26 — `--max-vram -1`, резерв ~1 ГБ от
   стартового свободного VRAM; рекомендация sd.cpp для «модели, которая
   влезает в карту»). Для disk-политик замер 29.10.26 — `--max-vram -2`
   (резерв ~2 ГиБ под Windows/DWM).
3. **`--fa` / `--diffusion-fa`** — image-специфичные флаги скорости;
   видео-спавн их не копирует.

## Адаптивная лестница видео-бандла (29.10.26)

`VideoBundleEntry.placement` — лестница вариантов от требовательной к ОЗУ
к лёгкой; при спавне `resolve_placement` выбирает **самый требовательный
из влезающих** в свободную ОЗУ («жрёт сколько есть, остальное стримится
с SSD/NVMe»). Ни один не влез — честный отказ с порогом самого лёгкого.
Пороги каждого варианта — ТОЛЬКО из замера (RSS-пик × 1.15, округление
вверх). Легаси-режим (без `placement`) — единственная политика из
`spawn_flags`/`ram_min_gb`, как раньше.

## Замеры (проверяемые артефакты)

### Image-Qwen-стек (Krea2/turbo, 4070 Ti SUPER 16 ГБ)

- Без auto-fit (все веса в RAM, стриминг PCIe): 28 шагов 1024² → 272 с.
- С auto-fit diffusion резидентен в VRAM → ~10 с/шаг вместо ~1 (`image_engine.rs`).
- Урок: для image-стека offload-to-cpu НЕ передавать никогда.

### Видео MiniMax H3 q4km — политики размещения (draft dmad4, 124 кадра, 864×480)

Контроль времени — бенч 26.10.26 `king_orch_3/test/bench/video_h3_turbo/`
(конфиг dmad4, медиана 130.8 с у offload-контроля). Артефакты:
`king_orch_3/test/bench/video_h3_autofit/` (27.10.26) и
`king_orch_3/test/bench/video_h3_disk/` (29.10.26, bench_disk.py).

| Политика | RSS-пик | VRAM-пик | Время (124 кадра) |
|---|---|---|---|
| `--offload-to-cpu` (контроль 26.10.26) | 26.8–28.6 ГБ (dmad8; dmad4 6.2 — сэмпл-артефакт) | 15.26 ГБ | 130.8 с (медиана) |
| `--auto-fit on --max-vram -1` (27.10.26, старый бинарь) | 24.65 ГБ | 15.26 ГБ | 153.6 с |
| `--auto-fit on --max-vram -1` (29.10.26, текущий бинарь — контроль) | 24.84 ГБ | 15.19 ГБ | 153.2 с |
| `--params-backend te=disk,diffusion=cpu,vae=disk` (сырой замер) | 13.66 ГБ | 15.13 ГБ | 150.3 с |
| то же + `--max-vram -2` (боевой флаг лестницы) | **13.74 ГБ** | **14.03 ГБ** | **150.2 с** (0.98× контроля) |
| `--params-backend te=disk,diffusion=disk,vae=disk` (сырой замер) | 2.72 ГБ | 15.13 ГБ | 171.2 с (1.12×) |
| то же + `--max-vram -2` (боевой флаг лестницы) | **2.72 ГБ** | 14.35 ГБ | **174.2 с** (1.14× контроля) |

Все disk-замеры и контроль 29.10.26 — ОДИН бинарь движка, одна машина, один
пейлоад dmad4 (seed 42); совпадение с контролем 27.10.26 (153.6 vs 153.2 с)
подтверждает воспроизводимость замеров.

Раскладка auto-fit по логу сервера (факты, не догадки):

```
auto-fit: --backend "diffusion=CUDA0,te=CUDA0,vae=CUDA0"
          --params-backend "diffusion=cpu,te=cpu,vae=disk"
total params memory size = 29031.05MB (VRAM 5558.10MB, RAM 23472.95MB):
  text_encoders 12497.02MB(RAM), diffusion_model 10975.93MB(RAM), vae 5558.10MB(VRAM)
```

**Выводы 29.10.26 (из замеров, не из теории):**

1. **disk-политики доминируют auto-fit для видео-стека H3**: te_disk легче
   (13.74 vs 24.84 ГБ RSS) И не медленнее (0.98×) — auto-fit держал ~23.5 ГБ
   весов в RAM впустую. all_disk ещё легче (2.72 ГБ) за +14% времени
   (стриминг diffusion с NVMe на каждом шаге, гейт G2 ≤2× — PASS).
2. **`--max-vram -2` стоит ≤3 с** и отдаёт ~0.8–1.1 ГиБ VRAM рабочему
   столу — внедрено в disk-флаги каталога.
3. Пороги лестницы q4km: te_disk **16.0 ГБ** (13.74×1.15), all_disk
   **3.2 ГБ** (2.72×1.15). На 16 ГБ ОЗУ машине доступно обычно 8–12 ГБ →
   выбирается all_disk; на 32+ ГБ → te_disk. Auto-fit из лестницы q4km
   удалён (доминируется).

### Где живут пороги

`ram_min_gb` / `vram_min_gb` / `vram_fast_gb` (легаси) и `placement[].ram_min_gb`
(лестница) — в `video_models_catalog.json` / `image_models_catalog.json`
(SSOT каталога), из ЗАМЕРА (пик × 1.15 маржа, округление вверх), не с потолка.
Preflight сравнивает их с фактами машины (NVML/sysinfo) и отказывает канонами
`preflight::resource_markers` («Недостаточно RAM/VRAM/места на диске/памяти»).
Том бандла на HDD → честный warn в резюме preflight (стриминг с HDD мучителен).
