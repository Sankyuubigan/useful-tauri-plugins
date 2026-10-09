@echo off
rem Build the gateway binary into the workspace target dir.
rem Mirrors test-plugin.bat: find MSVC via vswhere, then call vcvarsall so the
rem linker (link.exe) is on PATH.
for /f "usebackq delims=" %%i in (`"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -property installationPath 2^>nul`) do (
    if exist "%%i\VC\Auxiliary\Build\vcvarsall.bat" (
        call "%%i\VC\Auxiliary\Build\vcvarsall.bat" x64 >nul 2>&1
        echo VS=%%i
    )
)
cargo build -p cloud-routers-gateway --release %*
echo EXIT=%ERRORLEVEL%
exit /b %ERRORLEVEL%