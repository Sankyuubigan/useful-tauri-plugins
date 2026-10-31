@echo off
setlocal enableextensions

REM Plugin test.bat: unit-tests of tauri-plugin-llama-engine.
REM Same VS-init ritual as the project's test.bat (vswhere -> vcvarsall x64),
REM because cargo builds MSVC C-deps (ring, libz-sys...) that need cl.exe.
REM The toolkit "test" expects <root>\src-tauri, but this plugin keeps
REM Cargo.toml at its root, so we call cargo test directly AFTER VC init.

set "PROJ=%~dp0"
if not exist "%PROJ%Cargo.toml" (
  echo [ERROR] This script must live in the plugin root, next to Cargo.toml
  pause
  exit /b 1
)

set "VS_INIT_OK="
for /f "usebackq delims=" %%i in (`"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -property installationPath 2^>nul`) do (
    if exist "%%i\VC\Auxiliary\Build\vcvarsall.bat" (
        call "%%i\VC\Auxiliary\Build\vcvarsall.bat" x64 >nul 2>&1
        set "VS_INIT_OK=1"
    )
)
if not defined VS_INIT_OK (
  echo [ERROR] Visual Studio not found. vswhere. Install VS with C++ workload.
  pause
  exit /b 1
)

set "CC="
set "CXX="
set "CMAKE_C_COMPILER_LAUNCHER="
set "CMAKE_CXX_COMPILER_LAUNCHER="
set "RUSTC_WRAPPER="
set "CARGO_BUILD_RUSTC_WRAPPER="

set "CARGO_PROFILE_RELEASE_LTO="
set "CARGO_PROFILE_RELEASE_CODEGEN_UNITS="
set "CARGO_PROFILE_RELEASE_STRIP="

sccache --start-server >nul 2>&1

cd /d "%PROJ%"
cargo test %*
if errorlevel 1 (
  echo [ERROR] Tests failed.
  pause
  exit /b 1
)

echo [+DONE] plugin test.bat finished.
endlocal
