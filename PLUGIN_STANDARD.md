# Переиспользуемые модули как Tauri-плагины (репозиторий `my-tauri-plugins`)

> **Назначение доки:** подробный, воспроизводимый инженерный референс по тому, как выносить
> переиспользуемый модуль (бизнес-логику **и** GUI) из Tauri-приложения в отдельный репозиторий
> `my-tauri-plugins` в виде **Tauri-плагина** (Rust-крейт + npm-пакет + Web Component). Написано
> так, чтобы по этой доке архитектуру можно было **один-в-один повторить в любом другом
> Rust/Tauri-проекте**.
>
> **Эталонная реализация ( reference implementation ):** `tauri-plugin-logs` (модуль «Логи»:
> один `log::Log`, файл лога + зеркало `test/last_logs.txt`, UI-вкладка через web-component
> `<logs-panel>`) — первый плагин, доведённый до стандарта BOTH-канала (npm **и** vanilla).
> `tauri-plugin-about-updates` и `tauri-plugin-speech` следуют тому же стандарту.
>
> **Ключевой принцип:** модуль переиспользуется как **compile-time зависимость** (path-зависимость
> в `Cargo.toml`), GUI переиспользуется через **Web Component** (фреймворк-агностично). JS-сторона
> плагина доставляется хосту **двумя каналами из одного источника** — см. §4.4.

---

## 0. Архитектурная сводка

```
my-tauri-plugins/                      ← отдельный git-репозиторий (НЕ внутри хост-проекта)
├─ Cargo.toml                          ← workspace: members + [workspace.dependencies]
├─ PLUGIN_STANDARD.md                  ← эта дока (SSOT инженерного стандарта)
└─ tauri-plugin-logs/                  ← один плагин = крейт + npm-пакет + iife-глобал
   ├─ Cargo.toml                       ← links + build-dep tauri-plugin
   ├─ build.rs                         ← COMMANDS + Bundle.New(COMMANDS).global_api_script_path(...)
   ├─ src/                             ← Rust: lib.rs, commands.rs, models.rs, logger.rs, path.rs ...
   ├─ permissions/default.toml         ← allow-* + [default]
   ├─ package.json                     ← @my-tauri-plugins/plugin-logs (+ esbuild devDep)
   ├─ tsconfig.json
   ├─ guest-js/
   │  ├─ index.ts                      ← типизированный TS-API (invoke) + side-effect import WC
   │  ├─ web-components.ts             ← <logs-panel> (Shadow DOM + CSS-переменные)
   │  └─ iife-entry.ts                 ← точка входа для esbuild (см. §4.4)
   ├─ dist-js/                         ← npm-канал (tsc, КОММИТИТСЯ, артефакт)
   └─ api-iife.js                      ← vanilla-канал (esbuild, КОММИТИТСЯ, артефакт)

repos-control/                         ← ХОСТ (vanilla, без npm/бандлера)
├─ src-tauri/
│  ├─ Cargo.toml                       ← path = "../../my-tauri-plugins/tauri-plugin-logs"
│  ├─ main.rs                          ← .plugin(tauri_plugin_logs::init())
│  ├─ tauri.conf.json                  ← "plugins": { "logs": {...} }, "withGlobalTauri": true
│  └─ capabilities/default.json        ← "logs:default"
└─ ui/index.html                       ← <logs-panel></logs-panel> — и всё (JS плагина вшит Tauri)
```

**Три имени плагина (важно не путать):**
| Что | Пример | Где используется |
|-----|--------|-------------------|
| Cargo-пакет | `tauri-plugin-logs` | имя папки/крейта, `links` в `Cargo.toml` |
| Rust-lib ( crate name ) | `tauri_plugin_logs` | `tauri_plugin_logs::init()` в хосте |
| plugin-identifier | `logs` | `Builder::new("logs")` → invoke `plugin:logs\|<cmd>` и capability `logs:default` |
| npm-пакет | `@my-tauri-plugins/plugin-logs` | `import` во фронтенде bundler-хоста |

---

## 1. Репозиторий `my-tauri-plugins`

Это **отдельный** git-репозиторий (sibling хост-проектов), а не сабмодуль и не папка внутри проекта.
Каждый плагин — отдельный workspace-member.

`my-tauri-plugins/Cargo.toml` (workspace с общими зависимостями):
```toml
[workspace]
resolver = "2"
members = ["tauri-plugin-about-updates", "tauri-plugin-speech", "tauri-plugin-logs"]

[workspace.dependencies]
tauri = { version = "2", features = [] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
# ... остальное общее (gix, chrono, anyhow, аудио-либы speech и т.п.)
```

Каждый плагин сам содержит и Rust-крейт, и npm-пакет (`package.json` + `guest-js/`). Фронтенд
плагина собирается **двумя путями**: `npm run build` (tsc → `dist-js/`, npm-канал) и
`npm run build:global` (esbuild → `api-iife.js`, vanilla-канал). Оба артефакта **коммитятся** —
cargo не должен требовать node при сборке хоста.

---

## 2. Единый источник правды (SSOT) для JS-стороны

**Один источник — `guest-js/` (там правки руками).** Артефакты `dist-js/` и `api-iife.js` —
**производные**, коммитятся, генерируются сборкой, руками НЕ правятся. Поменял `guest-js/*` →
`npm run build` + `npm run build:global` + коммит обоих артефактов → хост при пересборке получает
новую версию.

> ⚠️ Правило: забыл пересобрать артефакт после правки `guest-js/*` → хост тянет старый JS
> (та же механика, что у официального `dist-js`).

---

## 3. Rust-часть плагина (КРИТИЧНО — иначе не соберётся/не запустится)

### 3.1 `Cargo.toml` — обязателен `links` + build-зависимость

```toml
[package]
name = "tauri-plugin-logs"
version = "0.1.0"
edition = "2021"
links = "tauri-plugin-logs"        # ← ОБЯЗАТЕЛЬНО (без пробелов, с дефисами)

[dependencies]
tauri = { workspace = true }
# ...

[lib]
name = "tauri_plugin_logs"
path = "src/lib.rs"

[build-dependencies]
tauri-plugin = { version = "2", features = ["build"] }   # ← для permissions + глobal-скрипта
```

> ⚠️ Без `links`/`build.rs` permissions не сгенерятся → хост упадёт с
> `permission "logs:default" not found`. Без `global_api_script_path` не будет vanilla-канала.

### 3.2 `build.rs` — команды + `global_api_script_path` (vanilla-канал)

```rust
const COMMANDS: &[&str] = &["get_last_logs_path", "log_frontend_event", /* ... */];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .global_api_script_path("./api-iife.js")   // ← vanilla-канал (см. §4.4)
        .build();
}
```

`COMMANDS` — **единственный источник правды** для имён команд плагина.

### 3.3 `src/lib.rs` — Config, PluginState, `init`

```rust
use serde::Deserialize;
use tauri::{plugin::{Builder, TauriPlugin}, Manager, Runtime};

/// Конфиг из tauri.conf.json хоста: "plugins": { "logs": { "log_file_name": "...", "last_logs": true } }
#[derive(Deserialize, Clone)]
pub struct Config { /* ... */ }

/// Вызывается из хоста: `.plugin(tauri_plugin_logs::init())`.
pub fn init<R: Runtime>() -> TauriPlugin<R, Config> {
    Builder::<R, Config>::new("logs")
        .setup(|app, api| { /* инициализация состояния, log::Log, путь last_logs */ Ok(()) })
        .invoke_handler(tauri::generate_handler![commands::get_last_logs_path, /* ... */])
        .build()
}
```

### 3.4 Команды — ОБЯЗАНЫ быть дженериками над `Runtime`

```rust
#[tauri::command]
pub fn get_last_logs_path<R: Runtime>(app: AppHandle<R>) -> String { /* ... */ }
```
Не-дженерик `AppHandle` даёт `E0277` на этапе сборки крейта хоста.

### 3.5 Порядок добавления НОВОЙ команды (чек-лист)

1. `#[tauri::command] pub fn my_cmd<R: Runtime>(...)` в `commands.rs`;
2. `"my_cmd"` в `COMMANDS` (`build.rs`) **и** в `generate_handler!` (`lib.rs`);
3. `[[permission]] allow-my-cmd` + в `[default].permissions` (`permissions/default.toml`).

Любое расхождение → рантайм-ошибка «permission not found» для команды.

---

## 4. Frontend-часть плагина

### 4.1 `guest-js/index.ts` — типизированный TS-API

```ts
import { invoke } from '@tauri-apps/api/core'

export async function getLastLogsPath(): Promise<string> {
  return invoke<string>('plugin:logs|get_last_logs_path')
}
// ... остальные команды

// Обязательно в конце: side-effect import регистрирует Web Component при импорте пакета.
import './web-components'
```

> Вызовы идут через `plugin:<identifier>|<command>` — identifier из `Builder::new("logs")`.

### 4.2 `package.json` плагина

```json
{
  "name": "@my-tauri-plugins/plugin-logs",
  "version": "0.1.0",
  "type": "module",
  "main": "dist-js/index.js",
  "types": "dist-js/index.d.ts",
  "scripts": {
    "build": "tsc -p tsconfig.json",
    "build:global": "esbuild guest-js/iife-entry.ts --bundle --format=iife --outfile=api-iife.js",
    "check:global": "esbuild guest-js/iife-entry.ts --bundle --format=iife --outfile=api-iife.js && node --check api-iife.js"
  },
  "dependencies": { "@tauri-apps/api": "^2", /* другие @tauri-apps/plugin-* */ },
  "devDependencies": { "typescript": "^5.4.0", "esbuild": "^0.21.0" }
}
```

### 4.3 `tsconfig.json`

```json
{
  "compilerOptions": {
    "target": "ES2020", "module": "ESNext", "moduleResolution": "Bundler",
    "lib": ["ES2020", "DOM", "DOM.Iterable"], "strict": true,
    "declaration": true, "outDir": "dist-js", "rootDir": "guest-js", "skipLibCheck": true
  },
  "include": ["guest-js/**/*.ts"]
}
```

### 4.4 vanilla-канал: global API script (`api-iife.js`) — СТАНДАРТ

Этот канал — официальный механизм Tauri v2 для доставки JS плагина в хост **без npm и бандлера**
(канал всех официальных плагинов Tauri). Конвейер:

1. Плагин собирает **IIFE** из `guest-js` (esbuild) и коммитит `api-iife.js` в корень плагина.
2. `build.rs` регистрирует его: `.global_api_script_path("./api-iife.js")` → Cargo/env
   `DEP_<crate>_GLOBAL_API_SCRIPT_PATH`.
3. `tauri-build` хоста на сборке кладёт скрипт в `OUT_DIR/__global-api-script.js`, `tauri-codegen`
   **вшивает его содержимое в бинарник** (только если `app.withGlobalTauri: true`).
4. Рантайм Tauri вставляет скрипт в вебвью **первым** (`main_frame_script`), ДО фронтенд-кода хоста.
   → `<logs-panel>` уже определён, `window.__TAURI__.<plugin>` уже доступен.

**Требования к `api-iife.js`:**
- Никаких голых импортов (`import { x } from '@tauri-apps/api/...'`). Всё, что использовал
  `guest-js` через `@tauri-apps/*`, в IIFE биндится на **глобалы**:
  - `@tauri-apps/api/core` → `window.__TAURI__.core` (иначе `window.__TAURI_INTERNALS__.invoke`);
  - `@tauri-apps/api/event` → `window.__TAURI__.event`;
  - `@tauri-apps/plugin-dialog` → `window.__TAURI__.dialog` (появляется, если хост включил этот
    плагин со своим iife); `@tauri-apps/plugin-updater` → `window.__TAURI__.updater`;
    `@tauri-apps/plugin-shell` → `window.__TAURI__.shell` — и т.д.
- **Динамические `import('@tauri-apps/...')` в IIFE НЕ работают** — их надо заменить на вызов
  через соответствующий глобал с проверкой наличия и фолбэком (см. пример ниже).
- Идемпотентность: guard в начале, как у официального `api-iife.js`:
  `if ("__TAURI__" in window && window.__TAURI__.<plugin>) return;`
- В конце: `customElements.define('logs-panel', ...)` (если WC) и/или
  `Object.defineProperty(window.__TAURI__, '<plugin>', { value: {...} })`.

**Как это уживается с `@tauri-apps/api`:** esbuild-бандл нельзя просто собрать из `guest-js`
напрямую — он импортирует `@tauri-apps/*`. Поэтому у каждого плагина есть `guest-js/iife-entry.ts`,
который импортирует код плагина через АЛИАСЫ esbuild (см. §9.3). Механика:
- в `guest-js/` рядом с кодом лежат шимы `shims.ts` вида:
  ```ts
  const core = (window as any).__TAURI__?.core ?? (window as any).__TAURI_INTERNALS__;
  export const invoke = core.invoke.bind(core);
  export const Channel = core.Channel;
  ```
  Соответственно шимы для `event`, `dialog`, `updater`, `shell`;
- `iife-entry.ts` собирает всё вместе: `import { ... } from './web-components'` + регистрация
  `window.__TAURI__.<plugin>` + вызов `initFrontendErrorCapture()` где есть;
- esbuild: `--alias:@tauri-apps/api/core=./guest-js/shims/core.ts` и т.д. (подменяет голые импорты
  на шимы; сам `guest-js/index.ts` и `web-components.ts` НЕ трогаются).

**Пример замены динамического диалога (внутри iife-entry или шима):**
```ts
export async function saveDialog(): Promise<string | null> {
  const dlg = (window as any).__TAURI__?.dialog
  if (!dlg?.save) return null          // диалог не включён в хосте — честный фолбэк
  return dlg.save({ title: 'Сохранить логи', filters: [...] }) ?? null
}
```

> Хост не получает JS плагина «из руки» — код вшивается Tauri из бинарника. Пропагация правок:
> `guest-js/*` → `npm run build:global` плагина (пересобрался `api-iife.js`) → пересборка хоста.
> Менять/копировать `api-iife.js` в хосте ЗАПРЕЩЕНО.

### 4.5 Стилизация Web Component (САМОЕ ВАЖНОЕ для GUI)

Темизируй компонент через CSS-переменные хоста (`var(--token, fallback)`). Custom properties со
`:root` хоста проникают сквозь Shadow DOM. **Никогда не хардкодь светлые цвета.** Внешние ссылки —
через `@tauri-apps/plugin-shell` → `open(...)` (не `window.open`).

---

## 5. Permissions (`permissions/default.toml`)

```toml
"$schema" = "../gen/schemas/acl.json"

[[permission]]
identifier = "allow-get-last-logs-path"
description = "..."
commands.allow = ["get_last_logs_path"]

# ... по одному allow-* на каждую команду из COMMANDS

[default]
description = "Default permissions for the logs plugin."
permissions = [ "allow-get-last-logs-path", /* ... */ ]
```

Хост подключает плагин целиком через `"logs:default"` в capability.

---

## 6. Интеграция в хост

### 6.1 `src-tauri/Cargo.toml` (путь — именно `../../`)

```toml
[dependencies]
tauri-plugin-logs = { path = "../../my-tauri-plugins/tauri-plugin-logs" }
```
Плюс зависимости `package.json`/npm НЕ нужны — они только для bundler-хостов (см. §6.5).

### 6.2 `src-tauri/src/main.rs` / `lib.rs`

```rust
// в цепочке .plugin(...):
.plugin(tauri_plugin_logs::init())
// УБРАТЬ из invoke_handler дублирующие команды, которые теперь в плагине.
```

### 6.3 `src-tauri/capabilities/default.json`

```json
{
  "permissions": ["core:default", "logs:default"]
}
```

### 6.4 `src-tauri/tauri.conf.json`

```json
{
  "app": { "withGlobalTauri": true, ... },
  "plugins": { "logs": { "log_file_name": "repos_control.log", "last_logs": true } }
}
```

### 6.5 Фронтенд — ДВА канала (выбери один для своего хоста)

**A. Хост с npm/бандлером** — npm-канал: `package.json` хоста
`"@my-tauri-plugins/plugin-logs": "file:../my-tauri-plugins/tauri-plugin-logs"`, затем
`import "@my-tauri-plugins/plugin-logs";` (side-effect → регистрация WC). Официальный путь для
bundler-проектов.

**B. Хост vanilla (без npm/бандлера)** — vanilla-канал: НИЧЕГО в npm не добавляем. Просто:
```html
<logs-panel></logs-panel>
```
JS плагина вшит в бинарник Тauri и вставлен до кода хоста. Доступ к API из своей UI:
`window.__TAURI__.log.logFront(...)`, `window.__TAURI__.event.listen('logs:message', ...)`.

---

## 7. Сборка

- **Плагин** (при правке `guest-js/*`): в папке плагина `npm install` → **`npm run build`**
  (→ `dist-js`) **и `npm run build:global`** (→ `api-iife.js`), коммит обоих артефактов.
  Без `build:global` vanilla-хост получит старый JS.
- **Хост**: только через `.bat`-обёртки (см. `rules.md` §2). `build.bat` компилирует path-зависимый
  крейт плагина (а вместе с ним вшивает и `api-iife.js` из OUT_DIR).

---

## 8. Чек-лист создания/переноса плагина

1. Создать папку `tauri-plugin-<name>/` как member workspace `my-tauri-plugins`.
2. `Cargo.toml`: `links` + `[build-dependencies] tauri-plugin`.
3. `build.rs`: `COMMANDS` + `.global_api_script_path("./api-iife.js")` + `.build()`.
4. `lib.rs`: `Config`, `PluginState`, `init<R: Runtime>()`, `generate_handler!`.
5. Команды — строго `pub fn cmd<R: Runtime>(app: AppHandle<R>, ...)`.
6. `permissions/default.toml`: `allow-*` на каждую команду + все в `[default]`.
7. `package.json`/`tsconfig.json`: scripts `build` (tsc) + `build:global` (esbuild), devDeps
   `typescript` + `esbuild`.
8. `guest-js/index.ts` (`plugin:<id>|<cmd>`) + `web-components.ts` (WC, `var(--token)`).
9. `guest-js/iife-entry.ts` + шимы `@tauri-apps/*` (глобалы `window.__TAURI__.*`) — §4.4, §9.3.
10. Собрать и закоммитить `dist-js/` И `api-iife.js`.
11. Хост: path-зависимость `../../`, `.plugin(...)`, capability `<id>:default`, `plugins.<id>`
    в конфиге; фронтенд — тег WC (vanilla) или `import` (npm).
12. `npm install` + `npm run build` + `npm run build:global` в плагине → `.bat` хоста.

---

## 9. Подводные камни

- **camelCase, НЕ snake**: `fn tts_list_models(models_dir)` ждёт от JS `{ modelsDir }`.
- **Debug-бинарь грузит `devUrl`, а не `frontendDist`** — standalone только через `tauri build`
  (фича `custom-protocol`). В этом проекте dev-режим вообще запрещён.
- **`sccache` ломает C-сборку** — чисти `CC`/`CXX`/`rustc-wrapper` (см. `rules.md`).
- **«permission not found»** → нет `links`/`build.rs` ИЛИ команда не во всех трёх местах.
- **`E0277: AppHandle: CommandArg`** → команда не `<R: Runtime>`-дженерик.
- **`config().version` — `Option<String>`** → `app.package_info().version`.
- **Неверный путь зависимости** → от `src-tauri/` пиши `../../my-tauri-plugins/...`, не `../`.
- **WC «белый в тёмной теме»** → захардкожены светлые цвета; только `var(--token)`.
- **Динамические `import('@tauri-apps/...')` ломают esbuild-IIFE** → замени на глобал
  `window.__TAURI__.<plugin>` с фолбэком.
- **Забыл исполнить `npm run build:global`** после правки `guest-js/*` → vanilla-хост тянет старый
  JS: всегда пересобирай оба артефакта.
- **`window.__TAURI__.<plugin>` нет в хосте** → в `build.rs` плагина не добавлен
  `global_api_script_path`, ИЛИ у хоста выключен `withGlobalTauri`.
- **Порядок iife при связанных плагинах** (напр. logs зависит от dialog): главный скрипт биндится на
  глобал зависимого плагина **в момент вызова**, а не при загрузке → связь по порядку не критична.

---

## 10. Планы (следующие модули)

`tauri-plugin-llama-engine` (GUI + логика движка llama.cpp) и др. — те же принципы: Rust-крейт +
Web Component + **два JS-канала** (npm `dist-js` + vanilla `api-iife.js`).