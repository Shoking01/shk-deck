@echo off
REM StreamCraft Controller Update Script
REM Compiles the latest version and updates the installed copy

title StreamCraft Controller Update
color 1F

echo.
echo ================================================
echo   StreamCraft Controller - Update Script
echo ================================================
echo.

set "PROJECT_ROOT=C:\Users\quiro\Downloads\Adrian\Trabajo\Portfolio\REA RD"
set "SOURCE=%PROJECT_ROOT%\crates\ss550-gui\target\release\ss550-gui.exe"
REM Use ss550 directory for compatibility with existing installations
set "INSTALL_DIR=%LOCALAPPDATA%\Programs\ss550"
set "INSTALL_FILE=%INSTALL_DIR%\ss550-gui.exe"

REM Change to project directory
cd /d "%PROJECT_ROOT%\crates\ss550-gui"
echo [INFO] Working directory: %CD%

REM Build release
echo [INFO] Building release version...
cargo build --release >nul 2>&1
if errorlevel 1 (
    echo [ERROR] Build failed!
    pause
    exit /b 1
)
echo [OK] Build completed successfully

REM Verify binary exists
if not exist "%SOURCE%" (
    echo [ERROR] Binary not found at: %SOURCE%
    pause
    exit /b 1
)
echo [OK] Binary found

REM Kill old process
tasklist /FI "IMAGENAME eq ss550-gui.exe" 2>NUL | find /I "ss550-gui.exe" >NUL
if %ERRORLEVEL% EQU 0 (
    echo [INFO] Stopping old instance...
    taskkill /F /IM ss550-gui.exe >nul 2>&1
    timeout /t 1 /nobreak >nul
    echo [OK] Old instance stopped
)

REM Create install directory
if not exist "%INSTALL_DIR%" (
    mkdir "%INSTALL_DIR%"
    echo [INFO] Created install directory
)

REM Copy new binary
echo [OK] Updating: %INSTALL_FILE%
copy /Y "%SOURCE%" "%INSTALL_FILE%" >nul

echo.
echo [OK] Update complete!
echo Location: %INSTALL_FILE%
echo.
echo To run now: "%INSTALL_FILE%"
echo To auto-start on login: Create a shortcut in %LOCALAPPDATA%\Microsoft\Windows\Start Menu\Programs\Startup
echo.

pause