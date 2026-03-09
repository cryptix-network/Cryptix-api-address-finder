@echo off
setlocal
cd /d "%~dp0"

echo Starting Cryptix API Address Finder...
echo.

if exist "target\release\cryptix-address-finder.exe" (
    "target\release\cryptix-address-finder.exe"
) else (
    cargo run --release
)

echo.
echo Program finished. Press any key to close this window.
pause >nul
