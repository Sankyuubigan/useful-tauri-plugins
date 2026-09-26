@echo off
setlocal enableextensions
set PLUGIN=%1
if "%PLUGIN%"=="" set PLUGIN=tauri-plugin-llama-engine

for /f "usebackq delims=" %%i in (`"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -property installationPath 2^>nul`) do (
    if exist "%%i\VC\Auxiliary\Build\vcvarsall.bat" (
        call "%%i\VC\Auxiliary\Build\vcvarsall.bat" x64 >nul 2>&1
        echo VS=%%i
    )
)
cargo test -p %PLUGIN% --lib -- --test-threads=4
set EXIT=%ERRORLEVEL%
echo EXIT=%EXIT%
exit /b %EXIT%
