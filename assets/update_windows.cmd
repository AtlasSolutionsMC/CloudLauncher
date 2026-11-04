@echo off
setlocal DisableDelayedExpansion
set "pid=%~1"
set "target=%~2"
set "replacement=%~3"
:wait_for_launcher
tasklist /FI "PID eq %pid%" /NH | findstr /C:"%pid%" >nul
if not errorlevel 1 (timeout /t 1 /nobreak >nul & goto wait_for_launcher)
set /a attempts=0
:replace_file
move /Y "%replacement%" "%target%" >nul
if not errorlevel 1 goto restart_launcher
set /a attempts+=1
if %attempts% GEQ 20 exit /b 1
timeout /t 1 /nobreak >nul
goto replace_file
:restart_launcher
start "" "%target%"
del "%~f0"
