@echo off
setlocal enableextensions
set PLUGIN=%1
if "%PLUGIN%"=="" set PLUGIN=tauri-plugin-llama-engine
shift

rem Цели cargo test. Без второго аргумента — только unit-тесты крейта (--lib)
rem с 4 потоками. Со вторым аргументом — цели и флаги тестов передаются как есть:
rem   test-plugin.bat tauri-plugin-about-updates --test rollback_download -- --ignored --nocapture
rem (shift в bat не меняет %*, поэтому позиционные аргументы собираем явно)
set CUSTOM=%1 %2 %3 %4 %5 %6 %7 %8 %9

for /f "usebackq delims=" %%i in (`"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -property installationPath 2^>nul`) do (
    if exist "%%i\VC\Auxiliary\Build\vcvarsall.bat" (
        call "%%i\VC\Auxiliary\Build\vcvarsall.bat" x64 >nul 2>&1
        echo VS=%%i
    )
)

if "%1"=="" (
    cargo test -p %PLUGIN% --lib -- --test-threads=4
) else (
    cargo test -p %PLUGIN% %CUSTOM%
)
set EXIT=%ERRORLEVEL%
echo EXIT=%EXIT%
exit /b %EXIT%
