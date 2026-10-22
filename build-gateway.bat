@echo off
setlocal enableextensions

rem Сборка release-бинаря шлюза cloud-routers-gateway.
rem Окружение — по образцу test-plugin.bat: vswhere → vcvarsall x64
rem (прямой cargo-запуск без MSVC-инициализации запрещён правилами сборки).
rem
rem Место результата: D:\Projects\my-tauri-plugins\target\release\cloud-routers-gateway.exe —
rem именно его видит `check_router_update` через source_candidates, поэтому
rem после пересборки панель King Orch при следующем старте сама предложит
rem «Обновить сейчас» (сравнение версии/mtime/размера).

for /f "usebackq delims=" %%i in (`"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -property installationPath 2^>nul`) do (
    if exist "%%i\VC\Auxiliary\Build\vcvarsall.bat" (
        call "%%i\VC\Auxiliary\Build\vcvarsall.bat" x64 >nul 2>&1
        echo VS=%%i
    )
)

cargo build -p cloud-routers-gateway --release
set EXIT=%ERRORLEVEL%
echo EXIT=%EXIT%
exit /b %EXIT%
