# tauri-plugin-speech

Rust + TS плагин для Tauri 2: управление движком **CrispASR** (TTS CosyVoice3 + STT Parakeet), скачивание бинарей и моделей, хранилище голосов для клонирования.

## Возможности

- **Движок:** установка/обновление prebuilt `crispasr.exe` по бэкендам (`cpu` / `cuda` / `cuda13` / …), выбор активного бэкенда, выгрузка (освобождение VRAM), проверка обновлений.
- **Модели:** список GGUF-пресетов (TTS и STT), скачивание недостающих, признаки установки, `RU`, тип голоса.
- **Голоса:** хранилище референсных голосов для клонирования (добавление/редактирование/удаление, аватар, прослушивание, шумоподавление RNNoise).
- **STT/TTS:** команды синтеза речи и распознавания.

## Готовый GUI (Web Components)

:warning: **У плагина есть готовые фронтенд-плашки.** Чтобы их использовать, у вас в приложении должен быть подключён npm-пакет `@my-tauri-plugins/plugin-speech` и включено хотя бы одно из этих элементов в разметку:

| Элемент | Что показывает |
|---------|----------------|
| `<speech-engine-panel>` | Тип бэкенда, статус движка, путь, установка/обновление, выгрузить VRAM |
| `<speech-models-panel>` | Установленные GGUF-модели, скачивание недостающих |
| `<speech-voice-storage>` | Хранилище голосов (добавить/изменить/удалить, прослушать) |

Импорт пакета (регистрирует все три компонента):

```ts
import '@my-tauri-plugins/plugin-speech'
```

В JSX/HTML:

```tsx
<speech-engine-panel />
<speech-models-panel />
<speech-voice-storage />
```

Компоненты рендерятся в Shadow DOM, темизируются через CSS-переменные хоста (`--text`, `--primary`, `--border`, `--bg-color`, `--session-hover`, `--font`). Фукнционально они полностью повторяют TS-API ниже (используют те же команды), так что их можно комбинировать с собственной вёрсткой.

## Подключение

### Rust (обязательно)

```toml
# Cargo.toml
tauri-plugin-speech = { path = "../../my-tauri-plugins/tauri-plugin-speech" }
```

```rust
// lib.rs
.plugin(tauri_plugin_speech::init())
```

`capabilities` — пакет «по-умолчанию» открывает все команды: `"speech:default"`.

### Frontend (опционально, для готового GUI или TS-API)

```jsonc
// package.json
"@my-tauri-plugins/plugin-speech": "file:../my-tauri-plugins/tauri-plugin-speech"
```

```ts
import { ttsSpeak, ttsListModels, ttsEngineBackends, ... } from '@my-tauri-plugins/plugin-speech'
```

## Команды (invoke)

Все команды вызываются как `plugin:speech|<name>`:

TTS: `tts_speak`, `tts_presets`, `tts_capabilities`, `tts_unload`, `tts_save_wav`, `tts_download_engine`, `tts_download_model`, `tts_engine_backends`, `tts_list_models`, `tts_list_voices`, `tts_add_voice`, `tts_delete_voice`, `tts_update_voice`, `tts_voice_avatar`, `tts_voice_audio`, `tts_voice_trimmed_audio`, `tts_check_update`, `tts_default_dirs`, `tts_get_settings`, `tts_save_settings`.

STT: `stt_get_settings`, `stt_save_settings`, `stt_start`, `stt_stop`, `stt_get_status`, `stt_inject_text`.

Прогресс скачиваний — событие `tts-download`: `{ kind: 'engine'|'model', name, downloaded, total }`.

## Настройки

`tts_settings.json` в app-config dir (поля `engine_dir`, `models_dir`, `engine_backend`, `preset`). Дефолтные пути можно задать в `tauri.conf.json`:

```jsonc
"plugins": {
  "speech": {
    "default_engine_dir": "D:\\nn\\crispasr",
    "default_models_dir": "D:\\nn\\models\\tts"
  }
}
```

Если `engine_dir`/`models_dir` пустые (нет настроек), берутся дефолты из конфига.

## Структура

```
guest-js/   — TS API + Web Components (исходник)
dist-js/    — собранный JS (main/types пакета)
src/        — Rust: commands, download, tts, stt, voices, audio
```