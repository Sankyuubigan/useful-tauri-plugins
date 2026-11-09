# my-tauri-plugins

Репозиторий переиспользуемых **Tauri-плагинов** (Rust-крейт + npm-пакет + Web Component).
Один плагин = один workspace-member = один модуль «улучшил раз — обновилось везде».

## ⚖️ Нейтральность репозитория (главное правило)

Этот репозиторий — набор компонентов для **любых** хост-проектов. Ни один компонент
здесь не принадлежит продукту-потребителю и не знает о его существовании.

- **Запрещено** в названиях крейтов/плагинов, коде, комментариях и доках: имена
  продуктов-потребителей, их пути, env-переменные, конфиги и бизнес-привязки.
- **Имена — по capability, не по потребителю**: что делает (`web-tools`, `downloader`),
  а не кто использует.
- **Привязку к продукту делает сам продукт** — в СВОЁМ репозитории, тонким адаптером
  над generic-ядром (path-зависимость + обёртка).
- **Исторические нарушения кода/имён — исправлены 09.11.26** (верифицировано компилятором
  обоих репозиториев): крейты `ko-{code,excel,diag}-tools`/`ko-json-store` →
  `code-tools`/`excel-tools`/`diag-tools`/`json-store`; env `KING_ORCH_DATA_ROOT` →
  `JSON_STORE_DATA_ROOT` (json-store); env websearch `KING_ORCH_BINS_DIR` → цепочка
  `AGENT_TOOLS_DATA_DIR` → `%APPDATA%\agent-tools` → tmpdir; temp-префиксы
  `king_orch_dl_`/`king_orch_chrome_dl_` → `dl_`/`chrome_dl_` (downloader); env
  `KING_ORCH_SYSTEM1_THREADS`/`KING_ORCH_DATA_DIR` → `SYSTEM1_THREADS`/`SYSTEM1_DATA_DIR`
  (system1); док-упоминания в тронутых файлах нейтрализованы. **Новые имена — только
  нейтральные.** Остаток — функциональные привязки (константы API типа `com.example`
  идентификаторов/каталогов данных хоста, bench-пути тестов) — это дизайн API, не проза;
  вынос в параметры — отдельное решение.

## Плагины

| Плагин | Идентификатор | Назначение | Web Component |
|--------|---------------|------------|---------------|
| `tauri-plugin-logs` | `logs` | Единый `log::Log`: файл лога + `test/last_logs.txt` + UI-вкладка + опц. аналитика | `<logs-panel>` |
| `tauri-plugin-about-updates` | `about-updates` | Версия, проверка обновлений, история релизов GitHub, откат | `<about-updates-panel>` |
| `tauri-plugin-speech` | `speech` | TTS/STT (движок CrispASR, голоса, модели) | да |

`tauri-build-toolkit/` — отдельный Node-CLI пакет для сборки/препа/релиза хост-проектов
(не плагин; дока — `tauri-build-toolkit/docs/README.md`).

Тул-семейство (не плагины) живёт в папке **`tools/`**:

- `tools/agent-tools/` — отдельный **Rust-CLI** набор тулов для ИИ-агентов кодеров
  (не плагин, без GUI): агент вызывает инструмент сам, чтобы эскалировать задачу к
  более сильной модели через локальный шлюз 9Router, с жёстким лимитом вызовов и
  принудительной остановкой агента кодом возврата. Инструменты добавляются как
  новые модули `src/tools/*`. Дока — [`tools/agent-tools/README.md`](tools/agent-tools/README.md).
- `tools/code-tools/` — **ядро кодо-тулов агента (SSOT)**: файловые, поисковые и
  shell-тулы (плюс нечёткое сопоставление при правке и чистка вывода тестов).
  Реализация существует ровно один раз — здесь; Хост-приложение оборачивает её в
  тонкие адаптеры, а `agent-tools` отдаёт её как CLI/MCP-инструмент `code`.
  Тяжёлые AST-парсеры в ядро НЕ входят — они живут только в `tools/agent-tools`.
  Дока — [`tools/code-tools/README.md`](tools/code-tools/README.md).

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
6. **Нейтральность репо: ни имени, ни путей, ни env продуктов-потребителей** (раздел
   «⚖️ Нейтральность репозитория» выше). Имя крейта — по capability.

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