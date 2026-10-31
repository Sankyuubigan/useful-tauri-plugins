# tauri-plugin-logs

Переиспользуемый Tauri-плагин «Логи» для desktop-приложений (стандарт по правилам
`global_ai_docs/core_rules.md` §2.5 / §2.5.1 / §2.5.2).

## Возможности

- **Единый `log::Log`** — единственная точка записи всех логов приложения:
  - exe-файл лога рядом с бинарём (путь через `current_exe().parent()`, правило cwd);
    **каждый запуск начинается с чистого файла** (truncate, как `test/last_logs.txt`, см. rules §2.5.1) — нет неконтролируемого роста между сессиями;
  - **dev-зеркало `test/last_logs.txt`** — пишется, только если рядом с exe есть каталог `test/`
    (удобно для отладки и CI; см. rules §2.5.1);
  - событие для UI-вкладки «Логи».
- **Ранний краш-лог** — `early_init`/`early_log` до старта Tauri (panic-hook).
- **«Последние логи»** — кольцевой буфер последних записей (`get_log_file_path` / last_logs).
- **Вкладка «Логи»** — web-component `<logs-panel></logs-panel>` в guest-js.
- **Облачный репорт + аналитика Aptabase** (опционально, поле `project`):
  - `reporting.enabled` — рантайм-флаг, единственный источник правды — настройка хоста
    (`tauri_plugin_logs::set_reporting_enabled(false)`, пока не зарегистрирован ключ);
  - события `track_event("имя", Some(json!({...})))` — локально всегда + в облако;
  - отключение облака не влияет на локальный файл.
- **Host RSL-признак** при debug-сборке.

## Стек

- Rust backend (`src/`), guest-js (`guest-js/`) — TypeScript; `dist-js/` — npm-канал (tsc), `api-iife.js` — vanilla-канал (esbuild). Оба — производные коммитируемые артефакты от `guest-js/` (SSOT).
- Плагин **Wry-only**: `init()` объявлен как `TauriPlugin<Wry, Config>`, все хендлеры
  не-generic (это позволяет хранить `AppHandle` в `OnceLock` для `emit` из логгера).

## Подключение (вкратце)

Проект из состава `tauri-build-toolkit` получает пошаговый гайд командой:

```
node cli.cjs init --with-logs --project <корень-проекта>
```

(вкладывает `LOGS_SETUP.md` в корень проекта). Полный список шагов:

1. `src-tauri/Cargo.toml`:
   `tauri-plugin-logs = { path = "../../my-tauri-plugins/tauri-plugin-logs" }`, `log = "0.4"`.
2. `src-tauri/tauri.conf.json` → корневой блок `plugins.logs` (схема конфига v2):
   ```json
   "plugins": {
     "logs": {
       "log_file_name": "king_orch.log",
       "last_logs": true,
       "reporting": {
         "enabled": false,
         "project": "<plugin-slug>",
         "app_key": "<aptabase-key>",
         "events": true
       }
     }
   }
   ```
3. `src-tauri/capabilities/default.json` → `"permissions": [... "logs:default"]`.
4. Rust:
   ```rust
   tauri::Builder::default()
       .plugin(tauri_plugin_logs::init())
       .setup(|app| { tauri_plugin_logs::track_event("app_started", None); Ok(()) })
   ```
5. Фронтенд:
   ```json
   "dependencies": { "@my-tauri-plugins/plugin-logs": "file:../my-tauri-plugins/tauri-plugin-logs" }
   ```
   ```ts
   import "@my-tauri-plugins/plugin-logs"; // side-effect: ловит ошибки UI
   import { logFront } from "@my-tauri-plugins/plugin-logs";
   ```
   В HTML: `<logs-panel></logs-panel>`, стили — в вашем `style.css`.
6. **Vanilla-хост (без npm/бандлера)** — официальный стандарт global API script
   (см. `PLUGIN_STANDARD.md` §5.4): никакого npm/`file:`-депа и бандлера не нужно.
   Плагин коммитит `api-iife.js`, `build.rs` регистрирует `.global_api_script_path("./api-iife.js")`,
   Tauri вшивает его в бинарник и вставляет до кода хоста (`withGlobalTauri: true`):
   `<logs-panel></logs-panel>` работает «из коробки», API доступен как `window.__TAURI__.logs`
   (`logFront`, `onLogMessage`, `getLastLogsPath`, `logFrontendEvent`).
   Распространяется изменение: правка `guest-js/` → `npm run build:global` → пересборка хоста.

## Rust API

| Функция | Назначение |
|---|---|
| `init()` | инициализация плагина (Wry-only) |
| `early_init(log_file_name)` | ранний panic-hook + старт файла лога (первая строка `main()`) |
| `early_log(level, msg)` / `log_line(level, msg)` | прямая запись строки в конвейер |
| `log::info!/warn!/error!/debug!` | стандартный маршрут (единственная точка записи) |
| `set_reporting_enabled(bool)` | гейт облачной отправки |
| `track_event(name, props: Option<Value>)` | аналитика: локально + (при enabled) в облако |

## Guest-js API

```ts
import { logFront, logFrontendEvent, onLogMessage, setReportingEnabled, trackEvent, trackError, saveLogsToFile, initFrontendErrorCapture } from "@my-tauri-plugins/plugin-logs";
```

- `logFront(msg)` / `logFrontendEvent(level, msg)` — запись строки из фронтенда в единый лог (локально). Никогда не бросают.
- `onLogMessage(cb)` — подписка на новые строки лога для UI-вкладки (`UnlistenFn`).
- `trackEvent(name, props)` / `trackError({ errorType, message, stack?, severity?, kind? })` — аналитика/ошибки.
- `setReportingEnabled(enabled)` — переключение облачного репорта (rust-функция плагина).
- `getLastLogsPath()` / `getLogFilePath()` — пути к `last_logs` и exe-файлу лога.
- `saveLogsToFile(content)` — сохранить текущие логи в файл. Диалог открывается
  с предложенным именем `<проект>_logs_<timestamp>.txt`, где `<проект>` — имя
  exe-лог-файла из конфига хоста без расширения (без него — `logs_<timestamp>.txt`).
- `initFrontendErrorCapture()` — вешает обработчики `error`/`unhandledrejection` (вызывать один раз; плагин сам не дублирует).
- `<logs-panel>` — web-component вкладки «Логи» (частично в guest-js `web-components.ts`).

## Тесты

```bash
# Rust-тесты плагина (единица + доктесты):
cargo test -p tauri-plugin-logs
```

## Чтение отчётов из Aptabase (скрипт aptabase_read)

`scripts/aptabase_read.mjs` — CLI для чтения отчётов об ошибках и аналитики
Aptabase (проверка багов юзеров без открытия дашборда). Только GET: тестовые
события/ошибки в прод-облако из скрипта не отправляются.

### Доступ (кука auth-session)

У облачного Aptabase **нет публичного query-API и read-ключей** — чтение идёт
через cookie `auth-session` дашборда (срок жизни ~365 дней, обновлять при 401).
Ключ `A-EU-*` в `tauri.conf.json` — ключ **отправки**, читать им нельзя.

1. Войти на `https://eu.aptabase.com` в браузере.
2. DevTools (F12) → Application → Cookies → `https://eu.aptabase.com` →
   скопировать значение куки `auth-session`.
3. Сохранить значение в `%APPDATA%\aptabase\auth-session.txt`
   (или передавать через env `APTABASE_AUTH_SESSION`).

**Кука = полный доступ к дашборду**: не коммитить, не печатать в логах/чатах.

4. `%APPDATA%\aptabase\config.json`: `{"baseUrl": "https://eu.aptabase.com", "appId": "<id из list-apps>"}`.

### Команды

```
node scripts/aptabase_read.mjs list-apps                     # приложения (найти appId)
node scripts/aptabase_read.mjs errors --days 30 --kind crash # ошибки за 30 дней
node scripts/aptabase_read.mjs error-types --days 30         # уникальные типы
node scripts/aptabase_read.mjs error-detail <errorId>        # полный стек одной ошибки
node scripts/aptabase_read.mjs metrics --period 30d          # DAU/сессии/события
node scripts/aptabase_read.mjs top-events --period 90d       # топ событий
node scripts/aptabase_read.mjs top-versions --period 90d     # топ версий приложения
node scripts/aptabase_read.mjs usage                         # месячный расход (квота 20 000/мес free)
```

Фильтры `errors`: `--days N | --from YYYY-MM-DD --to YYYY-MM-DD`, `--type`,
`--severity fatal|error`, `--kind crash|unhandled|taskException|handled`,
`--os`, `--limit` (1..100, по умолчанию 50), `--offset`, `--json`.
Общие: `--app <appId>`, `--debug` (buildMode=debug, по умолчанию release),
`--base-url`, `--period` (`24h|48h|today|yesterday|7d|14d|30d|month|last-month|90d|180d|365d|all`).

Exit-коды: `0` — успех, `1` — ошибка запроса/формата, `2` — кука не найдена,
`3` — кука истекла/не принята (HTTP 401). HTTP 429 — rate limit (~1000 req/час).

### Известные ограничения данных (важно при анализе)

- **Breadcrumbs в облако не доходят**: сервер Aptabase (`ErrorBody.cs`) поле
  `breadcrumbs` игнорирует (молча отбрасывает). Хроника действий перед ошибкой
  живёт только в локальном `king_orch.log` юзера — для контекста просить файл у юзера.
- Поле `project` в error-теле сервером **сейчас игнорируется** (задел на
  несколько приложений на одном App-Key, не действующий контракт).
- Обрыв «какой версией приложено» — до релиза бинаря с обогащённым телом
  (`appVersion`/`osName`/`sessionId`, см. `src/error_reporting/aptabase.rs`)
  в дашборде Errors фильтров по версии/ОС не будет.

## Структура

```
src/lib.rs        — init(), Config, Rust API
src/logger.rs     — единый log::Log, файл (truncate на старте сессии) + last_logs-зеркало + host RSL
src/path.rs       — резолв путей (cwd + test/)
src/commands.rs   — Tauri-команды (get_*, track_*, set_*)
src/error_reporting/ — облачный репорт + Aptabase (mod.rs, aptabase.rs)
guest-js/         — TS-обвязка + web-components + shims (dist-js/, api-iife.js — производное)
permissions/      — капабилити ("logs:default")
build.rs          — сборка плагина (COMMANDS + global_api_script_path)
scripts/          — build-global.cjs (сборка api-iife.js), aptabase_read.mjs (чтение отчётов Aptabase)
```