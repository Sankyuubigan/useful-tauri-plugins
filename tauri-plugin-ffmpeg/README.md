# tauri-plugin-ffmpeg

Переиспользуемый движок FFmpeg для Tauri-хостов (L2 в [архитектуре слоёв](../README.md)).

## Что делает

1. **Автоустановка**: `ensure_ffmpeg(dir, on_log)` — если `dir/bin/ffmpeg.exe` + `ffprobe.exe`
   отсутствуют, скачивает официальную сборку (BtbN FFmpeg-Builds, GitHub Releases latest,
   zip ~100 МБ) через `tauri-plugin-downloader` (гейты magic `PK` + min_size), распаковывает
   только нужные exe, верифицирует запуском `-version`. Гейт — глобальный Mutex + re-check:
   параллельные вызовы не качают архив дважды.
2. **Запуск без shell**: `run_tool(exe, args, timeout)` — subprocess с args-массивом, stdin
   закрыт (ffmpeg не зависает на «overwrite?»), таймаут с kill, хвост stderr в отчёте.
3. **Метаданные**: `probe_json(ffprobe, path)` — компактная проекция JSON ffprobe
   (длительность, размер, битрейт; потоки: кодек/разрешение/fps/каналы).

## Нейтральность

Плагин не знает ни одного продукта-потребителя: каталог установки (`dir: &Path`) передаёт
хост в каждый вызов. Внутри плагина нет путей/имён хоста.

## Подключение хостом

```toml
# src-tauri/Cargo.toml
tauri-plugin-ffmpeg = { path = "../../my-tauri-plugins/tauri-plugin-ffmpeg" }
```

```rust
// main.rs
.plugin(tauri_plugin_ffmpeg::init())
```

Нативные тула хоста зовут engine-функции напрямую (без AppHandle):

```rust
use tauri_plugin_ffmpeg::engine::{ensure_ffmpeg, ffmpeg_dir, ffmpeg_exe, ffprobe_exe, probe_json, run_tool};
```

## Тесты

```bash
cargo test -p tauri-plugin-ffmpeg
```
