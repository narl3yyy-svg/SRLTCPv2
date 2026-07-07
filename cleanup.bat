@echo off
REM SRLTCP v0.2.0 — Full cleanup (Windows)
setlocal EnableDelayedExpansion
cd /d "%~dp0"

set PID_FILE=%~dp0.srltcp.pid

echo [cleanup] SRLTCP full cleanup starting...

if exist "%PID_FILE%" (
    set /p APP_PID=<"%PID_FILE%"
    echo [cleanup] Stopping process !APP_PID!...
    taskkill /PID !APP_PID! >nul 2>&1
    timeout /t 3 /nobreak >nul
    taskkill /PID !APP_PID! /T /F >nul 2>&1
    del "%PID_FILE%"
)

taskkill /IM srltcp-desktop.exe /F >nul 2>&1

if exist "%~dp0.srltcp.log" del "%~dp0.srltcp.log"
if exist "%~dp0.srltcp-tmp" rmdir /s /q "%~dp0.srltcp-tmp" 2>nul

echo [cleanup] Android: use App Info -^> Force Stop to fully stop background service.
echo [cleanup] Cleanup complete.
exit /b 0