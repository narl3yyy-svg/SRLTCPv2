@echo off
REM SRLTCP v0.2.3 — Download and Run (Windows)
setlocal EnableDelayedExpansion

cd /d "%~dp0"

set PID_FILE=%~dp0.srltcp.pid
set LOG_FILE=%~dp0.srltcp.log
set QUIC_PORT=%SRLTCP_PORT%
if "%QUIC_PORT%"=="" set QUIC_PORT=9473
set VERSION=0.2.3

echo.
echo   ========================================
echo        SRLTCP v%VERSION% - Desktop
echo    Secure P2P over Serial/LAN/WAN
echo   ========================================
echo.

REM ── Ensure Rust ──────────────────────────────────────────────────
where cargo >nul 2>&1
if %ERRORLEVEL% neq 0 (
    echo [SRLTCP] Rust not found. Installing via rustup...
    curl -sSf https://win.rustup.rs/x86_64 -o rustup-init.exe
    rustup-init.exe -y --default-toolchain stable
    del rustup-init.exe
    call "%USERPROFILE%\.cargo\env.bat"
)

echo [SRLTCP] Rust found.

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

REM ── Find binary (prebuilt or compiled) ───────────────────────────
set BINARY=target\release\srltcp-desktop.exe
set PREBUILT=dist\bin\windows-x86_64\srltcp-desktop.exe

if "%1"=="--rebuild" goto :build
if exist "%PREBUILT%" (
    set BINARY=%PREBUILT%
    echo [SRLTCP] Using prebuilt binary.
    goto :launch
)
if exist "%BINARY%" (
    echo [SRLTCP] Binary found - skipping build.
    goto :launch
)

:build
echo [SRLTCP] Building ^(may take a few minutes^)...
cargo build --release -p srltcp-desktop >> "%LOG_FILE%" 2>&1
if %ERRORLEVEL% neq 0 (
    echo [SRLTCP] Build failed. Check %LOG_FILE%
    exit /b 1
)
echo [SRLTCP] Build complete.
set BINARY=target\release\srltcp-desktop.exe

:launch
echo [SRLTCP] Launching...
echo [SRLTCP] Close the window or press Ctrl+C to shut down.
echo.
set RUST_LOG=info
"%BINARY%"
echo [SRLTCP] Shutdown complete.
exit /b 0