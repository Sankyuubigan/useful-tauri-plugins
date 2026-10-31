# GUI-тестирование Tauri-приложений ИИ-агентом (без зрения, через CDP)

> **Для кого:** ИИ-агенты (opencode и т.п.), которым нужно тестировать GUI десктопного Tauri-приложения на Windows (WebView2).
> **Суть:** Интерфейс Tauri-приложения — это веб-страница внутри WebView2 (Chromium). Агент может подключиться к ней по CDP-порту и читать DOM / кликать / вводить текст / делать скриншоты. Пиксели и «зрение» не нужны.
> **Каноническая интеграция в этом репо:** `tauri-build-toolkit` — порт включается через `devWindow.additionalBrowserArgs` в `.build-config.json` (см. `build_release_rules.md` §5, `templates/.build-config.example.json`). Краткая выжимка правил — там же, полный гайд — здесь.

## 1. Почему это работает

- WebView2 — это Chromium; при запуске с `--remote-debugging-port` он открывает CDP-эндпоинт (`http://127.0.0.1:<port>`), как обычный Chrome.
- Подключение: Playwright/puppeteer через `connectOverCDP` либо CLI `agent-browser --cdp <port>`.
- Клики по HTML-элементам (селекторы/id) всегда точны, в отличие от координат на скриншоте.

## 2. Как включить debug-порт (сторона приложения)

### ⚠️ ЛОВУШКА №1: env-переменная WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS НЕ работает с wry

Казалось бы, достаточно запустить приложение с `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 --remote-allow-origins=*`.
**Это НЕ сработает в приложениях на wry (Tauri 2)**: wry **всегда** задаёт свои дополнительные
аргументы через `CoreWebView2EnvironmentOptions::set_additional_browser_arguments` (см.
`wry-*/src/webview2/mod.rs` — дефолт `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`).
По документированному поведению WebView2: если аргументы заданы явно в опциях среды — переменная
окружения **игнорируется**.

### Правильный способ: `app.windows[].additionalBrowserArgs` (Tauri 2)

Поле конфига Tauri (Windows-only), которое ПОДМЕНЯЕТ дефолтные аргументы wry:

```json
{ "app": { "windows": [{ "additionalBrowserArgs": "--remote-debugging-port=9222 --remote-allow-origins=*" }] } }
```

Обязательные требования:
- **`--remote-allow-origins=*` обязателен** (Chromium 111+ отклоняет CDP-подключения из чужого origin без него).
- **Только dev-сборки**: порт нельзя вшивать в production-релиз (любой локальный процесс сможет управлять приложением). В King Orch — через конфиг-оверрайд, который build.cjs передаёт в `npx tauri build --config` (при слиянии конфигов массивы заменяются целиком, поэтому окно в оверрайде указывается ПОЛНЫМ объектом: title/width/height/resizable/fullscreen + additionalBrowserArgs).

Проверить, что порт открыт: `Invoke-RestMethod http://127.0.0.1:9222/json/version` (или в браузере открыть `http://127.0.0.1:9222`).

## 3. Как подключиться агенту (сторона тестирования)

### Вариант A (рекомендуемый): agent-browser CLI

```powershell
# Snapshot интерактивных элементов с ref'ами (@e1, @e2...)
npx agent-browser --cdp 9222 snapshot -i

# Действия (ref'ы из snapshot)
npx agent-browser --cdp 9222 click @e3
npx agent-browser --cdp 9222 fill @e2 "текст"
npx agent-browser --cdp 9222 select @e1 "опция"
npx agent-browser --cdp 9222 press Enter
npx agent-browser --cdp 9222 wait 1000
npx agent-browser --cdp 9222 get text body
npx agent-browser --cdp 9222 screenshot app.png
npx agent-browser --cdp 9222 eval 'document.title'
```

Рабочий цикл: `snapshot -i` → действие → повторный `snapshot -i` (ref'ы инвалидируются после изменений DOM).

### ⚠️ ЛОВУШКА №2: agent-browser через bash-тул opencode на Windows

agent-browser держит фоновый демон; прямой вызов `npx agent-browser --cdp 9222 snapshot` через bash-тул
opencode может быть убит тулом (`ChildProcess.kill` — тул ждёт всё дерево процессов, см. §9 в
`D:\Projects\docusaurus-starter\docs\Sega Mega Note\Моя картотека\software\настройки\global_ai_docs\desktop_rust_tauri_rules.md`).
**Правильный вызов — через Start-Process с редиректом в файл и опросом (чтение только с `-Encoding UTF8`):**

```powershell
Remove-Item test\gui_snap.txt -ErrorAction SilentlyContinue
Start-Process npx.cmd -ArgumentList '-y','agent-browser','--cdp','9222','snapshot','-i' `
  -WorkingDirectory <project> -RedirectStandardOutput <project>\test\gui_snap.txt `
  -RedirectStandardError <project>\test\gui_snap_err.txt -WindowStyle Hidden
# затем опрос: Get-Content test\gui_snap.txt -Raw -Encoding UTF8
```

### Вариант B: puppeteer-core скрипт (фолбэк)

Если CLI не распознаёт WebView2 — скрипт на puppeteer-core (в проектах King Orch уже есть как npm-зависимость MCP-сервера browser):

```javascript
const puppeteer = require("puppeteer-core"); // или npm:puppeteer-core в Deno
const browser = await puppeteer.connect({ browserURL: "http://127.0.0.1:9222" });
const pages = await browser.pages();
const page = pages[0];
await page.click("#btn-send");
await page.type("#chat-input", "привет");
```

## 4. Типовой процесс тестирования

1. **Убедись, что приложение запущено** с debug-портом (см. §2; в King Orch — запуск через `build.bat`).
2. **Подключись**: `npx agent-browser --cdp 9222 snapshot -i`.
3. **Изучи интерфейс**: снапшот показывает кнопки/инпуты с текстом и ref'ами. При необходимости `get text body` для полного текста или `screenshot` для визуальной проверки.
4. **Выполни сценарий**: клики, ввод, выбор из селектов; после каждого значимого шага — новый снапшот и проверка ожидаемого результата (появился текст, кнопка стала disabled, открылась вкладка и т.д.).
5. **Проверь состояние**: текст сообщений, `disabled`/`visible`, содержимое областей через `get text <selector>`.
6. **Зафиксируй результат**: скриншот (`screenshot`), при необходимости `diff snapshot` для сравнения состояний.

## 5. Особенности и ограничения

- **Тестируется ЖИВОЕ приложение.** Действия влияют на реальное состояние (сессии, настройки). Для тестов используй отдельную тестовую сессию; не кликай во время долгих операций (генерация LLM, скачивание) — дождись завершения через `wait`.
- **WebView2 при сворачивании может троттлить** — окно должно быть открыто/не свёрнуто для надёжных скриншотов.
- **Селекторы**: предпочитай стабильные `id` (в King Orch все элементы UI имеют `id`, см. `index.html` / `docs/GUI_TESTING.md`).
- **Порт занят** — выбери другой (параметризуй в build.cjs; порт должен совпадать в env приложения и в `--cdp`).
- **Данные на диске**: CDP даёт доступ к DOM, а не к файловой системе. Файловые проверки делай обычными инструментами (fs_read и т.п.).
- **Security**: никогда не включай порт в релизных сборках (см. §2).
- **CSP**: если у приложения строгий CSP — это не мешает CDP (CDP работает вне CSP).

## 6. Чеклист агента перед тестом GUI

- [ ] Приложение запущено через dev-скрипт (build.bat), а не установщик
- [ ] `http://127.0.0.1:9222/json/version` отвечает
- [ ] Первый `snapshot -i` показывает элементы UI приложения
- [ ] Тестовые действия выполняются на тестовой сессии/данных
- [ ] После каждого шага — re-snapshot и проверка ожидаемого результата
- [ ] В отчёте: что нажимали, что ожидали, что увидели (+ скриншот при необходимости)