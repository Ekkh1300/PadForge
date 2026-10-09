@echo off
REM Builds the PadForge installer as a single self-contained .exe.
REM
REM The installer embeds padforge.exe, so the application has to be built first.
REM This script does both in the right order and drops the result in dist\.

setlocal enabledelayedexpansion

REM Cargo is not always on PATH (a fresh shell, or a machine where Rust was
REM installed per-user), so fall back to the usual install location.
where cargo > nul 2>&1
if errorlevel 1 if exist "%USERPROFILE%\.cargo\bin\cargo.exe" (
    set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
)

REM The GNU toolchain lives outside the repository on this machine, and Cargo
REM needs gcc, ar and dlltool on PATH to link.
set "EXTRA_PATH=C:\Users\Public\mingw64\bin"
if exist "%EXTRA_PATH%" set "PATH=%EXTRA_PATH%;%PATH%"

where cargo > nul 2>&1
if errorlevel 1 (
    echo Could not find cargo on PATH.
    echo Install Rust from https://rustup.rs and try again.
    exit /b 1
)

set "TARGET=x86_64-pc-windows-gnu"
set "ROOT=%~dp0"

echo === Building PadForge ===
cargo build --manifest-path "%ROOT%Cargo.toml" -p padforge --release --target %TARGET%
if errorlevel 1 goto :failed

echo.
echo === Building the installer ===
cargo build --manifest-path "%ROOT%Cargo.toml" -p padforge-installer --release --target %TARGET%
if errorlevel 1 goto :failed

set "BUILT=%ROOT%target\%TARGET%\release\padforge-installer.exe"
if not exist "%BUILT%" (
    echo Could not find %BUILT%
    goto :failed
)

echo.
if not exist "%ROOT%dist" mkdir "%ROOT%dist"
copy /Y "%BUILT%" "%ROOT%dist\PadForge-Setup.exe" > nul

echo.
echo Done.
echo   Installer : %ROOT%dist\PadForge-Setup.exe
for %%F in ("%ROOT%dist\PadForge-Setup.exe") do echo   Size      : %%~zF bytes
echo.
echo Run it with no arguments to install, or /S to install silently.
goto :end

:failed
echo.
echo Build failed.
exit /b 1

:end
endlocal