# tauri-plugin-cloud-routers

Переиспользуемый плагин Tauri v2: **шлюз облачных LLM через несколько роутеров**
([9Router](https://github.com/decolua/9router), ExtremeRouter, OmniRoute).
Работает без системных зависимостей и терминала — плагин сам скачивает портативный
`node.exe` и standalone-бандл каждого роутера.

## Что делает

- **Установка «из коробки»**: `node.exe` (официальный zip с nodejs.org, LTS) +
  npm-тарболл роутера → распаковка в `<exe>/cloud_routers/<id>/`. Пользователю не нужен
  установленный Node.js и админ-права.
- **Долгоживущий автозапуск по требованию**: сервер поднимается, когда хост
  реально использует роутер (например, выбрал комбо в чате), и продолжает работать
  в фоновом режиме после закрытия приложения, обслуживая и dev, и релизные сборки.
- **Комбо** из `/api/combos` — для дропдауна моделей чата.
- **OpenAI-совместимый чат** `/v1/chat/completions` со стримингом через
  Tauri-событие `cloud-routers-chunk`.
- **Веб-дашборд** в браузере по умолчанию.

## Поддерживаемые роутеры

| ID | NPM-пакет | Порт по умолчанию | Серверный скрипт |
|----|-----------|-------------------|------------------|
| `9router` | `9router` | 20128 | `app/custom-server.js` |
| `extremerouter` | `@rsalmn/extremerouter` | 20129 | `app/custom-server.js` |
| `omniroute` | `omniroute` | 20130 | `server.js` |

## Rust-подключение

`src-tauri/Cargo.toml`:

```toml
tauri-plugin-cloud-routers = { path = "../../my-tauri-plugins/tauri-plugin-cloud-routers" }
```

`main.rs`:

```rust
tauri::Builder::default()
    .plugin(tauri_plugin_cloud_routers::init())
    // ...
    .run(tauri::generate_context!())
```

`capabilities/default.json` — добавить `"cloud-routers:default"`.

`tauri.conf.json` — `app.withGlobalTauri: true` (для vanilla-канала) и опционально:

```json
"plugins": { "cloud-routers": { "data_dir_name": "com.kingorch.app" } }
```

## Команды

Все команды принимают параметр `router: RouterId` (`"9router" | "extremerouter" | "omniroute"`).

| Команда | Назначение |
|---------|-----------|
| `get_status` | статус (installed/running/version/port/path), сервер не запускает |
| `install_or_update` | установка/обновление (портативный Node + роутер) |
| `ensure_started` | ленивый автозапуск по требованию |
| `stop` | остановка сервера |
| `set_router_dir` | смена папки установки + проверка наличия роутера по новому пути |
| `get_combos` | список LLM-комбо (лениво стартует сервер) |
| `open_dashboard` | открыть веб-дашборд |
| `chat_completion` | OpenAI-совместимый чат (SSE-стриминг) |

События: `cloud-routers-progress` (`{router, stage, done, total, text}`) — установка;
`cloud-routers-chunk` (`{router, text, author, kind}`) — стриминг чата.

## Frontend (TS / Web Component)

```ts
import { getStatus, getCombos, chatCompletion, onChunk } from '@my-tauri-plugins/plugin-cloud-routers'
```

Или vanilla (без бандлера): глобал `window.__TAURI__['cloud-routers']` + тег
`<cloud-routers-panel>` (панель статуса в настройках). JS вшит Tauri из
`api-iife.js` (см. `PLUGIN_STANDARD.md` §5.4).

Панель `<cloud-routers-panel>` позволяет сменить папку установки кнопкой
«Изменить путь»: выбирается каталог, путь сохраняется в `cloud_routers.<id>.dir`,
работающий сервер останавливается, и статус пересчитывается под новую папку
(если в ней нет `node.exe`/серверного скрипта — высветится «Установить»).

## Хранение

- Установка: `<exe>/cloud_routers/<id>/` (`runtime/node.exe`, `dist/app/…`), переопределяется
  ключом `cloud_routers.<id>.dir`.
- Данные (БД, auth, runtime): `<app_data>/cloud_routers/<id>` (`%APPDATA%/<app_data_name>/cloud_routers/<id>`), общие для dev и релизных сборок. Поддерживается автомиграция из legacy `%APPDATA%/9router`.
- Идентичность сервера: `server.json` (pid, port, data_dir) в каталоге данных.
- Конфиг: вложенная секция `cloud_routers` в `app_config.json` хоста
  (field-preserving merge — чужие ключи не затираются).
- Порт по умолчанию: `20128` (9router), `20129` (extremerouter), `20130` (omniroute), бинд только на `127.0.0.1`.

## Сборка JS-плагина

При правке `guest-js/*`:

```
npm install
npm run build         # -> dist-js/ (npm-канал)
npm run build:global  # -> api-iife.js (vanilla-канал)
```

Оба артефакта коммитятся. Хост пересобирается через свой `.bat`-обёртчик.
