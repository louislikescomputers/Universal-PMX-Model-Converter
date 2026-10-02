@echo off
rem compile.bat - Build mmdconv for Windows (cmd.exe)
rem
rem Usage:
rem   compile.bat            release build (default)
rem   compile.bat debug      debug build
rem   compile.bat tests      run test suite first, then release build
rem
rem Output binary is copied to .\dist\mmdconv.exe

setlocal enabledelayedexpansion
pushd "%~dp0"

set "PROFILE=release"
set "PROFILE_FLAG=--release"
set "RUN_TESTS=0"

if /I "%~1"=="debug" (
    set "PROFILE=debug"
    set "PROFILE_FLAG="
)
if /I "%~1"=="tests" (
    set "RUN_TESTS=1"
)

where cargo >nul 2>&1
if errorlevel 1 (
    echo ERROR: 'cargo' was not found on PATH.
    echo Install Rust from https://rustup.rs and reopen the terminal.
    popd
    exit /b 1
)

echo == mmdconv build (Windows) ==
echo    profile : %PROFILE%

for /f "delims=" %%V in ('cargo --version') do echo    toolchain: %%V

if "%RUN_TESTS%"=="1" (
    echo.
    echo -- test suite --
    cargo test --workspace %PROFILE_FLAG%
    if errorlevel 1 goto :fail
)

echo.
echo -- building mmdconv --
cargo build %PROFILE_FLAG% --bin mmdconv
if errorlevel 1 goto :fail

set "SRC=target\%PROFILE%\mmdconv.exe"
if not exist "%SRC%" goto :missing

if not exist dist mkdir dist
copy /y "%SRC%" "dist\mmdconv.exe" >nul
if errorlevel 1 goto :fail

for /f "delims=" %%H in ('certutil -hashfile "dist\mmdconv.exe" SHA256 ^| findstr /r /v "^[A-Za-z]"') do set "HASH=%%H"

echo.
echo BUILD OK
echo   binary : %~dp0dist\mmdconv.exe
echo   sha256 : %HASH%
popd
exit /b 0

:missing
echo.
echo BUILD FAILED: expected artifact not found: %SRC%
popd
exit /b 1

:fail
echo.
echo BUILD FAILED (see cargo output above)
popd
exit /b 1
