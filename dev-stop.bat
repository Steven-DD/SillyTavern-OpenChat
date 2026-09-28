@echo off
setlocal

echo [i] Stopping st-chat dev family (App / vite / tauri-cli / SillyTavern) ...
echo.

rem Match by command-line signature, so other node apps on this machine are NOT touched:
rem   st-chat.exe            = the App itself
rem   tauri.js ... dev       = tauri CLI
rem   D:\st-chat-dev\app...  = vite / npm run dev
rem   server.js --port 8000  = the SillyTavern instance managed by this App
rem (cmd.exe excluded so the script never kills its own console host)
powershell -NoProfile -Command "Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -and $_.Name -ne 'cmd.exe' -and ($_.CommandLine -match 'st-chat\.exe|tauri\.js.+dev|st-chat-dev\\app|server\.js --port 8000') } | ForEach-Object { $n=$_.CommandLine; if($n.Length -gt 90){$n=$n.Substring(0,90)}; Write-Host ('  kill ' + $_.ProcessId + '  ' + $n); taskkill /PID $_.ProcessId /T /F >$null 2>&1 }"

echo.
echo [ok] Stopped. Note: force-kill is a "crash exit" for SillyTavern, so the st.pid
echo      ledger stays behind on purpose. The next dev-start will auto-reap that
echo      leftover instance before launching a fresh one (see app console log).
pause
endlocal
