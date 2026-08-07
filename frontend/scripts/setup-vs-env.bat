@echo off
REM Meetily shared Visual Studio environment setup for Windows
REM Locates vcvars64.bat (vswhere first, then standard VS 2022/2026 install
REM paths), configures the MSVC + Windows SDK environment, and verifies the
REM toolchain. Exits with a non-zero code when no usable VS C++ toolchain is
REM found. Must be invoked with `call` so environment changes propagate to
REM the caller.

set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
set "VCVARS="
set "VS_INSTALL="

REM --- 1. Locate vcvars64.bat via vswhere (newest install with C++ tools) ---
if exist "%VSWHERE%" (
    echo 🔧 Locating Visual Studio via vswhere...
    for /f "usebackq delims=" %%i in (`"%VSWHERE%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -find VC\Auxiliary\Build\vcvars64.bat`) do set "VCVARS=%%i"
)

REM --- 2. Fallback: probe standard install paths (VS 2026 first, then VS 2022) ---
if not defined VCVARS (
    echo 🔧 Visual Studio not found via vswhere, probing standard install paths...
    call :probe "C:\Program Files\Microsoft Visual Studio\2026\BuildTools"
    call :probe "C:\Program Files (x86)\Microsoft Visual Studio\2026\BuildTools"
    call :probe "C:\Program Files\Microsoft Visual Studio\2026\Community"
    call :probe "C:\Program Files (x86)\Microsoft Visual Studio\2026\Community"
    call :probe "C:\Program Files\Microsoft Visual Studio\2026\Professional"
    call :probe "C:\Program Files (x86)\Microsoft Visual Studio\2026\Professional"
    call :probe "C:\Program Files\Microsoft Visual Studio\2026\Enterprise"
    call :probe "C:\Program Files (x86)\Microsoft Visual Studio\2026\Enterprise"
    call :probe "C:\Program Files\Microsoft Visual Studio\2022\BuildTools"
    call :probe "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools"
    call :probe "C:\Program Files\Microsoft Visual Studio\2022\Community"
    call :probe "C:\Program Files (x86)\Microsoft Visual Studio\2022\Community"
    call :probe "C:\Program Files\Microsoft Visual Studio\2022\Professional"
    call :probe "C:\Program Files (x86)\Microsoft Visual Studio\2022\Professional"
    call :probe "C:\Program Files\Microsoft Visual Studio\2022\Enterprise"
    call :probe "C:\Program Files (x86)\Microsoft Visual Studio\2022\Enterprise"
)

if not defined VCVARS (
    echo.
    echo ❌ Error: No usable Visual Studio C++ toolchain found.
    echo    Install Visual Studio 2022 or 2026 with the Desktop development
    echo    with C++ workload, then re-run this script.
    exit /b 1
)

echo ✅ Visual Studio environment found: "%VCVARS%"
call "%VCVARS%" >nul 2>&1

REM Derive the installation root from vcvars64.bat's location
for %%I in ("%VCVARS%") do set "VS_INSTALL=%%~dpI"
for %%I in ("%VS_INSTALL%..\..\..") do set "VS_INSTALL=%%~fI"

REM --- 3. Version-agnostic manual environment (newest MSVC toolset + SDK) ---
set "MSVC_VERSION="
set "SDK_VERSION="
for /f "delims=" %%D in ('dir /b /o-n "%VS_INSTALL%\VC\Tools\MSVC\*" 2^>nul') do if not defined MSVC_VERSION set "MSVC_VERSION=%%D"
for /f "delims=" %%D in ('dir /b /o-n "%ProgramFiles(x86)%\Windows Kits\10\Lib\*" 2^>nul') do if not defined SDK_VERSION set "SDK_VERSION=%%D"

set "MSVC_BASE=%VS_INSTALL%\VC\Tools\MSVC\%MSVC_VERSION%"
set "SDK_BASE=%ProgramFiles(x86)%\Windows Kits\10"

set "LIB=%MSVC_BASE%\lib\x64;%SDK_BASE%\Lib\%SDK_VERSION%\um\x64;%SDK_BASE%\Lib\%SDK_VERSION%\ucrt\x64"
set "INCLUDE=%MSVC_BASE%\include;%SDK_BASE%\Include\%SDK_VERSION%\um;%SDK_BASE%\Include\%SDK_VERSION%\shared;%SDK_BASE%\Include\%SDK_VERSION%\ucrt"
set "PATH=%MSVC_BASE%\bin\HostX64\x64;%SDK_BASE%\bin\%SDK_VERSION%\x64;%PATH%"

echo LIB path: %LIB%
echo INCLUDE path: %INCLUDE%

REM --- 4. Verify critical libraries exist ---
if exist "%SDK_BASE%\Lib\%SDK_VERSION%\um\x64\kernel32.lib" (
    echo ✓ kernel32.lib found
) else (
    echo ✗ kernel32.lib NOT found - Windows SDK issue
)

if exist "%MSVC_BASE%\lib\x64\msvcrt.lib" (
    echo ✓ msvcrt.lib found in Visual Studio MSVC
) else (
    echo ✗ msvcrt.lib NOT found - C++ runtime issue
)

exit /b 0

:probe
if defined VCVARS exit /b 0
if exist "%~1\VC\Auxiliary\Build\vcvars64.bat" (
    set "VCVARS=%~1\VC\Auxiliary\Build\vcvars64.bat"
    set "VS_INSTALL=%~1"
)
exit /b 0
