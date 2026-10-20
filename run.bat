@echo off
REM SRLTCP — Build (or download) and Run (Windows)
setlocal EnableDelayedExpansion

cd /d "%~dp0"

set PID_FILE=%~dp0.srltcp.pid
set LOG_FILE=%~dp0.srltcp.log
set REPO=narl3yyy-svg/SRLTCPv2
set PLATFORM=windows-x86_64

for /f "tokens=2 delims== " %%v in ('findstr /r "^version" Cargo.toml') do set VERSION=%%v
set VERSION=%VERSION:"=%
set VERSION=%VERSION: =%

echo.
echo   ========================================
echo        SRLTCP v%VERSION% - Desktop
echo    Secure P2P over Serial/LAN/WAN
echo   ========================================
echo.

REM ── Check stale process ──────────────────────────────────────────
if exist "%PID_FILE%" (
    set /p OLD_PID=<"%PID_FILE%"
    tasklist /FI "PID eq !OLD_PID!" 2>nul | find "!OLD_PID!" >nul
    if !ERRORLEVEL! equ 0 (
        echo [SRLTCP] ERROR: Already running ^(PID !OLD_PID!^). Run cleanup.bat first.
        exit /b 1
    )
    del "%PID_FILE%"
)

set BINARY=
set PREBUILT=dist\bin\%PLATFORM%\srltcp-desktop.exe
set FORCE_REBUILD=0
set USE_PREBUILT=1

set FORCE_PREBUILT=0
if "%1"=="--rebuild" set FORCE_REBUILD=1
if "%1"=="--no-prebuilt" set USE_PREBUILT=0
if "%1"=="--prebuilt" set FORCE_PREBUILT=1

if "%FORCE_REBUILD%"=="1" goto :build
REM Default: build this checkout when Rust is installed, so you run the current source
if "%FORCE_PREBUILT%"=="0" (
    where cargo >nul 2>&1
    if !ERRORLEVEL! equ 0 goto :build
)

if "%USE_PREBUILT%"=="1" (
    if exist "%PREBUILT%" (
        set BINARY=%PREBUILT%
        echo [SRLTCP] Using local prebuilt binary.
        goto :launch
    )
    echo [SRLTCP] Trying prebuilt binary for %PLATFORM%...
    if not exist "dist\bin\%PLATFORM%" mkdir "dist\bin\%PLATFORM%"
    curl -fsSL --retry 2 -o "%PREBUILT%" "https://github.com/%REPO%/releases/download/v%VERSION%/srltcp-desktop-%PLATFORM%.exe"
    if !ERRORLEVEL! equ 0 (
        set BINARY=%PREBUILT%
        echo [SRLTCP] Downloaded prebuilt binary.
        goto :launch
    )
    del "%PREBUILT%" 2>nul
)

echo [SRLTCP] ERROR: No prebuilt binary found for %PLATFORM%.
echo [SRLTCP] Download v%VERSION% from: https://github.com/%REPO%/releases/tag/v%VERSION%
echo [SRLTCP] Or compile from source: run.bat --rebuild
exit /b 1

:build
where cargo >nul 2>&1
if %ERRORLEVEL% neq 0 (
    echo [SRLTCP] Rust not found. Install from https://rustup.rs
    exit /b 1
)

echo [SRLTCP] Building ^(first build takes several minutes^)...
cargo build --release -p srltcp-desktop
if %ERRORLEVEL% neq 0 (
    echo [SRLTCP] Build failed - see the compiler output above.
    exit /b 1
)
echo [SRLTCP] Build complete.
set BINARY=target\release\srltcp-desktop.exe

:launch
if not exist "%BINARY%" (
    echo [SRLTCP] ERROR: Binary not found at %BINARY%
    exit /b 1
)

echo [SRLTCP] Launching: %BINARY%
echo [SRLTCP] Close the window or press Ctrl+C to shut down.
echo.
set RUST_LOG=info
"%BINARY%"
echo [SRLTCP] Shutdown complete.
exit /b 0