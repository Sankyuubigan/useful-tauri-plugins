# SOURCES_VIDEO — источники видео-стека MiniMax H3 (PRP 21.10.26, Фаза 1)

Сборка первоисточников по видео-генерации H3 в плагине. Бандлы/цифры —
`video_models_catalog.json` + §9 `BUNDLES.md`; этот файл — про то, ОТКУДА
факты и что проверено/не проверено.

## Первоисточники (проверенные)

- **sd.cpp `docs/minimax_h3.md`** (leejet/stable-diffusion.cpp, master):
  4 компонента (`--diffusion-model`, `--vae`, `--audio-vae`, `--llm`), режимы
  T2VA/I2VA/FL2VA/Ref2VA (`--init-img`, `--end-img`, `--ref-image`/`--ref-video`/
  `--ref-audio`), дефолт CFG 1.0 / 864×480 / 24 fps, работоспособность
  `--offload-to-cpu` для video.
- **sd.cpp `examples/server/api.md`** (пример `vid_gen`, 2026-10):
  `POST /sdcpp/v1/vid_gen` (202 Accepted) + `GET /sdcpp/v1/jobs/{id}` + cancel;
  контракт см. §9 BUNDLES.md / `build_payload` (1:1 с примером api.md).
- **unsloth MiniMax-H3-GGUF** (SSOT весов, «всё-в-одном» репо):
  `https://huggingface.co/unsloth/MiniMax-H3-GGUF` — pruned diffusion GGUF
  (Q2_K 6.26 ГБ / Q3_K 8.16 ГБ / Q4_K_M 10.64 ГБ для fl2va_pruned и
  ref2va_pruned), TE GGUF (Q2_K_M 12.2 ГБ, Q4_K_M 17 ГБ), VAE
  (`vae/`: video fp16 4.85 ГБ + audio fp32 605 МБ). Альтернативы:
  `leejet/MiniMax-H3-GGUF`, `Abiray/MiniMax-H3-GGUF` — не используются.
- **`https://github.com/wildminder/awesome-minimax-H3`** — агрегатор гайдов и
  LoRA; источник ссылок ниже.
- **Гайды локального запуска 8 ГБ**: reventadirecta-пост, hellogen-гайд —
  ориентиры для класса Q3_K + offload (не внедрившими словами, численно
  подтверждает bench фазы 1).

## Лицензионная нота MiniMax H3 (обязательно к прочтению)

**Community License H3: не лицензировано для EU/UK/KR/US без авторизации.**
Текст лицензии поставляется с моделью (репо unsloth/MiniMax-H3-GGUF, раздел
Community License). Пользователь принимает лицензию сам при скачивании весов;
плагин не пересылает и не валидирует лицензию — только информирует этой
заметкой (и в GUI — примечанием бандла).

## Ref-режим Ref2VA — правила агента (по верифицированным субтитрам)

- **>6 image-референсов ломает следование** (технически до 9 возможно).
  Правило агента: ≤3 image-референса + ≤3 аудио-референса.
- **Character sheet 4-view** (экваторы лица) сильно повышает консистентность.
- **Видео-референсы не рекомендуются** авторами воркфлоу (медленно, плохо
  следование).

## Turbo-LoRA — внедрено: DMAD 4-step (бенч 26.10.26)

Поле `lora[]` vid_gen подтверждено живым прогоном (замер —
`king_orch_3/test/bench/video_h3_turbo/`, гейты — `tasks/26.10.26 Ускорение
видео H3 — turbo-LoRA бенч (Фаза 0-1).md`):

- **Файл**: `ZhengmingYu/DMAD/minimax_h3/dmad_minimax_h3_4step_full_critic.safetensors`
  (1 383 674 320 Б, diffusers-формат, rank-128, 312 модулей) — в каталоге роль
  `lora_draft`, доставка кнопкой «Скачать» (как остальные файлы бандла).
- **Доказательство применения**: `server_dmad4.log:165` —
  `lora.hpp:1046 — (600 / 600) LoRA tensors have been applied` (runtime,
  без Python). В файле 624 тензора: 24 token_refiner не видны в счётчике —
  риск качества закрывает A/B юзера.
- **Условие (операционная точка DMAD, README)**: 4 шага, cfg 1.0,
  **lcm-сэмплер + simple-шедулер, flow_shift 12** — «ODE samplers such as euler
  are not their operating point». euler/discrete остаётся путём final и
  draft-без-LoRA. Все четыре параметра — поля каталога (`steps_draft`,
  `turbo_sampler`, `turbo_scheduler`, `turbo_flow_shift`), код их не дублирует.
- **Скорость (q4km, 5 с, 864×480, seed 42)**: dmad4 медиана **130.8 с** против
  ctrl20f (final, 20 шагов euler) **505.6 с** → **3.87×** (гейт ≥2.5× PASS);
  dmad8 255.8 с. VRAM-пик 15.6 ГБ.
- **Серверный контракт**: спавн с `--lora-model-dir <папка бандла>`, в payload
  `lora[].path` — имя файла относительно этого каталога (temp/api.md:332-333,
  1201-1207).
- **`flow_shift: 0.0` запрещён** — вырожденная сигма-сетка → CUDA crash
  (диагностика матрицей 2×2, `diag_report.json`); payload опускает ключ при
  отсутствии явного shift, валидатор каталога отклоняет неположительный
  `turbo_flow_shift`.

Кандидаты на будущее (НЕ внедрено, без замеров):

- **RunningHubAI `rh-minimax-h3-dmd-fl2va-8step-turbo-pruned`** (8 шагов,
  обучен на pruned fl2va; ComfyUI-формат ключей) — запасной, если DMAD-ключи
  на другом кванте не лягут.
- **Merge LoRA Larry step-6**: `https://huggingface.co/noname1992/loras/tree/main`
- **FastH3 DMD2 4-step (FastVideo)**
- **lightx2v** (как для Wan-стека)
- **De-oil LoRA** (снимает «oily skin»/выжженные блики H3) — кандидат в
  постоянный пост-набор.
- **Character Swap LoRA v1.5**: `https://huggingface.co/akatz-ai/MiniMax-H3-Character-Swap-LoRA`
  — кандидат в роль `lora_swap`; отдельный bench.

Статус каждой — только после замера; до подтверждения работает
`steps_draft_nolora` (6 шагов euler) без LoRA (роль lora_draft есть, файла
нет → fallback сэмплинг-плана).

## Качество/апскейл — будущие фазы (справочно, НЕ внедряем)

- **SelfLift progressive sampling** (`https://github.com/facok/comfyui-SelfLift`,
  разбор t02ttse7Qhw): ранние шаги на низком разрешении → подъём латента;
  three-pass «secret sauce» — быстрее/меньше VRAM, но мелкие артефакты;
  single-pass — стабильнее. Ориентир для «фаза 2» (16GB/64GB класс).
- **Латентные апскейлеры**: `https://huggingface.co/LBH-123-AI/Minimax_h3_latent_Upscaler`,
  `https://huggingface.co/Alissonerdx/MiniMax-H3-ComfyUI/tree/main/latent_upscaler`.
- **FlashVSR Plus** (видео-суперрезолюция до 4K): `https://github.com/OpenImagingLab/FlashVSR_Plus`
  — **РЕШЕНО юзером 21.10.26: не делаем** (весовой формат safetensors,
  движок не исполнен; в плагин только отдельный Python-сайдкар).
- **SAM 3.1 mask-swap** (`https://github.com/drozbay/MaskVidExperiments`) —
  маски-условия (control_frames) у H3 в vid_gen не работают → только фаза 2.
- **Hybrid модели fl2va+ref2va**: `https://huggingface.co/smhfacct/Minimax-H3-fl2va-ref2va-hybrid-models/tree/main`
  (b25-49 — справочно).
- **Kablex VSA-замеры** (`https://github.com/Kablex/ComfyUI-Ref2VA-VSA`) —
  справочно для качества ref-режима.

## Разборы юзера (ролики) — справочно

Ngc6vmyGOyU (замер final 20 шагов), e3qljriQYOk, t02ttse7Qhw (Self-Lift +
de-oil LoRA + DLSS-апскейл), mMxny2hTnwA, gMlFU9MMryc («Best Balance» —
merged H3, accel-LoRA, ref-лимиты ≤3), e1sJ78nHLDM (FlashVSR — отклонено),
LfjGcnkHkjc (Character Swap). Авторы роликов — ComfyUI-стек; переносимость
в vid_gen проверяется отдельно (LoRA — бенч; маски — нет).
