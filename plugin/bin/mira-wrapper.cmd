@echo off
:: Mira Windows compatibility shim.
::
:: On Windows, cmd.exe cannot execute POSIX shell scripts (#!/bin/sh) directly.
:: When Claude Code plugin hooks invoke
::   "${CLAUDE_PLUGIN_ROOT}/bin/mira-wrapper hook <event>"
:: Windows produces "'mira-wrapper' is not recognized as an internal or
:: external command", blocking all Mira hooks.
::
:: This .cmd file is auto-resolved by Windows when no extension is given,
:: so hooks.json requires no changes.
::
:: Priority:
::   1. bash on PATH   — full wrapper logic (auto-download, auto-update)
::   2. mira.exe found — call it directly (fast path, no update check)
::   3. Neither        — print a helpful error and exit 1

setlocal EnableDelayedExpansion

set "SCRIPT_DIR=%~dp0"
set "MIRA_HOME=%USERPROFILE%\.mira"
set "MIRA_BIN=%MIRA_HOME%\bin\mira.exe"

:: Prefer bash — preserves auto-download and update logic from mira-wrapper
where bash >nul 2>&1
if !errorlevel! equ 0 (
    bash "%SCRIPT_DIR%mira-wrapper" %*
    exit /b !errorlevel!
)

:: Bash not found — fall back to installed binary if present
if exist "%MIRA_BIN%" (
    "%MIRA_BIN%" %*
    exit /b !errorlevel!
)

echo [mira] Error: bash.exe not found and mira.exe not installed. 1>&2
echo [mira] Install Git for Windows ^(https://gitforwindows.org/^) so bash is on PATH, 1>&2
echo [mira] or download mira manually from https://github.com/ConaryLabs/Mira/releases 1>&2
echo [mira] and place it at: %MIRA_BIN% 1>&2
exit /b 1
