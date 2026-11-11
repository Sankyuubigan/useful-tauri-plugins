# 📺 Реестр YouTube-роликов (SSOT)

> **Правило владельца (08.11.26):** все ролики YouTube, которые владелец скидывает в чат,
> записываются СЮДА на будущее. Каждая запись переживает сессии — это постоянный реестр,
> не задачный файл. При добавлении записи заполнять все поля таблицы + выводы, если уже
> есть (если разбор ещё не делался — пометить «не разобрано»).

| # | Добавлен | Ссылка | Канал / автор | Тема | Что взяли / выводы |
|---|----------|--------|---------------|------|--------------------|
| 1 | 04.11.26 | [youtu.be/qk0o4S0Beu4](https://youtu.be/qk0o4S0Beu4) | AI Generation | MiniMax H3 + Regenerate 2K (ComfyUI-воркфлоу Open-H3-Regenerate-2K) | Источник матрицы сравнения A–E (план: `tasks/261105_1528-92e5af13/07.11.26 H3-Regenerate 2K сравнение с нашим стеком.md`). Из их воркфлоу взяты армы B (res_multistep/simple, 960×544), C (hires steps 3 / strength 0.9), D (лора sharp_curve_branches) |
| 2 | 08.11.26 | [youtube.com/watch?v=LUCQBgjFnt8](https://www.youtube.com/watch?v=LUCQBgjFnt8) | neuroshort | «MiniMax H3 infinity video extend» (публикация 12.10.2026) | Воркфлоу `h3_obvpm_timeline_r2v_neuroshort.json` (obvpm-timeline, chanon) лежит в `test/bench/video_hires/cases_h3cmp/`. Разбор 08.11.26 — раздел «obvpm neuroshort» в план-файле: хитрая подача референсов = склейка фото в лист с кроп-боксами (LoadImagesCompose) + структурный промпт `subject_definitions`. Переносимые фишки — пробы X25/X26; extend/.mctx в sd-cli НЕ переносим |
| 3 | 11.11.26 | [youtube.com/watch?v=Ngc6vmyGOyU](https://www.youtube.com/watch?v=Ngc6vmyGOyU) | The AI Brief | «MiniMax H3 Just Got 2.6x Faster (New Veda Attention)» | Ролик был разобран в прошлых сессиях (занесён в реестр 11.11.26): источник контрольного замера final 20 шагов. См. SOURCES_VIDEO.md «Разборы юзера» |
| 4 | 11.11.26 | [youtube.com/watch?v=e3qljriQYOk](https://www.youtube.com/watch?v=e3qljriQYOk) | Junkun Studio | «Minimax H3 Ultra (New Loras, New Sampling and New Height) — Blazing Fast!» | Ролик был разобран в прошлых сессиях (занесён в реестр 11.11.26): новые лоры/сэмплинг — справочно в SOURCES_VIDEO.md |
| 5 | 11.11.26 | [youtube.com/watch?v=t02ttse7Qhw](https://www.youtube.com/watch?v=t02ttse7Qhw) | CottonByte AI | «MiniMax H3's Secret Sauce: Faster 4K Video, Less VRAM» | SelfLift progressive sampling (three-pass «secret sauce») разобран: ранние шаги на низком разрешении → подъём латента; быстрее/меньше VRAM, но мелкие артефакты. + de-oil LoRA + DLSS-апскейл |
| 6 | 11.11.26 | [youtube.com/watch?v=mMxny2hTnwA](https://www.youtube.com/watch?v=mMxny2hTnwA) | GRAWWWW | «MiniMax H3 in ComfyUI: 650s to 72s with LoRA + Ref2VA-VSA (Real Benchmark)» | Источник VSA-замеров ref-режима: репа Kablex/ComfyUI-Ref2VA-VSA (SOURCES_VIDEO.md, справочно) |
| 7 | 11.11.26 | [youtube.com/watch?v=gMlFU9MMryc](https://www.youtube.com/watch?v=gMlFU9MMryc) | CongZ AI | «MiniMax H3 ComfyUI Workflow: The Best Balance of Speed & Quality» | «Best Balance»: merged H3, accel-LoRA, ref-лимиты ≤3 картинки — согласуется с нашим лимитом 1–3 |
| 8 | 11.11.26 | [youtube.com/watch?v=e1sJ78nHLDM](https://www.youtube.com/watch?v=e1sJ78nHLDM) | AIQUEST | «Upscale Any Video to 4K for FREE! (FlashVSR Plus Kaggle Setup)» | РЕШЕНО владельцем 21.10.26: не делаем — весовой формат safetensors, движок не исполнен; в плагин только отдельный Python-сайдкар |
| 9 | 11.11.26 | [youtube.com/watch?v=LfjGcnkHkjc](https://www.youtube.com/watch?v=LfjGcnkHkjc) | Veteran AI | «H3 Character Swap 1.5: 2 Workflows for Face Swaps, Expressions & Crowds» | Источник кандидата `lora_swap`: Character Swap LoRA akatz-ai v1.5 (SOURCES_VIDEO.md «Кандидаты») — НЕ скачана, НЕ тестирована |

## Что записывать (шаблон)

1. Дата добавления (по сообщению владельца в чате).
2. Полная ссылка.
3. Канал/автор, название/тема, дата публикации (если есть).
4. Локальные файлы от ролика (воркфлоу JSON и т.п.) — путь.
5. Выводы/что взяли — или «не разобрано» + ссылка на план-файл, где разберём.
