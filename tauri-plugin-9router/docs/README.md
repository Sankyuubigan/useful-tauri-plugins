# tauri-plugin-9router

Переиспользуемый плагин Tauri v2: **шлюз облачных LLM через [9Router](https://github.com/decolua/9router)**.
Работает без системных зависимостей и терминала — плагин сам скачивает портативный
`node.exe` и standalone-бандл 9router.

## Что делает

- **Установка «из коробки»**: `node.exe` (официальный zip с nodejs.org, LTS) +
  npm-тарболл 9router → распаковка в `<exe>/9router/`. Пользователю не нужен
  установленный Node.js и админ-права.
- **Ленивый автозапуск по требованию**: сервер поднимается, только когда хост
  реально использует 9router (например, выбрал комбо в чате), и гасится при
  выходе приложения через Windows Job Object `KILL_ON_JOB_CLOSE` + реестр PID.
- **Комбо** из `/api/combos` — для дропдауна моделей чата.
- **OpenAI-совместимый чат** `/v1/chat/completions` со стримингом через
  Tauri-событие `9router-chunk`.
- **Веб-дашборд** в браузере по умолчанию.

## Rust-подключение

`src-tauri/Cargo.toml`:

```toml
tauri-plugin-9router = { path = "../../my-tauri-plugins/tauri-plugin-9router" }
```

`main.rs`:

```rust
tauri::Builder::default()
    .plugin(tauri_plugin_9router::init())
    // ...
    .run(tauri::generate_context!())
```

`capabilities/default.json` — добавить `"9router:default"`.

`tauri.conf.json` — `app.withGlobalTauri: true` (для vanilla-канала) и опционально:

```json
"plugins": { "9router": { "data_dir_name": "com.kingorch.app" } }
```

## Команды

| Команда | Назначение |
|---------|-----------|
| `get_status` | статус (installed/running/version/port/path), сервер не запускает |
| `install_or_update` | установка/обновление (портативный Node + 9router) |
| `ensure_started` | ленивый автозапуск по требованию |
| `stop` | остановка сервера |
| `set_router_dir` | смена папки установки + проверка наличия 9router по новому пути |
| `get_combos` | список LLM-комбо (лениво стартует сервер) |
| `open_dashboard` | открыть веб-дашборд |
| `chat_completion` | OpenAI-совместимый чат (SSE-стриминг) |

События: `9router-progress` (`{stage, done, total, text}`) — установка;
`9router-chunk` (`{text, author, kind}`) — стриминг чата.

## Frontend (TS / Web Component)

```ts
import { getStatus, getCombos, chatCompletion, onChunk } from '@my-tauri-plugins/plugin-9router'
```

Или vanilla (без бандлера): глобал `window.__TAURI__['9router']` + тег
`<nine-router-panel>` (панель статуса в настройках). JS вшит Tauri из
`api-iife.js` (см. `PLUGIN_STANDARD.md` §4.4).

Панель `<nine-router-panel>` позволяет сменить папку установки кнопкой
«Изменить путь»: выбирается каталог, путь сохраняется в `nine_router.dir`,
работающий сервер останавливается, и статус пересчитывается под новую папку
(если в ней нет `node.exe`/серверного скрипта — высветится «Установить»).

## Хранение

- Установка: `<exe>/9router/` (`runtime/node.exe`, `dist/app/…`), переопределяется
  ключом `nine_router.dir`.
- Конфиг: вложенная секция `nine_router` в `app_config.json` хоста
  (field-preserving merge — чужие ключи не затираются).
- Порт по умолчанию: `20128`, бинд только на `127.0.0.1`.

## Сборка JS-плагина

При правке `guest-js/*`:

```
npm install
npm run build         # -> dist-js/ (npm-канал)
npm run build:global  # -> api-iife.js (vanilla-канал)
```

Оба артефакта коммитятся. Хост пересобирается через свой `.bat`-обёртчик.
