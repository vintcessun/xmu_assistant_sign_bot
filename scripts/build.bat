@echo off
REM 探测合适的编译器并执行 cargo build --release 的便捷入口（cmd 包装器）。
REM 用法:  scripts\build.bat            构建 release
REM        scripts\build.bat debug      构建 debug
setlocal
set "SCRIPT_DIR=%~dp0"
where pwsh >nul 2>&1
if %ERRORLEVEL%==0 (
    set "PS=pwsh"
) else (
    set "PS=powershell"
)

set "PROFILE=release"
if /I "%~1"=="debug" set "PROFILE=debug"
if /I "%~1"=="release" set "PROFILE=release"

"%PS%" -NoProfile -ExecutionPolicy Bypass -File "%SCRIPT_DIR%build.ps1" -Profile %PROFILE%
exit /b %ERRORLEVEL%
