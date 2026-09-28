@echo off
setlocal
cd /d "D:\st-chat-dev\app"

rem ---- pin Windows built-ins (avoid GNU find/timeout shadowing on some PATHs) ----
set "FIND=%SystemRoot%\System32\find.exe"
set "TASKLIST=%SystemRoot%\System32\tasklist.exe"

rem ---- already running? ----
"%TASKLIST%" /FI "IMAGENAME eq st-chat.exe" 2>nul | "%FIND%" /I "st-chat.exe" >nul
if %errorlevel%==0 (
    echo [!] st-chat dev is ALREADY running. Run dev-stop.bat first.
    pause
    exit /b 1
)

echo [i] Starting st-chat dev (minimized window, log: dev-run.log) ...
start "st-chat-dev" /min cmd /c "npx tauri dev > dev-run.log 2>&1"

rem wait ~5s (ping-based delay, no external dependency)
ping -n 6 127.0.0.1 >nul

"%TASKLIST%" /FI "IMAGENAME eq st-chat.exe" 2>nul | "%FIND%" /I "st-chat.exe" >nul
if %errorlevel%==0 (
    echo [ok] Started. App window will pop up. Log: D:\st-chat-dev\app\dev-run.log
) else (
    echo [..] Still compiling (takes longer after code changes). Check dev-run.log later.
)
pause
endlocal
