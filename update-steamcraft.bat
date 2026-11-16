@echo off
REM StreamCraft Controller update script.
REM Builds the release binary and installs it over the copy Windows starts.
REM The work is in update-steamcraft.ps1; this wrapper only exists because a
REM batch file is what people double-click.

title StreamCraft Controller Update

powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0update-steamcraft.ps1" %*
set "RESULT=%ERRORLEVEL%"

echo.
if not "%RESULT%"=="0" (
    echo [ERROR] Update failed with exit code %RESULT%.
) else (
    echo [OK] Update finished.
)
pause
exit /b %RESULT%
