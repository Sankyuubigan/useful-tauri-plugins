# tauri-build-toolkit

Общий набор скриптов сборки/релизов Tauri-приложений (Windows), вынесенный из
эталонного проекта King Orch. Внутри — как собирать, как подключать, какие
гарантии безопасности git/gh.

> **Zero-dependency**: чистый Node (CommonJS), без `npm i`. Для САMОГО пакета
> ставить ничего не нужно. `npx tauri ...` / `gh` / `cargo` вызываются из проекта.

## Что умеет

| Команда     | Что делает                                                                                                                                  |
|-------------|---------------------------------------------------------------------------------------------------------------------------------------------|
| `build`     | dev-сборка: бамп версии + sync resources + `npm install` + иконки + `npx tauri build` + запуск приложения                                      |
| `prep`      | только подготовка (бамп версии + sync + npm + иконки) — шаг 1 `generate_installer.bat`                                                       |
| `installer` | `prep` + `npx tauri build --bundles nsis` + верификация установщика и `.sig` (подпись lenient: без ключа собирает без подписи)              |
| `release`   | полный релиз: сборка NSIS → подпись (strict) → `gh release create` (с `--repo`) → `latest.json` → коммит версии → push (ветка автодетект). В конце печатает success-отчёт (тег, продукт, установщик, URL релиза, `latest.json`, ветка); `release.bat` ставит `pause`, консоль не закрывается |
| `test`      | `cargo test [фильтр]` (MSVC-окружение — из вызывающего `.bat`)                                                                               |
| `version`   | только бамп версии по схеме `YY.M.P` и печать                                                                                                 |
| `doctor`    | read-only диагностика: конфиг, git origin, repo-guard, путь ключа подписи                                                                     |
| `init`      | раскладка `.bat`-шаблонов и `.build-config.json` в корень проекта                                                                            |
| `init --with-logs` | то же + вкладывает `LOGS_SETUP.md` — пошаговый гайд подключения `tauri-plugin-logs` (вкладка «Логи», `log::Log`, файл лога рядом с exe)      |

## Подключение к проекту (механизм интеграции)

1. **Положи toolkit** в предсказуемое место рядом с проектами, напр. `..\my-tauri-plugins\tauri-build-toolkit` (или задай env `TAURI_BUILD_TOOLKIT` на `cli.cjs`). Сам пакет развёрнут как каталог; при работе он **не пишет** в свою папку.
2. **Раскидай `.bat`-обёртки** (одним из способов):
   ```powershell
   node <toolkit>\cli.cjs init --project "D:\Projects\<my_app>"
   node <toolkit>\cli.cjs init --with-logs --project "D:\Projects\<my_app>"   # + гайд подключения tauri-plugin-logs
   ```
   Либо скопируй `templates\build.bat`, `generate_installer.bat`, `release.bat`, `test.bat` в корень проекта вручную. Окончания строк — CRLF, кодировка ASCII.
3. **Создай `.build-config.json`** в корне проекта (см. `.build-config.example.json`). Минимум обязателен `repo`; остальное либо опционально, либо автодетектится:
   - `repo` — `<owner>/<repo>` для GitHub. **Гард безопасности:** origin git обязан совпадать, иначе `release`/git-операции падают.
   - `appExe` — имя exe приложения (дефолт: `package.name` из `src-tauri/Cargo.toml`).
   - `productName` — дефолт из `tauri.conf.json`.
   - `branch` — дефолт: `git branch --show-current`.
   - `syncDirs` — каталоги, копируемые в `src-tauri/resources` перед сборкой.
   - `devWindow` — окно dev-сборки (title, размеры, `additionalBrowserArgs` для CDP-порта). Dev-override в `build` отключает бандлинг (`bundle.active=false`).
   - `vcRedist` — копировать VC++ runtime DLL рядом с exe.
   - `npmInstallArgs` — аргументы `npm install` (дефолт `["--legacy-peer-deps"]`).
   - `signing.*` — переопределение пути ключа/пароля подписи (подробнее → `docs/build_release_rules.md` §3.1).
   - `syncPackageJsonVersion` — синхронизировать версию и в `package.json` (дефолт `false`).
4. **Запускай всё через `.bat`** (они инициализируют MSVC: vswhere → vcvarsall x64), либо:
   ```powershell
   node <toolkit>\cli.cjs doctor --project "D:\Projects\<my_app>"
   ```

## Правила, которые встроены (не нарушать)

- **Никаких прямых `cargo`/`npx tauri` вручную**, если в проекте есть `.bat`-обёртки — только через них (см. `docs/build_release_rules.md` §2).
- **Единственный источник правды профиля** — `Cargo.toml [profile.release]`; `.bat`-шаблоны сбрасывают `CARGO_PROFILE_RELEASE_*`, `CC/CXX/RUSTC_WRAPPER` и т.п.
- **latest.json — только ПОСЛЕ публикации** (`gh release create`), с жёсткими проверками.
- **Выбор установщика — только точным совпадением** `_<версия>_x64-setup.exe`, никакой сортировки (анти-паттерн §3.7).
- **Сброс ключей/пароля из env, не из кода.** Ключи подписи — общие для всех проектов, в глобальной доке.

## Как протестировать пакет (короткий путь)

```powershell
node cli.cjs doctor --project "D:\Projects\<my_app>"
node cli.cjs prep --project "D:\Projects\<my_app>"     # без GUI-запуска
```
Полный `build` через `Start-Process cmd ...` — см. `docs/build_release_rules.md` §9.

## Документация

- `docs/README.md` — этот файл (механизм интеграции, команды, конфиг).
- `docs/build_release_rules.md` — правила сборки/релизов (перенесено из глобальной
  доки `desktop_rust_tauri/rules.md` §2-§3 с обобщением под любой проект).