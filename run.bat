@echo off
REM SRLTCP v0.2.1 — Download and Run (Windows)
setlocal EnableDelayedExpansion

cd /d "%~dp0"

set PID_FILE=%~dp0.srltcp.pid
set LOG_FILE=%~dp0.srltcp.log
set QUIC_PORT=%SRLTCP_PORT%
if "%QUIC_PORT%"=="" set QUIC_PORT=9473

echo.
echo   ========================================
echo        SRLTCP v0.2.1 - Desktop
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

REM ── Build if needed ──────────────────────────────────────────────
set BINARY=target\release\srltcp-desktop.exe
if not exist "%BINARY%" (
    echo [SRLTCP] First run - building ^(may take a few minutes^)...
    cargo build --release -p srltcp-desktop >> "%LOG_FILE%" 2>&1
    if %ERRORLEVEL% neq 0 (
        echo [SRLTCP] Build failed. Check %LOG_FILE%
        exit /b 1
    )
    echo [SRLTCP] Build complete.
) else (
    echo [SRLTCP] Binary found - skipping build.
)

REM ── Launch in foreground (Ctrl+C goes directly to app for graceful shutdown) ──
echo [SRLTCP] Launching...
echo [SRLTCP] Press Ctrl+C to shut down gracefully.
echo.
set RUST_LOG=info
if exist "%BINARY%" (
    "%BINARY%"
) else (
    cargo run --release -p srltcp-desktop
)

echo [SRLTCP] Shutdown complete - ports and resources released.
exit /b 0