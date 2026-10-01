@echo off
setlocal enableextensions
rem Временный раннер тестов плагина tauri-plugin-llama-engine.
rem MSVC инициализируется так же, как в king_orch_3\test.bat — иначе cargo
rem не находит cl.exe/sccache (правило проекта: не запускать cargo без VS-окружения).

set "PROJ=%~dp0.."
pushd "%PROJ%"

set "VS_INIT_OK="
for /f "usebackq delims=" %%i in (`"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -property installationPath 2^>nul`) do (
    if exist "%%i\VC\Auxiliary\Build\vcvarsall.bat" (
        call "%%i\VC\Auxiliary\Build\vcvarsall.bat" x64 >nul 2>&1
        set "VS_INIT_OK=1"
    )
)
if not defined VS_INIT_OK (
    echo [ERROR] Visual Studio not found.
    popd
    exit /b 1
)

sccache --start-server >nul 2>&1

cargo test --lib %*
set "RC=%ERRORLEVEL%"

popd
exit /b %RC%