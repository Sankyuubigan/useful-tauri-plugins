# Движок llama.cpp: интеграция как отдельный процесс (llama-server.exe)

> **Назначение доки:** подробный, воспроизводимый инженерный референс по тому, как движок LLM
> подключается и работает в связке с хостом (эталонный хост — King Orch 3). Написано так, чтобы
> по этой доке архитектуру можно было **один-в-один повторить в любом другом Rust/Tauri (или даже
> не-Tauri) десктоп-проекте**. Каноническая реализация — сам плагин `tauri-plugin-llama-engine`
> этого репозитория.

> **Ключевой принцип (он же §6.7 в `D:\Projects\docusaurus-starter\docs\Sega Mega Note\Моя картотека\software\настройки\global_ai_docs\desktop_rust_tauri_rules.md`):**
> приложение **НЕ линкует llama.cpp нативно** (нет
> PE-импортов, нет DLL рядом с exe). Инференс идёт через **отдельный процесс `llama-server.exe`**
> (полный релиз llama.cpp) по **HTTP на localhost**. Это даёт: отсутствие 20-минутной CUDA-сборки
> в дереве зависимостей приложения, graceful-выгрузку моделей из VRAM через `Drop`, и возможность
> держать несколько бекендов одновременно.

---

## 0. Архитектурная сводка

```
┌──────────────────────────────┐         HTTP (localhost)          ┌──────────────────────────────┐
│  King Orch (Rust/Tauri exe)  │  GET  /health                    │   llama-server.exe            │
│                              │  POST /tokenize                  │   (полный релиз llama.cpp)    │
│  infra::LlamaEngine          │  POST /v1/chat/completions (SSE) │                              │
│   - spawn subprocess         │ ───────────────────────────────▶ │   -m <model.gguf>            │
│   - random port + api-key    │                                   │   --host 127.0.0.1           │
│   - Drop → kill дерева       │ ◀─────────────────────────────── │   --port <rand>              │
│   - точный /tokenize-триминг │   SSE: choices[].delta.content   │   --ctx-size <N>             │
│                              │                                   │   -ngl 999|0  --flash-attn   │
└──────────────────────────────┘                                   └──────────────────────────────┘
        │                                                                  │
        │ GGUF-парсинг (только метаданные, НЕ инференс)                    │ chat_template (jinja из GGUF)
        ▼                                                                  ▼
  llm_gguf.rs: validate_gguf, extract_*_from_gguf               промпт рендерит САМ сервер
```

**Что делает Rust-сторона сама:** запуск процесса, генерация port/api-key, HTTP-клиент,
точный подсчёт токенов через `/tokenize`, обрезку истории, сэмплинг-параметры в запросе,
чтение GGUF-метаданных (размер, вшитые параметры сэмплинга, chat_template).
**Что делает сервер:** рендер промпта по jinja `chat_template` из GGUF, сам инференс, KV-кэш,
грамматики GBNF/json_schema.

---

## 1. Получение бинаря движка (runtime download, НЕ sidecar)

Движок **не собирается** и **не бандлится** в приложение. Он скачивается **пользователем**
в момент установки из GitHub Releases `ggml-org/llama.cpp`:

- Endpoint: список релизов `https://api.github.com/repos/ggml-org/llama.cpp/releases?per_page=30`
  (`LLAMA_CPP_RELEASES`). **НЕ** используется `/releases/latest` (см. ниже).
- **Важно (схема релизов llama.cpp изменилась):** «последний» релиз (тег `v0.3.0` и новее) —
  это **source-only стабильный релиз**, в нём НЕТ готовых бинарников (только `nightly-tag.txt`).
  Сами пребилды `llama-server.exe` публикуются в **nightly-пре-релизах `bXXXX`** (напр. `b10621`).
  Поэтому инсталлятор **сканирует список релизов** (новые сначала) и берёт **первый релиз, в
  котором реально есть** ассет движка для запрошенного варианта (`first_release_with_engine`).
  Запасной путь: читает `nightly-tag.txt` из стабильного релиза и запрашивает релиз по этому тегу.
  Так выбор не зависит ни от того, является ли релиз стабильным/pre-release, ни от конкретного
  формата тега — подобный баг больше не повторится.
- Имя ассета (пример): `llama-b10621-bin-win-cuda-12.4-x64.zip`. Поиск ведётся **по семейству**
  (`-win-cuda-12` / `-win-cuda-13` / `-win-vulkan` / `-win-rocm` / `-win-cpu`), а не по точному
  совпадению строки — смена минорной версии CUDA (13.3→13.7) или переименование тега не ломает
  установку. `hip-radeon` резолвится в реальный ассет `-win-rocm-*.zip`.
- Скачивается **самый свежий** релиз с бинарником (никакого пиннинга версии в текущей реализации).
- Доп. архив CUDA-рантайма (только для cuda-вариантов): `cudart-llama-bin-win-<variant>-x64.zip`.
  С релиза **b10275** CUDA-DLL (`cublas64_*.dll`, `cublasLt64_*.dll`, `cudart64_*.dll`) вынесены
  из основного архива — без них `ggml-cuda.dll` не грузится и сервер **тихо уходит в CPU**.
- SHA-256 верификация: если у ассета есть `digest` в GitHub API, скачанный zip сверяется по хэшу.
- Докачка (HTTP Range): при обрыве докачивается с места (`engine.zip.part` → rename).
- Распаковка: `zip` crate, защита от path-traversal (файл обязан остаться внутри `dest_dir`).
- `engine_meta.json` в папке варианта: `{tag, variant, installed_at}`.

> **Важно для переноса:** это НЕ Tauri sidecar (`externalBin` в `tauri.conf.json` отсутствует).
> Бинарь ищется в рантайме по пути. В `tauri.conf.json` движок не прописан — `bundle.resources`
> бандлит только `agents/`, `mcp_servers/`, каталоги и `*.json`.

### Варианты бекендов (backends)
Несколько могут жить одновременно (как в Jan). Каждый — отдельная папка:

| variant (id)        | Семейство      | Когда использовать |
|---------------------|----------------|--------------------|
| `cpu`               | Cpu            | Любой ПК, без GPU |
| `cuda-12.4`         | Cuda12         | NVIDIA GTX 10xx – RTX 40xx, драйвер CUDA 12+ |
| `cuda-13.3`         | Cuda13         | NVIDIA с драйвером CUDA 13+ (R580+); обязателен для Blackwell (RTX 50xx) |
| `vulkan`            | Vulkan         | Любые GPU (AMD/Intel/NVIDIA) через Vulkan |
| `hip-radeon`        | Hip            | Только современные AMD RX 6000/7000+ (ROCm); старые AMD — Vulkan |
| `auto`              | —              | Подбор по GPU (см. §6) |

Файлы: `llamacpp_installer.rs` (`VARIANT_*`, `EngineFamily`, `install`, `find_engine_asset`,
`find_cudart_asset`, `lift_server_files`, `migrate_legacy_layout`, `migrate_sources_layout`),
`sources.rs` (реестр источников).

---

## 1а. Источники движка (multi-repo) и KV-квантование (KVarN)

### Источники (engine sources)

Официальный релиз `ggml-org/llama.cpp` — не единственный поставщик готовых бинарей.
Плагин поддерживает **несколько источников** (multi-repo), каждый со своим реестром
в `engine_sources.json` (embedded `include_str!` в плагине). Выбор источника хранится
в `app_config.json` (`engine_source`), варианты (cpu/cuda-…/vulkan/hip) стоят **side-by-side**:

```
<llamacpp_dir>/backends/
├── ggml-org/
│   ├── cuda-12.4/…            ← официальные llama-* билды
│   └── cuda-13.3/…
├── beellama/
│   ├── cuda-12.4/…            ← BeeLlama (KVarN KV-квант)
│   └── cuda-13.3/…
└── prismml/
    └── cuda-13.3/…            ← PrismML (ternary-модели Bonsai)
```

| source id | repo | asset_prefix | cudart | prefer_stable | runtime KV |
|-----------|------|--------------|--------|---------------|------------|
| `ggml-org` | `ggml-org/llama.cpp` | `llama-` | `cudart-llama-bin` | false (nightly `bXXXX`) | без кванта (чекбоксы `kv_quant_*` убраны из UI, всегда false) |
| `beellama` | `Anbeeld/beellama.cpp` | `beellama-` | `cudart` | true (только stable `v0.x`) | `--cache-type-k kvarn5 --cache-type-v kvarn4 --kv-tail-tokens 1024` |
| `prismml` | `PrismML-Eng/llama.cpp` | `llama-` | `cudart-llama-bin` | false (каждый билд = релиз `prism-bNNNNN-hash`, не prerelease) | без кванта. Форк для ternary-моделей Bonsai (кванты PQ2_0 / Q2_0_g64 / Q1_0 не грузятся на официальном бинаре); обычные GGUF — лучше `ggml-org` |

Правила:
- **Строить сами НЕ нужно** — только готовые release-asset с GitHub.
- KVarN/аргументы запуска — **в реестре** (захардкожены в `engine_sources.json`), юзеру
  в UI ничего не показывать (только выбор источника: официальный / BeeLlama / PrismML).
- UI-чекбоксы `kv_quant_keys/values` **удалены** (вариант A): на хосте всегда `false`;
  BeeLlama включает KVarN только через `source.runtime`. Старые ключи в
  `app_config.json` принудительно сбрасываются в `load_config` (`sanitize_retired_fields`).
- `prompt_format` UI-селект убран: всегда `Auto` (санитайз в `load_config`).
- RotorQuant (`johndpope/llama-cpp-turboquant@feature/planarquant-kv-cache`) — вне scope:
  на ветке нет готовых release-asset.
- BeeLlama требует `--flash-attn on` для KVarN — уже в `build_cmd` (обязателен).
- **PrismML публикует arm64-ассеты** (`…-win-cpu-arm64.zip`, `…-win-cuda-13.4-arm64.zip`,
  cudart тоже), причём в списке ассетов они идут РАНЬШЕ x64. Хост — x64-only, поэтому
  `find_asset_by_family` и `find_cudart_asset` пропускают всё с `arm64` в имени
  (регрессия `family_fallback_never_picks_arm64_assets`). Точное имя ассета
  (`{prefix}{tag}-bin-win-{variant}-x64.zip`) у PrismML совпадает со схемой ggml-org,
  поэтому реестровая запись отличается от ggml-org только repo/note.
- Внутри архива PrismML `llama-server.exe` лежит в корне (структура как у ggml-org),
  HTTP-API тот же (форк свежего мейнлайна) — резолв бинаря и `/health`/`/tokenize`/
  `/v1/chat/completions` работают без изменений.

> ⚠️ **`source.runtime` применяется ко всем моделям без исключения.** У части
> моделей BeeLlama с `--kv-tail-tokens` **падает при старте**, и причина — не SWA,
> а **совместный KV-кэш между слоями** (Cross-Layer Attention). Проверено на
> метаданных GGUF:
>
> | модель | `block_count` | `*.attention.shared_kv_layers` | BeeLlama |
> |---|---|---|---|
> | Gemma 4 E4B (Q4) | 42 | **18** | падает на слое 24 = `42 − 18` |
> | Gemma 4 12B (Q4) | 48 | 0 | работает |
>
> Ключи `sliding_window` / `key_length_swa` есть у **обеих** моделей (512 против
> 1024), поэтому SWA как объяснение **не подтверждается** — различие ровно в
> `shared_kv_layers`, и `42 − 18 = 24` совпадает со слоем из ошибки.
>
> Ловится дважды: превентивно по метаданным GGUF (до spawn,
> `diagnose_shared_kv_before_spawn`) и реактивно по тексту лога (§3.4а).
> Авто-подмены аргументов нет — качество KV-памяти у юзера молча не меняем.
> Форк сам предупреждает: *«Gemma 4 needs a separate decision… avoiding quantized
> KV cache when Gemma quality is non-negotiable»*.

### Бенчи чувствительности KV-кванта (anbeeld.com)

- https://anbeeld.com/articles — серия бенчей KVarN/KLD/QAT: Qwen 3.6 27B vs Gemma 4 31B,
  лестницы `q4_0 → q8_0 → kvarn*`.
- https://github.com/Anbeeld/beellama.cpp — README с рекомендательными лестницами квантов.
- Доп.: vLLM docs quantized KV, huawei-csl/KVarN, ersiner/kvqbench, HF Daily Papers.

**Ключевой вывод (проверено):** Gemma 4 non-QAT **не квантовать KV** (или только `q8_0`
без tail): 50 SWA-слоёв + окно 1024 → KLD 0.036 vs 0.000936 у Qwen при одинаковом пресете
(~38×), same-top ~80% уже на `q8_0`. Qwen-подобные GQA+full-attn — терпимо до `kvarn*`.

### Красные флаги архитектуры (для выбора/предупреждений)

| Флаг | Почему опасен для KV-кванта |
|------|-----------------------------|
| **SWA / hybrid attention** (Gemma 4: 50 SWA-слоёв, окно 1024) | Окно 1024 + KVarN-tail — высокий KLD, деградация same-top |
| **MQA / мало KV-head** (1–2 head'а) | Квант режет уже узкий канал, ошибки не усредняются |
| **K=V reuse** | Ошибка кванта K идёт в V и наоборот |
| **MLA** (DeepSeek-стиль) | Сжатое latent-представление чувствительно к кванту |
| **pre-RoPE outliers в K** | Длинные хвосты не влезают в узкий квант |
| **Обычный GQA + full-attn** | Зелёный: терпимо до `q8_0`/`kvarn*` (Qwen-класс) |

**Правило:** Gemma 4 non-QAT → `cache_type_k/v = null` (или только `q8_0` без tail).
Детект архитектуры из GGUA — отдельный scope (не в этом изменении).

---

## 2. Раскладка папок и обнаружение бинаря

```
<exe_dir>/
└── llamacpp/                         ← default_dir(exe_dir) = current_exe().parent().join("llamacpp")
    ├── llama-server.exe             ← legacy-фолбэк (до миграции)
    ├── engine_meta.json             ← legacy
    └── backends/
        ├── _cudart/                 ← ОБЩИЙ CUDA-рантайм, SSOT (НЕ копируется в варианты)
        │   ├── cuda-12/             ← cublas64_12.dll, cublasLt64_12.dll, cudart64_12.dll…
        │   └── cuda-13/             ← cublas64_13.dll, …
        └── <source>/                ← source id: ggml-org | beellama
            ├── cpu/
            │   ├── llama-server.exe
            │   └── engine_meta.json
            ├── cuda-12.4/
            │   ├── llama-server.exe
            │   └── engine_meta.json
            └── cuda-13.3/
                ├── llama-server.exe
                └── engine_meta.json
```

> **Почему CUDA-рантайм вынесен в `_cudart/`:**
> BeeLlama — форк llama.cpp и перевыкладывает **тот же** апстримный redistributable NVIDIA,
> что и ggml-org (для cuda-12.4 sha256 `cudart-llama-bin-win-cuda-12.4-x64.zip` и
> `beellama-v0.4.7-cudart-win-cuda-12.4-x64.zip` совпадают байт-в-байт, ~370 МБ).
> Копии в `backends/<source>/<variant>/` давали бы ×N дублей по 370 МБ. Теперь распаковка
> идёт один раз, а DLL находятся через `PATH` дочернего процесса
> (`child_path_with_cudart`) — Windows ищет библиотеки: папка exe → System32 → PATH.
>
> **Ключ папки — мажор CUDA** (`cuda-12` / `cuda-13`), т.к. набор DLL в пределах мажора
> одинаков (все CUDA 12.x грузят `cudart64_12.dll`, `nvrtc64_120_*`).
>
> **Обратная совместимость:** `cudart_ready()` принимает и старую раскладку (DLL в папке
> варианта) и System32 — установки, сделанные до `_cudart/`, продолжают работать.
> Миграция старых копий в `_cudart/` НЕ выполняется (безопаснее); их можно удалить руками.
>
> `_cudart/` лежит вне `backends/<source>/`, поэтому миграции и списки источников/вариантов
> её не видят (`migrate_sources_layout` пропускает, `list_installed_sources` не возвращает).
> Тест: `_cudart_dir_is_not_mistaken_for_a_source_or_variant`.

> **Миграция:** старый формат `backends/<variant>/` (без уровня source) автоматически
> переносится в `backends/ggml-org/<variant>/` (`migrate_legacy_layout` + `migrate_sources_layout`).
> Legacy root `llama-server.exe` → `backends/ggml-org/<variant>/`.

**Резолв пути к серверу** (`LlamaEngine::new_with_mmproj`, `llm.rs`):
```rust
let source = resolve_source(cfg.engine_source);           // "ggml-org" | "beellama"
let selected_variant = llamacpp_installer::resolve_variant(pref);
let mut server_exe = llamacpp_installer::variant_dir(engine_dir, &source, &selected_variant)
    .join("llama-server.exe");
if !server_exe.exists() {
    // legacy root fallback (старый формат до миграции)
    server_exe = engine_dir.join("llama-server.exe");
}
```
Где `engine_dir`:
- если `cfg.llamacpp_dir` задан пользователем (`set_engine_dir`) → он и используется;
- иначе `std::env::current_exe().parent()` → `llamacpp_installer::default_dir(exe_dir)`
  (**НЕ** `app.path().executable_dir()` — см. правило о cwd: §2 «Приложение не должно
  писать в рабочую папку» в `desktop_rust_tauri_rules.md` глобальной доки).

Селектор варианта: `resolve_variant("auto"/None/неизвестный)` → `select_variant()` (подбор по GPU),
явный известный вариант → как есть.
Селектор источника: `resolve_source(None/пусто)` → `"ggml-org"` (дефолт), известный id → как есть.

### ⚠️ ОБЯЗАТЕЛЬНОЕ ПРАВИЛО: определять движок ТОЛЬКО по факту, НИКОГДА не гадать

> **Перед любой работой с движком LLM (тест, репродукция, отладка логов, запуск llama-server)**
> определяй актуальный движок приложения **по факту**, а не по догадкам и не по «первому
> найденному по дереву».

**Алгоритм определения (выполнять в этом порядке, все шаги):**

1. **Прочитать конфиг приложения** `~AppData/Roaming/com.kingorch.app/app_config.json`
   (или `<проект>/src-tauri/src/infra/config.rs` — дефолты). Взять оттуда:
   - `llamacpp_dir` (явный путь движка ОБЯЗАТЕЛЕН к использованию, если задан);
   - `engine_source` (например `ggml-org` / `beellama`; None = `ggml-org`);
   - `engine_variant` (например `cuda-13.3`);
   - `reasoning_budget` (`>0` → сервер запускается с `--reasoning-format deepseek
     --reasoning-budget N --no-reasoning-preserve`);
   - `context_size`; `kv_quant_keys/values` всегда false (UI убран, санитайз в load_config)
   Если `llamacpp_dir` не задан — дефолт `std::env::current_exe().parent().join("llamacpp")`
   (**НЕ** `app.path().executable_dir()`, см. правило о cwd в `desktop_rust_tauri_rules.md` §2).
2. **Реальный бинарь** — только `variant_dir(engine_dir, engine_source, engine_variant).join("llama-server.exe")`:
   `D:\Programs\llamacpp\backends\<source>\<engine_variant>\llama-server.exe`
   (до миграции multi-source: `D:\Programs\llamacpp\backends\<engine_variant>\llama-server.exe`).
   **ЗАПРЕЩЕНО** брать `llama-server.exe` из другой папки `backends/<tag>/...` (например из
   `b9967/.../build/bin/`) — это другой бекенд/версия/источник, к сессии приложения отношения не имеет.
3. **Проверить фактическую версию** через `backends/<source>/<variant>/engine_meta.json`
   (например `b10331` или `v0.4.6`). Сверить, что его лог-строки совпадают с логом сессии:
   - b10331 (cuda-13.3): `selected slot by LCP similarity, f_sim_best = %.3f ... f_keep = %.3f`;
   - более старые/иные сборки (b9967): `... sim_best = %.3f ...` (БЕЗ `f_` в `sim_best`).
   Наличие/отсутствие `f_sim_best` — маркер, по которому видно, какой бинарь реально писал лог.
4. **Сверять cwd**: сервер запускается с `current_dir = engine_dir`
   (`llm.rs` `build_cmd`), лог — `engine_dir/llama_server.log`.
5. Вывод (логи/сигналы) движка сверять с **фактическим** бинарём из шагов 1–3, а не с
   произвольным `backends/<tag>`, найденным первым при поиске файлов.

> **Инцидент-пример (НЕ повторять):** агент искал утилитой поиска любой `llama-server.exe`,
> нашёл первым `backends/b9967/win-cuda-13-common_cpus-x64/build/bin/llama-server.exe` (Jan-дистрибутив)
> и тестировал его, хотя приложение использует `backends/ggml-org/cuda-13.3` (b10331, `engine_variant=cuda-13.3`).
> Результаты тестов на b9967 к реальной сессии неприменимы, а гипотеза «грамматика не применяется»
> долго проверялась на неверном бинаре. Гаданий быть не должно: **только конфиг → source → variant_dir → engine_meta.json**.

---

## 3. Запуск процесса (llm.rs `LlamaEngine::new`)

### 3.1 Подготовка перед spawn
1. Лог аппаратного обеспечения (CPU/RAM через `sysinfo`, GPU/VRAM через `nvml_wrapper` NVML).
2. **Валидация GGUF** (`llm_gguf::validate_gguf`): проверка целостности (объявленный `block_count`
   vs реальные тензоры `blk.N`) **до** старта — чтобы вместо непонятного падения сервера дать
   понятную ошибку.
3. Проверка наличия `llama-server.exe` для варианта (ошибка → «откройте Настройки → установите движок»).
4. **CUDA-runtime preflight** (`llm.rs:213`): для `Cuda13`/`Cuda12` ищем `cublas64_13.dll`/
   `cublas64_12.dll` рядом с сервером ИЛИ в `C:\Windows\System32`. Если нет — явная ошибка
   («GPU-режим не работает, движок тихо уйдёт в CPU»), а не молчаливый фолбэк.
5. Предупреждение при несовместимости бекенда и GPU (только для Blackwell — жёсткая ошибка;
   для прочих — варнинг, запуск всё равно).

### 3.2 Аргументы командной строки
Клозура `build_cmd(use_reasoning)` (`llm.rs:309`):
```text
llama-server.exe
  -m <model_path>
  --host 127.0.0.1
  --port <random 17800..19300>
  --api-key <32 hex>
  --ctx-size <global_ctx_limit>
  -t <threads>                    // (logical_cores/2).max(4)
  -ngl <gpu_layers>               // 999 если GPU, иначе 0
  --flash-attn on
  --no-webui
  --log-file <engine_dir>/llama_server.log
  --mmproj <path>]               // если мультимодалка
  [--cache-type-k q8_0]           // только если kv_quant_keys (на хосте всегда false — UI убран)
  [--cache-type-v q8_0]           // только если kv_quant_values (на хосте всегда false)
  // Для beellama (SourceSpec.runtime из engine_sources.json) — ВМЕСТО/ВМЕСТЕ с q8_0:
  [--cache-type-k kvarn5]         // source.runtime.cache_type_k
  [--cache-type-v kvarn4]         // source.runtime.cache_type_v
  [--kv-tail-tokens 1024]         // source.runtime.kv_tail_tokens (kvarn-хвост)
  [--reasoning-format deepseek]    // если reasoning_budget > 0
  [--reasoning-budget <N>]
  [--no-reasoning-preserve]
```
- `--creation_flags(0x08000000)` (`CREATE_NO_WINDOW`) на Windows — без мелькания консоли.
- `stderr` сервера пайпится в отдельный поток и логируется как `[llama-server] …`
  (туда идут CUDA/ggml-ошибки, которые НЕ попадают в `--log-file`).
- `threads = (available_parallelism / 2).max(4)` (`llm.rs:284`).

### 3.3 Случайные port + api-key (обязательно!)
- Порт: `PORT_MIN=17800`, `PORT_RANGE=1500`. Перебираем до 5 кандидатов, bind-им через
  `TcpListener::bind(("127.0.0.1", candidate))` — берём первый свободный. (Гарантирует, что
  параллельные запуски/старые серверы не конфликтуют; нельзя хардкодить порт.)
- api-key: 32 шестнадцатеричных символа (ГПСЧ `Prng` — xorshift64*, seed от времени+PID).
  Передаётся в заголовок `Authorization: Bearer <api_key>`. **Без ключа сервер на localhost
  уязвим для любого локального процесса.**

### 3.4 Ожидание готовности + reasoning-fallback
- Цикл опроса `GET /health` (таймаут `HEALTH_TIMEOUT = 120s`) пока модель грузится в память.
- Если сервер **умер при старте** и были заданы reasoning-флаги → **перезапуск БЕЗ них**
  (`build_cmd(false)`), т.к. старая версия движка не знает `--reasoning-*`.
- Если `try_wait()` вернул код завершения → ошибка с хвостом `llama_server.log`
  (`read_log_tail`).
- Таймаут → `child.kill()` + ошибка.

### 3.4а Диагностика падения НА СТАРТЕ (не путать с §3.5)

Когда сервер умирает, не успев поднять `/health`, юзер раньше получал
`format!("Движок llama-server завершился при запуске (код {}). {}", code, log_tail)`
— 50 строк ggml-спама, из которых он делал вывод «программа сломалась».
На деле обе типовые причины лечатся в Настройках.

`engine/startup_diagnosis.rs` — классификатор, вызываемый в финальной ветке
цикла запуска (`llm.rs`):

```rust
diagnose_startup_failure(log_tail, &server_trace.diagnostic_lines(), code, &ctx) -> Option<StartupDiagnosis>
```

- **Два источника текста, читаются оба.** CUDA/ggml-ошибки идут в **stderr**
  процесса (в `--log-file` их нет), а ошибки инициализации KV-кэша BeeLlama —
  наоборот, в **log-файл**. Классифицировать только один источник — потерять
  половину случаев.
- `diagnose_cuda_fallback` (§3.5) сюда **не относится**: он вызывается только
  ПОСЛЕ успешного `/health`, а эти ошибки случаются ДО health.
- Юзеру возвращается `user_message` (причина + конкретное действие);
  полный технический след уходит в `log::error!` (`log_full_trace`) — вкладка
  «Логи» + телеметрия. Ничего не теряется, но мусор наружу не выдаётся.
- `None` (причина не распознана) → вызывающий код сохраняет прежнее
  поведение и показывает хвост лога.

#### Превентивная проверка (ДО spawn)

Реактивный классификатор срабатывает после падения — но юзер к этому моменту
уже заплатил два неудачных запуска (~3 с) и сырой трейс в логах. Часть причин
детерминирована и видна **в метаданных модели**, поэтому проверяется заранее
(`llm.rs`, сразу после `kv_spec_from_source`, до VRAM-расчётов и spawn):

```rust
diagnose_shared_kv_before_spawn(&model_path, &diag_ctx) -> Option<StartupDiagnosis>
```

- Условие: `ctx.kv_tail_tokens > 0` (источник просит KV-хвост) **и**
  `*.attention.shared_kv_layers > 0` в GGUF.
- `kv_tail_tokens` берётся из `kv_spec_from_source` — то есть из
  `source.runtime`, а не из сравнения `source_id == "beellama"`. Любой будущий
  источник с KV-хвостом проверяется автоматически.
- Ключ читается **под префиксом архитектуры** из самого файла
  (`extract_gguf_arch` → `extract_u32_with_arch(path, Some(arch), …)`). Передавать
  `arch = None` нельзя: современный GGUF хранит `gemma4.attention.*`, а не
  `llama.*`, и проверка молча выключится.
- Нет ключа / `shared_kv_layers == 0` → `None`, модель считается совместимой
  (Gemma 4 12B так и запускается на BeeLlama).
- Хардкода имён моделей нет: правило опирается на архитектурное свойство.

Оба пути (превентивный и реактивный) печатают **один и тот же** текст —
общая функция `kv_tail_diagnosis(ctx, detected, shared_layers)`, флаг `detected`
(`preflight` / `after_crash`) уходит только в лог.

| Признак в логе | `FailureKind` | Смысл для юзера |
|---|---|---|
| `KV tail has no execution descriptor for layer N` | `KvTailIncompatible` | BeeLlama не умеет KV-хвост для слоёв с общим KV (Cross-Layer Attention, `shared_kv_layers > 0`) → сменить источник на `ggml-org` |
| `no kernel image is available for execution on the device` | `CudaArchMissing` | в сборке нет ядер для карты → поставить `cuda-12.4` (или `cuda-13.x` для Blackwell) |
| `driver version is insufficient` | `CudaDriverTooOld` | обновить драйвер (527.41+ / 580+) |
| `out of memory` / `cuMemAlloc failed` | `CudaOom` | уменьшить контекст / взять модель полегче |
| `failed to initialize cuda` | `CudaInitFailed` | обновить драйвер / сменить вариант |
| ничего, но код `1` / `0xC0000409` | `EngineCrash` | переустановить движок + назван рекомендованный вариант |

> **Важно:** `CudaArchMissing` и `CudaDriverTooOld` — разные причины, и
> путать их нельзя: на старой карте сообщение «обновите драйвер» бесполезно,
> там нужен другой вариант сборки. Маркеры у них разные, поэтому и проверяются
> раздельно.

> **Принцип:** никаких скрытых перезапусков и никакого авто-скачивания движка
> внутри `LlamaEngine::new` — приложение объясняет и предлагает, решение
> принимает юзер. Названия вариантов в текстах берутся из
> `variant_label()`, чтобы юзер находил их в дропдауне по точному названию.

### 3.5 Проверка реального попадания в VRAM (после health)
- Замеряем дельту VRAM через NVML (`vram_after - vram_before`).
- Если `diff > 100 MB` и GPU-намерение → `engine_mode = "gpu"`.
- Если GPU-намерение, но VRAM не выросла → `engine_mode = "cpu"` + `diagnose_cuda_fallback`
  парсит лог сервера (паттерны: `no kernel image`, `driver version is insufficient`,
  `failed to initialize cuda`) и пишет ПРИЧИНУ (не выдуманный диагноз).
- Предупреждение о нехватке RAM: `estimate_vram_mb(model, ctx, kv*) + 512 MB` vs `free_memory()`;
  не блокирует запуск, только варнинг.

### 3.6 Гарантированная зачистка (критично!)
- `impl Drop for LlamaEngine`: `process_util::kill_process_tree(&mut child)` (убивает ВСЁ дерево,
  не только прямого ребёнка) → `child.wait()`.
- `process_util::register_engine_pid(child.id())` + **Windows Job Object** с
  `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`: когда приложение насильственно закрывается (через
  Диспетчер), ОС сама убивает сервер. `assign_child_to_kill_job` (`process_util.rs:136`).
- Финальный сейфет: `kill_active_engines()` вызывается в обработчике `ExitRequested` в `main.rs`
  (докиляет по `ACTIVE_ENGINE_PIDS`, если `Drop` не успел).

---

## 4. HTTP-контракт с сервером

База: `http://127.0.0.1:<port>`. Auth: `Authorization: Bearer <api_key>`.
Клиент: `reqwest::blocking::Client` с `connect_timeout = 5s`.

### 4.1 `GET /health`
`LlamaEngine::is_healthy` (`llm.rs:624`) — статус 200 = готов. Используется при старте (3s timeout).

### 4.2 `POST /tokenize`
Тело: `{content, add_special:true, parse_special:true}`. Ответ: `{tokens:[i64]}`.
Используется для **точного** подсчёта токенов промпта (история + системный промпт + user).
`LlamaEngine::tokenize` (`llm.rs:651`), обёртка `get_tokens_count` (`llm.rs:608`).

### 4.3 `POST /v1/chat/completions` (OpenAI-совместимый, streaming)
- Промпт рендерит **САМ сервер** (jinja `chat_template` из GGUF). Клиент шлёт только `messages[]`.
  Если `prompt_format == "Auto"` — пытаемся взять `tokenizer.chat_template` из GGUF и отрендерить
  через `PromptFormat::format_messages_jinja`; иначе ручная склейка по `actual_format`.
- Тело запроса (`ChatCompletionsRequest`, `llm.rs:742`):
  ```jsonc
  {
    "messages": [ {"role":"user","content":"..."} ],   // вложения → content-parts (image_url, base64)
    "n_predict": <max_tokens>,
    "stream": true,
    "temperature": 0.5, "top_k": 40, "top_p": 0.95, "min_p": 0.1,
    "repeat_penalty": 1.15, "presence_penalty": 0.0, "frequency_penalty": 0.0,
    "dry_multiplier": 0.8, "dry_base": 1.75, "dry_allowed_length": 2, "dry_penalty_last_n": 256,
    "xtc_probability": 0.0, "xtc_threshold": 0.1,
    "stop": ["<|im_end|>","</s>", ...],   // DEFAULT_STOP_WORDS + форматные стоп-слова
    "cache_prompt": true,
    "seed": -1,
    "grammar": "<GBNF>"     // опц., consume-and-clear; либо json_schema вместо grammar
    "json_schema": {...}    // опц.
  }
  ```
  Перед отправкой сэмплинг клампится: `min_p.max(0.0)`, `repeat_penalty.max(1.0)`, `temperature.max(0.01)`.
- **Вложения (мультимодалка):** последнее user-сообщение получает `content` как массив `parts`:
  `{"type":"text","text":...}` + `{"type":"image_url","image_url":{"url":"data:<mime>;base64,<data>"}}`.
  Если вложения есть, но нет user-сообщения — добавляется отдельное user-сообщение с картинками.
- **Стриминг (SSE):** читаем построчно в отдельном потоке, основной цикл ждёт через
  `recv_timeout(READ_TIMEOUT=120s)` — это таймаут «первого токена»: живой длинный стрим НЕ
  обрезается, но зависший движок детектится. Парсим `data: {json}` →
  `ChatCompletionEvent` (`choices[].delta.content`, `choices[].delta.reasoning_content`,
  `finish_reason`, `tokens_evaluated`, `timings`).
- При обрыве до первого токена запрос повторяется один раз; после появления токена или по
  команде «Стоп» повтор запрещён.
- При обрыве формируется один блок `STREAM_DIAGNOSTIC`: HTTP status/version и безопасные
  заголовки (`Content-Type`, `Content-Length`, `Transfer-Encoding`, `Connection`), тип и цепочка
  ошибки чтения, тайминги первого события/токена, число строк/байт/токенов, состояние процесса
  и `/health`, ограниченный хвост stderr. Полный блок не создаётся для успешных запросов.
- В телеметрию `llm_stream_error` уходят только числовые метрики, состояние процесса и
  перечисленные HTTP-заголовки; API-ключ, промпт, ответ и содержимое stderr не отправляются.
- Дифф стрима (`compute_stream_diff`) → колбэк `stream_cb` (кусочки текста в UI).
- `reasoning_content` (думатель) собирается отдельно, НЕ идёт в итоговый `content` и не сохраняется
  в историю.
- Ошибки генерации (`err_details`): причина + пик памяти (`MemReport`: RSS сервера/аппа/VRAM) +
  хвост `llama_server.log`. Пишутся в `startup_log` и телеметрию `llm_error`.
- Отмена: `cancel_flag` (`AtomicBool`) — при установке стрим прерывается (`stop_reason="CANCELLED"`).
- Память под контролем: `MemSampler::start(server_pid)` + `MemGuard` замеряют пики RAM/VRAM всё
  время генерации (RAII-гвард финиширует на любом пути выхода).

---

## 5. Контекст: оценка ДО старта и точная обрезка ПОСЛЕ

`--ctx-size` **фиксируется при старте процесса** и не меняется (runtime-resize через `/props`
недоступен в llama.cpp). Поэтому:

### 5.1 Эвристика worst-case ДО старта (`orchestrator/mod.rs:287`, `estimate_chars_per_token:237`)
```rust
let (worst_system_prompt, worst_has_tools) = build_worst_agent_prompt(...); // самый тяжёлый агент графа
let total_chars = worst_system_prompt.len() + history_chars + user_text.len();
let image_tokens = attachments.len() as u32 * 2048;
let chars_per_token = if cyrillic_heavy { 2 } else { 3 };   // ~2 симв/токен кириллица, ~3 латиница
let tool_budget = if worst_has_tools { TOOL_WORKING_BUDGET(1024) } else { 0 };
let estimated_tokens = (total_chars / chars_per_token)
                       + image_tokens + TOKEN_ESTIMATE_RESERVE(512) + tool_budget;
let engine_ctx_limit = (estimated_tokens + max_gen_tokens + 128).min(context_size).max(2048);
```
- `TOKEN_ESTIMATE_RESERVE = 512`, `TOOL_WORKING_BUDGET = 1024` (`orchestrator/consts.rs:9-10`).
- `+128` — запас; clamp к `[2048, context_size]` (где `context_size` из конфига, default 24576).
- `worst_system_prompt`: системный промпт самого «тяжёлого» `llm_worker`-агента графа (включая
  схемы инструментов и блок `[КРИТИЧЕСКОЕ ОГРАНИЧЕНИЕ]`) — SSOT с `run_agent_node`.
- Токенизатор недоступен до старта сервера → грубая эвристика; точная подгонка — после.

### 5.2 Точная обрезка ПОСЛЕ старта (`/tokenize`)
В `run_agent_node` (`orchestrator/mod.rs:759`) на каждой итерации цикл:
```rust
loop {
    let current_tokens = engine.get_tokens_count(&ctx.llm_messages, format_type)?;
    let ideal_ctx = (current_tokens + max_gen_tokens + 128).min(engine.global_ctx_limit);
    if current_tokens + max_gen_tokens <= ideal_ctx || msgs.len() <= 2 { break; }
    // удаляем старейшее сообщение (не трогая [СЖАТАЯ ИСТОРИЯ], не рвя пары tool/assistant)
    ctx.llm_messages.remove(oldest_idx);
}
```
Плюс **компакция** (`compaction::compact_llm_messages`) по реальному бюджету
`global_ctx_limit - max_gen_tokens`, слои по возрастанию агрессивности:
1. head/tail pruning крупных результатов инструментов (`[РЕЗУЛЬТАТ ИНСТРУМЕНТА]`, >1500 симв).
2. LLM-саммаризация старой истории → узел `[СЖАТАЯ ИСТОРИЯ]` (если диапазон ≥ 6 сообщений).
3. иначе усечение head/tail старых сообщений.
4. last-resort: жёсткий дроп старейших (не трогая `[СЖАТАЯ ИСТОРИЯ]`).
- `is_context_overflow(err)` (`compaction.rs:238`): детект по тексту ошибки
  (`"context length"|"exceed"|"n_ctx"|"too long"|"http 400"|"kv cache"|"sliding window"`).
  При переполнении — `truncate_largest` (head/tail самого крупного) и повтор генерации.
- **Правило:** тихих операций нет — каждое удаление/сжатие логируется (что удалено, сколько символов).

---

## 6. GPU / CUDA: детекция и выбор

`infra/gpu_detector.rs`:
- `detect_gpu()` → `GpuInfo` через NVML (`Nvml::init` → `device_by_index(0)`) → fallback `nvidia-smi`.
- Поля: `has_nvidia`, `cuda_major/minor` (из `sys_cuda_driver_version`), `compute_major/minor`
  (из `cuda_compute_capability`), `gpu_name`.
- `MIN_CUDA_MAJOR = 12`: драйвер должен поддерживать CUDA 12+ (exe слинкован с `cublas64_12/13.dll`).
- `MIN_COMPUTE_MAJOR_CUDA13 = 7`, `MIN_COMPUTE_MINOR_CUDA13 = 5`: **нижняя граница
  архитектуры для сборки `cuda-13.x`** — `sm_75` (Turing). Обоснование —
  `ggml/src/ggml-cuda/CMakeLists.txt`:
  ```cmake
  if (CUDAToolkit_VERSION VERSION_LESS "13")
      list(APPEND CMAKE_CUDA_ARCHITECTURES 50-virtual 61-virtual 70-virtual)
  endif ()
  list(APPEND CMAKE_CUDA_ARCHITECTURES 75-virtual 80-virtual 86-real 89-real 90-virtual 120a-real)
  ```
  Тулкит CUDA 13 выпилил Maxwell (sm_5x), Pascal (sm_6x) и Volta (sm_70).
  Релизные бинари собираются с этим дефолтным набором (`.github/workflows/release.yml`:
  "no CMAKE_CUDA_ARCHITECTURES: use the broad default arch set"), поэтому на карте
  младше Turing `cuda-13.x` падает с `CUDA_ERROR_NO_KERNEL_IMAGE`, а `cuda-12.4`
  там работает (в ней есть `61-virtual` PTX).
  ⚠️ Проверять **только major нельзя**: major 7 / minor 0 — это Volta (sm_70), она
  не поддержана; major 7 / minor 5 — Turing (sm_75), поддержана.
- `required_cuda_gen(info) -> Option<CudaGen>`:
  - нет NVIDIA / драйвер < CUDA 12 → `None` (CPU);
  - **Blackwell** (compute_major ≥ 12, т.е. RTX 50xx) → **Cuda13** (сборка
    cuda-12.x не содержит ядер sm_120);
  - **архитектура младше Turing** (`!cuda13_arch_supported(info)`) → **Cuda12**
    независимо от свежести драйвера. Это правило приоритетнее версии драйвера:
    R580+ одинаково дружит и с Maxwell, и с Blackwell, но бинарник cuda-13.x без
    ядер sm_61 на GTX 10xx гарантированно падает;
  - **архитектура не определилась** (compute_major == 0) → **Cuda12**: он совместим
    с заметно более широким набором карт, при неопределённости берём его, а не гадаем;
  - драйвер CUDA 13+ (R580+) → **Cuda13** (содержит ядра sm_75..sm_120, включая RTX 40xx);
  - драйвер CUDA 12.x → **Cuda12**.
- `cuda13_arch_supported(info)`, `compute_label(info)` (`sm_61` / `не определена`).
- `llamacpp_installer::arch_incompatibility(gpu, variant) -> Option<(нужный, установленный)>` —
  единственный ответ на вопрос «упал ли движок из-за отсутствия ядер»: нужны оба
  факта (архитектура карты + состав сборки). Жёстко ловит обе стороны:
  `cuda-12.x` на Blackwell и `cuda-13.x` на Maxwell/Pascal/Volta.
- `llamacpp_installer::cuda_gen_variant(CudaGen)` — единственный маппинг
  `CudaGen` → id варианта в папке `backends/<source>/<variant>/`; его используют
  и авто-подбор, и подписи в UI, и тексты диагностики.
- `requires_driver_update` — драйвер CUDA 11.x (нужен апгрейд).

Маппинг в вариант (`llamacpp_installer::select_variant`):
`Cuda13 → cuda-13.3`, `Cuda12 → cuda-12.4`, `None → cpu`.

В `LlamaEngine::new`: `use_gpu = installed_family.is_gpu()`; `gpu_layers = use_gpu ? 999 : 0`.
Т.е. `-ngl 999` (весь оффлоуд в VRAM) для GPU-бекендов, `-ngl 0` для CPU.

После старта — NVML-проверка дельты VRAM (§3.5): если GPU-намерение, но VRAM не выросла →
явная причина из `diagnose_cuda_fallback` (поиск паттернов в `llama_server.log`).

---

## 7. Модели и управление ими (config-driven, НЕТ hot-swap)

### 7.1 Конфиг движка (`%APPDATA%/llama-engine/app_config.json`)
Движковые ключи владеются ПЛАГИНОМ в его собственном каталоге, общем для всех
хостов плагина (`ko_json_store::roots::plugin_dir("llama-engine")`;
`set_app_data_dir_name` — deprecated no-op). Хост-приложение читает их ОВЕРЛЕЕМ
поверх своего конфига (`infra::config::load_config` → `apply_engine_keys`) тем же
лоадером, что и панель моделей, а при сохранении хостового файла — вырезает
(`ENGINE_KEYS`, см. инцидент 25.10.26: расхождение списков моделей чат↔панель).
Запись движковых ключей хостом — только через `set_engine_last_model` /
`set_engine_dirs`. См. §8 за полной схемой. Ключевое для движка:
- `models: Vec<String>` — пути к `.gguf`; `last_model: Option<String>` — выбранная.
- `model_params: HashMap<String, ModelParams>` — сэмплинг **по модели** (ключ = путь файла).
- `model_meta: HashMap<String, ModelMeta>` (`uncen`/`vision`/`audio`).
- `mmproj_files: HashMap<String, String>` — путь к мультимодальному проектору per-модель.
- `llamacpp_dir: Option<String>` — переопределение папки движка.
- `engine_source: Option<String>` — источник бинарей: `"ggml-org"` (дефолт) / `"beellama"` (KVarN).
- `engine_variant: Option<String>` — `"auto"`/`cpu`/`cuda-12.4`/`cuda-13.3`/`vulkan`/`hip-radeon`.

### 7.2 Загрузка / переключение
- Загрузка модели = запуск сервера с `-m <model_path>` (`LlamaEngine::new`, `llm.rs:316`).
- **Нет hot-swap.** `LlamaEngine` создаётся **заново под каждый `chat_request`**
  (`run_chat` → `LlamaEngine::new`). Переключение модели = следующий запрос с другим `model_path`
  → новый `llama-server.exe` с новым `-m`, старый убивается через `Drop`. Это и есть причина,
  почему `Drop`/Job Object критичны.
- GGUF-валидация ДО spawn (`validate_gguf`) — понятная ошибка вместо падения сервера.
- Добавление/удаление/скачивание моделей: `api/models.rs` (`add_model`, `remove_model`,
  `auto_download_default_model` из `models_catalog.json`, `ensure_mmproj`).

### 7.3 Сэмплинг: откуда берётся
1. `chat_request` получает `ModelParams` с фронта.
2. `get_model_params` (`api/models.rs:140`): берёт сохранённые per-модель params, либо
   `ModelParams::default()`, затем **накладывает вшитые в GGUF дефолты**
   (`tokenizer.ggml.temp/top_k/top_p/min_p/repetition_penalty` через `extract_*_from_gguf`) —
   они являются истиной, кешируется обратно в конфиг.
3. Все параметры уходят **per-request** в `/v1/chat/completions` (§4.3). Своих сэмплеров на Rust нет.
4. Опц. пресеты из `sampling_presets.json` (`load_sampling_presets`).

---

## 8. Схема конфига (app_config.json)

### `ModelParams` (`infra/config.rs:8`)
| Поле | Тип | Default | Примечание |
|------|-----|---------|-----------|
| `temperature` | f32 | 0.5 | кламп `.max(0.01)` |
| `top_k` | u32 | 40 | |
| `top_p` | f32 | 0.95 | |
| `min_p` | f32 | 0.1 | кламп `.max(0.0)` |
| `repetition_penalty` | f32 | 1.15 | кламп `.max(1.0)` |
| `presence_penalty` | f32 | 0.0 | |
| `dry_multiplier` | f32 | 0.8 | |
| `dry_base` | f32 | 1.75 | |
| `dry_allowed_length` | i32 | 2 | |
| `dry_penalty_last_n` | i32 | 256 | |
| `xtc_probability` | f32 | 0.0 | |
| `xtc_threshold` | f32 | 0.1 | |

### `AppConfig` (основные поля, `infra/config.rs:53`)
| Поле | Тип | Default | Куда идёт |
|------|-----|---------|-----------|
| `models` | Vec<String> | `[]` | список моделей |
| `last_model` | Option<String> | None | выбранная модель |
| `model_params` | HashMap<String,ModelParams> | `{}` | per-модель сэмплинг |
| `context_size` | u32 | 24576 | верхняя граница `--ctx-size` (clamp) |
| `max_gen_tokens` | u32 | 4096 | → `n_predict` |
| `reasoning_budget` | u32 | 1500 | → `--reasoning-budget` (0 = выкл) |
| `kv_quant_keys` | bool | false | deprecated (UI убран) → всегда false; `--cache-type-k` только из source.runtime |
| `kv_quant_values` | bool | false | deprecated (UI убран) → всегда false; `--cache-type-v` только из source.runtime |
| `theme` | String | "dark" | |
| `prompt_format` | String | "Auto" | всегда Auto (UI-селект убран, санитайз в load_config) |
| `confidence_threshold` | f32 | 0.8 | |
| `mmproj_files` | HashMap<String,String> | `{}` | per-модель mmproj |
| `model_meta` | HashMap<String,ModelMeta> | `{}` | uncen/vision/audio |
| `llamacpp_dir` | Option<String> | None | переопределение папки движка |
| `engine_source` | Option<String> | None | источник бинарей: "ggml-org"(дефолт)/"beellama" (KVarN)/"prismml" (Bonsai) |
| `engine_variant` | Option<String> | None | "auto"/cpu/cuda-12.4/cuda-13.3/vulkan/hip-radeon |
| `allow_error_reports` | bool | true | |
| `chat_font_scale` | f32 | 1.0 | |

Два файла, один SSOT на ключ:
- **Движковые ключи** (`models`, `last_model`, `models_dir`, `model_params`,
  `mmproj_files`, `model_meta`, `llamacpp_dir`, `engine_source`, `engine_variant`) —
  `%APPDATA%/llama-engine/app_config.json` (владелец — плагин, общий для всех хостов).
- **Хостовые ключи** (`theme`, `tabs`, `context_size`, `max_gen_tokens`,
  `reasoning_budget`, `translator_*`, `data_dir`, …) —
  `%APPDATA%/com.kingorch.app/app_config.json` (`APP_DATA_DIR_NAME`).
- Стык в хосте: `load_config`/`load_config_early` накатывают движковые ключи
  оверлеем из файла плагина; `save_config_file` вырезает их из хостового файла
  (вторая копия правды невозможна). Чтение ДО Tauri: `load_config_early()`
  (через `APPDATA`/`XDG_CONFIG_HOME`/`HOME`); в рантайме — `load_config(app)` /
  `save_config(app, cfg)`; движковые записи — `set_engine_last_model` /
  `set_engine_dirs` (внутри — `engine::config::save_config` плагина).

---

## 9. Модульная структура (3-слойная архитектура)

```
api/  (Tauri-команды — тонкий слой, делегирует)
 ├─ chat.rs          chat_request → domain::run_chat (model_path, model_params, engine_dir, callbacks)
 ├─ models.rs        add_model, get/set/reset_model_params (GGUF-aware), get_mmproj_path, capabilities
  ├─ llamacpp.rs      engine_dir(), get_engine_status, install_llamacpp, set_engine_variant, set_engine_source, set_engine_dir
  └─ config.rs        set_last_model …

domain/  (бизнес-логика)
 └─ orchestrator/
     ├─ mod.rs        run_chat() — worst-case ctx-оценка, spawn LlamaEngine, run_agent_node
     │                run_agent_node() — per-iter /tokenize-триминг, compaction, генерация
     ├─ consts.rs     TOKEN_ESTIMATE_RESERVE=512, TOOL_WORKING_BUDGET=1024
     └─ compaction.rs is_context_overflow(), compact_llm_messages() (4 слоя)

infra/  (обёртка над LLM — ТОЛЬКО через дверь infra/mod.rs)
 ├─ llm.rs            LlamaEngine { spawn, Drop, /health, /tokenize, /v1/chat/completions, сэмплинг, grammar }
 ├─ llm_types.rs      LlmMessage, GenerationResult, LlmMetrics, GrammarSpec, PromptFormat
 ├─ llm_gguf.rs       validate_gguf, extract_f32/u32/string_from_gguf
 ├─ llm_multimodal.rs generate_chat_multimodal (image parts → /v1/chat/completions)
  ├─ llamacpp_installer.rs  variant_dir(dir, source, variant), resolve_variant, resolve_source, install(), find_engine_asset, EngineFamily
 ├─ startup_diagnosis.rs   diagnose_startup_failure() — перевод ошибок ПАДЕНИЯ НА СТАРТЕ
 │                          на человеческий язык (см. §3.4а); diagnose_shared_kv_before_spawn()
 │                          — та же проверка по метаданным GGUF ДО запуска; build_context(), log_full_trace()
  ├─ sources.rs       engine_sources.json (реестр SourceSpec: repo/asset_prefix/prefer_stable/runtime KVarN)
  ├─ gpu_detector.rs   detect_gpu, required_cuda_gen, CudaGen
  ├─ config.rs         AppConfig, EngineConfig, load/save, sampling_presets, catalog
  ├─ process_util.rs   register/unregister PID, kill_process_tree, Windows Job Object
  ├─ mem_profiler.rs   MemSampler/MemGuard — пики RAM+VRAM во время генерации
  └─ mod.rs            pub use … (фасад)
```

**Ключевые точки расширения при переносе:**
- `LlamaEngine` — единственный, кто знает про spawn/HTTP. Остальной код общается с ним через
  методы `new/new_with_mmproj`, `generate_chat`, `get_tokens_count`, `Drop`.
- Резолв порта/ключа/варианта/путей вынесен в чистые функции (`resolve_variant`, `default_dir`,
  `Prng`) — легко адаптировать под не-Tauri среду (просто дайте `engine_dir` и `model_path`).

---

## 10. Чек-лист переноса в другой проект

1. **Не линкуй llama.cpp.** Только `reqwest` (HTTP), `zip`+`sha2` (установка), `nvml-wrapper`
   (VRAM, опц.), `sysinfo` (RAM). Никаких `llama-cpp-sys-2`/`llama-cpp-2` в `Cargo.toml`.
2. **Скачай движок** как `llama-server.exe` из выбранного источника (`ggml-org/llama.cpp`
   или форк, см. §1а), вариант под GPU, положи в `<exe>/<engine_dir>/backends/<source>/<variant>/`.
   Докачай CUDA-рантайм отдельно.
3. **Spawn** с `-m`, `--host 127.0.0.1`, `--port` (рандом bind-проверенный), `--api-key` (рандом
   32-hex), `--ctx-size`, `-t`, `-ngl 999|0`, `--flash-attn on`, `--no-webui`, `--log-file`.
4. **Жди `/health`** (≤120s). Имей fallback на случай неподдерживаемых флагов.
5. **Чисть через Drop + Job Object + kill_active_engines** — иначе серверы-зомби висят в VRAM.
6. **Контекст:** оцени worst-case `--ctx-size` эвристикой ДО старта (~3 симв/токен, +резервы);
   точно подгоняй историю ПОСЛЕ старта через `/tokenize` (не рви пары tool/assistant, не трогай
   сжатую историю).
7. **Сэмплинг** — per-request в `/v1/chat/completions`; бери дефолты из GGUF-метаданных, клампь.
8. **Промпт рендерит сервер** (jinja `chat_template` из GGUF) — шли только `messages[]`.
9. **GPU:** детект через NVML/nvidia-smi; `-ngl 999` для GPU-бекенда; проверяй дельту VRAM после
   старта и объясняй причину CPU-фолбэка (не молчи).
10. **Пути через `current_exe().parent()`**, а не `app.executable_dir()` (cwd в dev отличается).

---

## 11. Подводные камни (реально встречались)

- **Тихий CPU-фолбэк** без CUDA-рантайма (`cublas64_*.dll`). Решение: preflight-проверка DLL +
  диагностика из лога сервера.
- **Переполнение контекста** (runtime-resize недоступен). Решение: эвристика ДО + `/tokenize`-триминг ПОСЛЕ.
- **Висящие процессы** при насильственном закрытии. Решение: Windows Job Object `KILL_ON_JOB_CLOSE`.
- **Конфликт портов** при параллельных запусках. Решение: рандом + bind-проверка.
- **Уязвимость localhost** без api-key. Решение: рандомный 32-hex ключ в `Bearer`.
- **Битый GGUF** роняет сервер с непонятным логом. Решение: `validate_gguf` ДО spawn.
- **Путаница версий CUDA** (cuda-13.3 vs 13.7, Blackwell нужен только 13.x). Решение: family-based
  поиск ассета (`find_asset_by_family`) — мажорная версия, а не точная строка.
- **Старая версия движка не знает `--reasoning-*`** → падает при старте. Решение: fallback-перезапуск без флагов.
- **`cuda-13.x` на карте младше Turing** (инцидент у юзера с 4 ГБ GPU): авто-подбор считал
  только версию драйвера (`cuda_major >= 13 → Cuda13`) и игнорировал архитектуру, поэтому ставил
  сборку без ядер для карты → `CUDA error: no kernel image is available for execution on the
  device`, выход `0xC0000409` (`-1073740791`). Драйвер тут ни при чём. Решение: нижняя граница
  `MIN_COMPUTE_MAJOR_CUDA13`/`MIN_COMPUTE_MINOR_CUDA13` в `required_cuda_gen` (§6) + проверка
  только major **не проходит** (Volta sm_70 не поддержана, Turing sm_75 — да).
- **BeeLlama + модель с общим KV-кэшем (Cross-Layer Attention)**: `--kv-tail-tokens 1024`
  из `source.runtime` неработает на слоях, чей KV общий с предыдущими
  (`*.attention.shared_kv_layers > 0`) → `KV tail has no execution descriptor for layer N`, код 1.
  Gemma 4 E4B: 42 слоя, 18 общих → падение на 24. Gemma 4 12B: 0 общих → работает. Модель
  исправна, несовместим движок. Решение: диагностика §3.4а — превентивная по метаданным
  GGUF (до spawn, без двух неудачных попыток) и реактивная по тексту лога; обе предлагают
  сменить источник на `ggml-org`.
- **Ложная атрибуция к SWA.** Первая версия диагностики объясняла падение скользящим окном
  внимания — это **неверно**: ключи `sliding_window`/`key_length_swa` есть у обеих моделей
  (и у падающей, и у работающей). Регрессия закреплена тестом
  `kv_tail_message_never_mentions_sliding_window`. Вывод: объяснять падение только по
  **различающимся** метаданным, а не по тем, что есть у обеих моделей.
- **Диагностика молчала именно там, где нужнее всего**: `diagnose_cuda_fallback` вызывается
  только ПОСЛЕ успешного `/health`, а падения при старте случаются ДО health — юзер получал
  сырой хвост ggml-лога и думал, что сломалась программа. Решение: отдельный классификатор
  §3.4а, читающий stderr **и** log-файл.
- **Мелкая ловушка:** не-CUDA варианты (`vulkan`, `hip-radeon`) не проходят через проверку
  архитектуры CUDA, и `EngineFamily` содержит больше ветвей, чем `CudaGen` — match по семейству
  без `_ =>` не компилируется. Проверку совместимости держим в
  `llamacpp_installer::arch_incompatibility`, а не инлайном в `llm.rs`.

---

## 12. Исправление устаревших упоминаний (история)

Ранее в доках проекта King Orch (и в `AGENTS.md`) фигурировал крейт `llama-cpp-sys-2` / `llama-cpp-2`
с утверждением, что приложение собирает CUDA через nvcc (~20 мин) и запрещено удалять
`target/debug/build/llama-cpp-sys-2-*`. **Это больше не соответствует реальности:**
- `Cargo.toml` проекта НЕ содержит `llama-cpp-sys-2`/`llama-cpp-2` (проверено по `Cargo.lock`).
- Инференс исключительно через внешний `llama-server.exe` (subprocess + HTTP), никакой нативной
  линковки llama.cpp в main-бинарь.
- Соответствующие правила `AGENTS.md` (§1 «Запрет на удаление кеша C++», п. «Модели: llama.cpp
  через llama-cpp-2», §«CUDA/nvcc несовместима с llama-cpp-sys-2») и `docs/ARCHITECTURE.md`
  (`llm.rs | Работа с llama-cpp-2`) **исправлены** на актуальное описание внешнего `llama-server.exe`.
