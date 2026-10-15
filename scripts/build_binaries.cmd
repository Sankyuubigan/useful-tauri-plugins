@echo off
rem Обновляет постоянные копии бинарников инструментов в папке bin\ репозитория.
rem
rem Зачем отдельный скрипт, если cargo и так собрал бинарь: папка target\Release
rem удаляется командой `cargo clean`, а путь к инструменту должны знать агенты и
rem хосты в ДРУГИХ репозиториях. Путь из target\ недолговечен, bin\ — постоянный
rem и при этом единственный: две копии одного бинаря разъезжаются.
rem
rem Запуск из корня репозитория my-tauri-plugins:
rem     scripts\build_binaries.cmd
rem
rem Требует MSVC в PATH: скрипт НЕ поднимает vcvarsall сам (это делает
rem test-plugin.bat). Если cargo не найден — запусти в окружении, где MSVC
rem инициализирован, либо через test-plugin.bat agent-tools --release.

setlocal
set REPO=%~dp0..
set TARGET=%REPO%\target\release
set DEST=%REPO%\bin

if not exist "%DEST%" mkdir "%DEST%"

rem Список бинарей, которые разворачиваются наружу из target\.
rem Правка списка — здесь и только здесь: это единственное место, где решается,
rem что именно лежит в bin\.
set BINS=agent-tools

where cargo >nul 2>&1
if errorlevel 1 (
    echo [ERROR] cargo не найден в PATH. Открой окружение с MSVC ^(через test-plugin.bat^) и повтори.
    exit /b 1
)

echo [1/2] cargo build --release
cargo build --release -p agent-tools || goto :failed

echo [2/2] копирование в bin\
for %%B in (%BINS%) do (
    if not exist "%TARGET%\%%B.exe" (
        echo [ERROR] не собран %%B.exe — ожидался %TARGET%\%%B.exe
        exit /b 1
    )
    copy /Y "%TARGET%\%%B.exe" "%DEST%\%%B.exe" >nul || goto :failed
    echo       bin\%%B.exe
)

echo.
echo Готово. Путь для агентов: %DEST%
exit /b 0

:failed
echo.
echo [ERROR] сборка или копирование не удались. См. вывод выше.
exit /b 1