@echo off
REM SRLTCP — Download and Run (Windows)
setlocal EnableDelayedExpansion

cd /d "%~dp0"

set PID_FILE=%~dp0.srltcp.pid
set LOG_FILE=%~dp0.srltcp.log
set QUIC_PORT=%SRLTCP_PORT%
if "%QUIC_PORT%"=="" set QUIC_PORT=9473
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

set BINARY=target\release\srltcp-desktop.exe
set PREBUILT=dist\bin\%PLATFORM%\srltcp-desktop.exe
set FORCE_REBUILD=0
set USE_PREBUILT=1

if "%1"=="--rebuild" set FORCE_REBUILD=1
if "%1"=="--no-prebuilt" set USE_PREBUILT=0

if "%FORCE_REBUILD%"=="1" goto :build

if "%USE_PREBUILT%"=="1" (
    if exist "%PREBUILT%" (
        set BINARY=%PREBUILT%
        echo [SRLTCP] Using local prebuilt binary.
        goto :launch
    )
    if exist "%BINARY%" (
        echo [SRLTCP] Binary found - skipping download/build.
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
    echo [SRLTCP] Prebuilt not available - will try local build.
)

if exist "%BINARY%" (
    echo [SRLTCP] Binary found - skipping build.
    goto :launch
)

:build
where cargo >nul 2>&1
if %ERRORLEVEL% neq 0 (
    echo [SRLTCP] Rust not found. Install from https://rustup.rs or use a release with prebuilt binaries.
    exit /b 1
)

echo [SRLTCP] Building ^(may take a few minutes^)...
cargo build --release -p srltcp-desktop >> "%LOG_FILE%" 2>&1
if %ERRORLEVEL% neq 0 (
    echo [SRLTCP] Build failed. Check %LOG_FILE%
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