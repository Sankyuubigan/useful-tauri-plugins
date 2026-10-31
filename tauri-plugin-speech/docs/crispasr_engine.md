# Движок CrispASR: интеграция как отдельный процесс (crispasr.exe)

> **Назначение доки:** воспроизводимый инженерный референс по тому, как движок TTS/ASR
> **CrispASR** подключается и работает в связке с хостом (эталонный хост — SpeechLab).
> Написано так, чтобы по этой доке архитектуру можно было один-в-один повторить в любом
> другом Rust/Tauri (или не-Taurи) десктоп-проекте. Это прямой аналог
> `../../tauri-plugin-llama-engine/docs/llama_cpp_engine.md`, но для CrispASR.

> **Ключевой принцип (он же §6.7 в `D:\Projects\docusaurus-starter\docs\Sega Mega Note\Моя картотека\software\настройки\global_ai_docs\desktop_rust_tauri_rules.md`):** приложение
> **НЕ линкует CrispASR нативно** (никаких `crispasr-sys` / `crispasr` крейтов в
> `Cargo.toml`, никакой сборки C++/GGML из source через cmake в `build.rs`). Инференс
> идёт через **отдельный процесс `crispasr.exe`** (полный релиз CrispASR) по **HTTP на
> localhost**. Это даёт: отсутствие 20-минутной CUDA/GGML-сборки в дереве зависимостей,
> graceful-выгрузку моделей из VRAM через `Drop`/Job Object, и возможность держать
> несколько бекендов одновременно без конфликтов DLL.

---

## 0. Архитектурная сводка

```
┌──────────────────────────────┐         HTTP (localhost)          ┌──────────────────────────────┐
│  SpeechLab (Rust/Tauri exe)  │  GET  /health                    │   crispasr.exe                │
│                              │  POST /v1/audio/speech  ────────▶ │   (полный релиз CrispASR)      │
│  modules::tts::TtsEngine     │  ◀──── WAV 24kHz mono (bytes) ── │                              │
│   - spawn subprocess         │                                   │   --server                   │
│   - random port + api-key    │                                   │   --backend qwen3-tts         │
│   - Drop → kill дерева       │                                   │   -m <model.gguf>            │
│   - HTTP client (reqwest)    │                                   │   --codec-model <codec.gguf> │
└──────────────────────────────┘                                   └──────────────────────────────┘
```

**Что делает Rust-сторона сама:** запуск процесса, генерация port/api-key, HTTP-клиент,
ожидание готовности (`/health`), отправка текста на синтез (`/v1/audio/speech`),
приём WAV-байтов, kill процесса при завершении. **Что делает сервер:** загрузка GGUF,
сам TTS-инференс, отдача PCM/WAV.

---

## 1. Получение бинаря движка (готовый prebuilt, НЕ сборка из source)

Движок **не собирается** и **не бандлится** из исходников. Он скачивается **пользователем**
из GitHub Releases `CrispStrobe/CrispASR` (как `llama-server.exe` у llama.cpp):

- Endpoint релизов: `https://github.com/CrispStrobe/CrispASR/releases` (берём `latest`).
- CrispASR поставляет **готовые бинари для всех платформ**, включая Windows:
  - **Windows CPU:** `crispasr-windows-x86_64-cpu.zip` (AVX2+FMA, Haswell 2013+),
    либо `crispasr-windows-x86_64-cpu-legacy.zip` (SSE2, для старых CPU до AVX2).
  - **Windows CUDA:** `crispasr-windows-x86_64-cuda.zip` (самодостаточный, несёт 3 DLL
    NVIDIA) + раздельные `crispasr-windows-x86_64-cuda-non-cuda.zip` и сами DLL
    (`cudart64_*.dll`, `cublas64_*.dll`, `cublasLt64_*.dll`) — чтобы не качать рантайм
    при каждом апгрейде.
  - **Linux:** `crispasr-linux-x86_64.tar.gz` (CPU), `-avx512`, `-cuda`, `-cuda13`,
    `-hip`, `-vulkan` (аналог вариантов llama.cpp).
- Никакой cmake / MSVC / CUDA-toolkit для сборки не нужно. Просто распакуй zip/tar.gz —
  внутри `crispasr.exe` (Windows) / `crispasr` (Linux).

> **Важно:** это НЕ Tauri sidecar (`externalBin` в `tauri.conf.json` отсутствует). Бинарь
> ищется в рантайме по пути (поле в настройках UI + резолв относительно `current_exe().parent()`).

### Раскладка папок и обнаружение бинаря

```
<exe_dir>/
└── crispasr/                         ← default_dir = current_exe().parent().join("crispasr")
    └── crispasr.exe                  ← основной бинарь (Windows)
```

Резолв пути к серверу (аналог `LlamaEngine`):
- если пользователь задал `engine_exe` в UI (`set_tts_paths`) → используется он;
- иначе `std::env::current_exe().parent().join("crispasr").join("crispasr.exe")`
  (**НЕ** `app.path().executable_dir()` — см. правило о cwd: §2 в `desktop_rust_tauri_rules.md` глобальной доки).
- Перед spawn проверяем `exists()`; если нет — явная ошибка («откройте Настройки → укажите
  путь к crispasr.exe»), а не молчаливый фолбэк.

---

## 2. Запуск процесса

### 2.1 Аргументы командной строки (TTS, backend qwen3-tts)

```
crispasr.exe
  --server
  --backend qwen3-tts-1.7b-base  # для 1.7B Base; варианты: qwen3-tts-0.6b-base, qwen3-tts-customvoice
  -m <model.gguf>               # qwen3-tts-12hz-1.7b-base-q8_0.gguf
  --codec-model <codec.gguf>    # qwen3-tts-tokenizer-12hz.gguf (обязателен для qwen3-tts)
  --host 127.0.0.1
  --port <random 17800..19300>
  --api-key <32 hex>
  [--voice-dir ./voices]        # опц., для именованных voice-паков
  [--no-watermark]              # опц., НЕ рекомендуется (EU AI Act: ответственность оператора)
```

- `--creation_flags(0x08000000)` (`CREATE_NO_WINDOW`) на Windows — без мелькания консоли
  (`#[cfg(windows)]` на `std::process::Command`).
- `stderr` сервера можно пайпить и логировать как `[crispasr] …` (туда идут ошибки
  загрузки GGUF / CUDA), но для Этапа 0 достаточно перехватывать через `/health` и HTTP-статусы.
- Порт: `PORT_MIN=17800`, `PORT_RANGE=1500`. Перебираем кандидат через `TcpListener::bind`
  (`bind("127.0.0.1:0")` → `local_addr().port()`), чтобы не было конфликта параллельных запусков.
- api-key: 32 шестнадцатеричных символа (ГПСЧ). Передаётся в заголовок
  `Authorization: Bearer <api_key>`. **Без ключа сервер на localhost уязвим** для любого
  локального процесса (как и у llama.cpp).

### 2.2 Ожидание готовности

- Цикл опроса `GET /health` (таймаут `HEALTH_TIMEOUT = 120s`) пока модель грузится в память.
  Ответ: `{"status":"ok","backend":"qwen3-tts"}`.
- Если `try_wait()` вернул код завершения → ошибка (показать stderr/лог сервера).
- Таймаут → `child.kill()` + ошибка.

### 2.3 Гарантированная зачистка (критично!)

- `impl Drop for TtsEngine`: убить процесс (`child.kill()` + `child.wait()`). Для полной
  гарантии — **Windows Job Object** с `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, как в
  `llama_cpp_engine.md §3.6` (`../../tauri-plugin-llama-engine/docs/llama_cpp_engine.md`;
  `process_util::assign_child_to_kill_job`). CrispASR при TTS
  не плодит дочерних процессов, но Job Object страхует от «зомби», висящих в VRAM при
  насильственном закрытии приложения (см. `desktop_rust_tauri_rules.md` §6.5 глобальной доки).
- Финальный сейфет: `kill_active_engines()` в обработчике `ExitRequested` в `main.rs`
  (докиляет по сохранённому PID, если `Drop` не успел).

---

## 3. HTTP-контракт с сервером

База: `http://127.0.0.1:<port>`. Auth: `Authorization: Bearer <api_key>`.
Клиент: `reqwest` (HTTP), как в llama.cpp варианте. **Никаких `crispasr`-крейтов.**

### 3.1 `GET /health`
Статус 200 = готов. Используется при старте (таймаут 120s).

### 3.2 `POST /v1/audio/speech` (OpenAI-совместимый TTS)
Тело (JSON):
```jsonc
{
  "input": "Привет, это тест синтеза речи.",   // обязательно
  "voice": "vivian",                            // опц. (для Base — имя/путь к voice-паку; можно опустить)
  "response_format": "wav",                     // wav (24kHz mono 16-bit) | pcm | f32 | mp3 | aac | opus
  "language": "ru",                             // опц., язык синтеза
  "speed": 1.0                                  // опц., 0.25..4.0
}
```
- Возвращает `audio/wav` (RIFF, 16-bit PCM int16, **24 kHz mono**) при `response_format=wav`.
- Ошибки: `400` (нет поля `input` / слишком длинный / неверный `speed`), `500` (пустой
  синтез, неизвестный voice), `503` (модель ещё грузится).
- Frontend получает **сырые байты WAV** → `Blob([bytes], {type:'audio/wav'})` →
  `URL.createObjectURL` → `<audio>.play()`.

### 3.3 Горячая смена модели (`POST /load`)
Сервер позволяет сменить GGUF на лету: `curl -F "model=path.gguf" http://localhost:8080/load`.
В нашей обёртке это эквивалентно перезапуску процесса с новым `-m` при смене пути в UI
(проще и надёжнее для Этапа 0).

---

## 4. Модели и управление ими

- **qwen3-tts (Base):** talker `cstr/qwen3-tts-1.7b-base-GGUF` (файл `qwen3-tts-12hz-1.7b-base-q8_0.gguf`,
  рекомендован q8_0) + codec `cstr/qwen3-tts-tokenizer-12hz-GGUF` (файл `qwen3-tts-tokenizer-12hz.gguf`).
  **Важно:** бэкенд НЕ `"qwen3-tts"`, а `"qwen3-tts-1.7b-base"` (и `-0.6b-base`). Русский
  поддерживается нативно (10 языков: `zh en ja ko de fr ru pt es it`).
- **qwen3-tts-customvoice:** именованные спикеры (`vivian`, `ryan`, …) через `--voice-dir`.
- Прочие TTS-бэкенды (cosyvoice3, omnivoice, kokoro, voxcpm2, bark, chatterbox, zonos) —
  меняются через `--backend` + соответствующие GGUF; архитектура остаётся той же.
- GGUF-файлы пользователь кладёт в папку вручную (Этап 0). Авто-скачивание
  (`-m auto` у CrispASR, либо рантайм-загрузчик как `llamacpp_installer.rs`) — отдельный этап.

> **Watermark (EU AI Act):** каждый ответ помечается watermark по умолчанию. Отключение
> только на уровне процесса (`--no-watermark` / `CRISPASR_NO_WATERMARK=1`) — тогда
> ответственность за AI-раскрытие ложится на оператора. Этап 0 оставляет watermark ВКЛ.

---

## 5. GPU / варианты бекенда

CrispASR сам выбирает лучший доступный бекенд (CUDA > Vulkan > CPU) при старте. Для GPU
нужен установленный драйвер/рантайм на хосте. Этап 0 = **CPU-zip** (AVX2 или legacy).
CUDA/Vulkan-варианты подключаются позже сменой скачанного zip и (при необходимости)
передачей `--gpu-backend`.

---

## 6. Чек-лист переноса в другой проект

1. **Не линкуй CrispASR.** Только `reqwest` (HTTP). Никаких `crispasr`/`crispasr-sys` в `Cargo.toml`.
2. **Скачай движок** как `crispasr.exe` из `CrispStrobe/CrispASR/releases/latest` (CPU/CUDA вариант),
   положи в `<exe>/crispasr/`.
3. **Spawn** с `--server --backend qwen3-tts -m <model> --codec-model <codec> --host 127.0.0.1
   --port <random> --api-key <32hex>` (`CREATE_NO_WINDOW` на Windows).
4. **Жди `/health`** (≤120s).
5. **Чисть через Drop + Job Object** — иначе серверы-зомби висят в VRAM.
6. **Синтез:** `POST /v1/audio/speech` с `{"input": text, "response_format":"wav"}` → WAV-байты.
7. **Пути через `current_exe().parent()`**, а не `app.executable_dir()`.

---

## 7. Подводные камни (реальные)

- **Молчаливый CPU-фолбэк / неверный вариант:** если скачать HIP/Vulkan zip без рантайма —
  бинарь может не запуститься. Для Этапа 0 берём CPU-zip (гарантированно работает).
- **Несовпадение модели и codec:** qwen3-tts **требует** `--codec-model`. Без него — ошибка
  загрузки. Проверяй оба пути до spawn, давай понятную ошибку.
- **Висящие процессы** при насильственном закрытии — решение: Windows Job Object `KILL_ON_JOB_CLOSE`.
- **Конфликт портов** — решение: рандом + bind-проверка (`TcpListener::bind 0`).
- **Уязвимость localhost** без api-key — решение: рандомный 32-hex ключ в `Bearer`.
- **Слишком длинный `input`** (>4096 симв по умолчанию) — сервер авто-режет по предложениям,
  но лимит можно поднять флагом `--tts-max-input-chars 0`.

---

## 8. Ссылки

- Релизы: https://github.com/CrispStrobe/CrispASR/releases
- Установка/варианты бинарей: `docs/install.md` (разделы «Windows CPU», «Windows CUDA», «Prebuilt Linux tarballs»)
- HTTP API (сервер): `docs/server.md` (раздел «Text-to-speech endpoint»)
- TTS-бэкенды и параметры: `docs/tts.md`
- EU AI Act (watermark/C2PA/клонирование голоса): `docs/eu-ai-act.md`
- Сравнение с llama.cpp: `COMPARISON.md`
