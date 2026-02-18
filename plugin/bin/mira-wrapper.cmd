@echo off
:: Mira Windows compatibility shim.
::
:: On Windows, cmd.exe cannot directly execute POSIX shell scripts (#!/bin/sh).
:: This .cmd file is auto-resolved when hooks.json calls
::   "${CLAUDE_PLUGIN_ROOT}/bin/mira-wrapper hook <event>"
:: because Windows tries .cmd / .bat / .exe extensions automatically.
::
:: If bash (Git Bash, WSL, or similar) is on PATH, we delegate to the POSIX
:: wrapper so auto-download and update logic works normally.
::
:: If bash is not found but the mira binary is already installed, we call it
:: directly (safe because the wrapper's sole job is finding/updating mira.exe).

setlocal EnableDelayedExpansion

set "SCRIPT_DIR=%~dp0"
set "MIRA_HOME=%USERPROFILE%\.mira"
set "MIRA_BIN=%MIRA_HOME%\bin\mira.exe"

:: Prefer bash — preserves auto-download and update logic
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
