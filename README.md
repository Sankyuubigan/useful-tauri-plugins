# my-tauri-plugins

Репозиторий переиспользуемых **Tauri-плагинов** (Rust-крейт + npm-пакет + Web Component).
Один плагин = один workspace-member = один модуль «улучшил раз — обновилось везде».

## Плагины

| Плагин | Идентификатор | Назначение | Web Component |
|--------|---------------|------------|---------------|
| `tauri-plugin-logs` | `logs` | Единый `log::Log`: файл лога + `test/last_logs.txt` + UI-вкладка + опц. аналитика | `<logs-panel>` |
| `tauri-plugin-about-updates` | `about-updates` | Версия, проверка обновлений, история релизов GitHub, откат | `<about-updates-panel>` |
| `tauri-plugin-speech` | `speech` | TTS/STT (движок CrispASR, голоса, модели) | да |

`tauri-build-toolkit/` — отдельный Node-CLI пакет для сборки/препа/релиза хост-проектов
(не плагин; дока — `tauri-build-toolkit/docs/README.md`).

## Стандарт

Полный инженерный референс (структура плагина, permissions, оба JS-канала, интеграция,
чек-лист, подводные камни) — **см. [`PLUGIN_STANDARD.md`](PLUGIN_STANDARD.md)**.

## Ключевые правила (SSOT)

1. **Один источник JS-кода плагина — `guest-js/`.** Правки руками — только там.
2. **Два производных артефакта** (коммитятся, руками НЕ правятся, генерируются сборкой):
   - `dist-js/` — npm-канал для хостов с бандлером (`npm run build`, tsc);
   - `api-iife.js` — vanilla-канал для хостов без npm/бандлера (`npm run build:global`, esbuild),
     вшивается в бинарник хоста через `global_api_script_path` в `build.rs` + `withGlobalTauri`.
3. **Изменил плагин → пересобрал артефакты → пересобрал хост.** Анти-дрейф: никогда не править
   артефакты руками и не копировать их в хост.
4. Код плагинов **можно менять при жёсткой необходимости** (баг, новая фича, адаптация под хост) —
   но только **внутри плагина**, не форк-копией в хосте.
5. Команды: `COMMANDS` (build.rs) = имя функции = `allow-*` = работы в `[default]` — одно место
   правки на 3 места.

## Сборка артефактов плагина

В папке конкретного плагина (пример `tauri-plugin-logs`):

```bash
npm install              # один раз (devDeps: typescript, esbuild)
npm run build            # → dist-js/  (npm-канал)
npm run build:global     # → api-iife.js (vanilla-канал)
```

Затем закоммитить оба артефакта. Cargo-сборка хоста не требует node — она берёт уже закоммиченный
JS из корня плагина.

## Быстрый старт подключения плагина к хосту

```toml
# src-tauri/Cargo.toml
[dependencies]
tauri-plugin-logs = { path = "../../my-tauri-plugins/tauri-plugin-logs" }
```

```json
// src-tauri/tauri.conf.json
{ "app": { "withGlobalTauri": true }, "plugins": { "logs": { "log_file_name": "app.log", "last_logs": true } } }
```

```json
// src-tauri/capabilities/default.json
{ "permissions": ["core:default", "logs:default"] }
```

```html
<!-- vanilla-хост: ui/index.html -->
<logs-panel></logs-panel>
```

или npm-хост: `import "@my-tauri-plugins/plugin-logs";`

Подробно: [PLUGIN_STANDARD.md](PLUGIN_STANDARD.md).