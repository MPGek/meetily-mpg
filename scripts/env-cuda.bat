@echo off
REM Sets CUDA environment variables for Meetily development.
REM
REM Run this in a cmd.exe session from frontend (or any dir that can reach scripts\):
REM   call scripts\env-cuda.bat
REM   call ..\scripts\env-cuda.bat
REM
REM Idempotent: safe to call repeatedly (PATH entries are not duplicated).
REM
REM UPDATE THE PATH BELOW IF THE CUDA TOOLKIT VERSION CHANGES.
set "CUDA_ROOT=C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.3"

set "CUDA_PATH=%CUDA_ROOT%"
set "CUDA_PATH_V13_3=%CUDA_ROOT%"
set "CUDA_MODULE_LOADING=LAZY"

REM Prepend CUDA bin directories to PATH only if not already present.
REM Order matters: add \bin before \bin\x64 so the substring check stays correct.
echo "%PATH%" | findstr /i /c:"%CUDA_ROOT%\bin;" >nul 2>&1
if errorlevel 1 set "PATH=%CUDA_ROOT%\bin;%PATH%"

echo "%PATH%" | findstr /i /c:"%CUDA_ROOT%\bin\x64;" >nul 2>&1
if errorlevel 1 set "PATH=%CUDA_ROOT%\bin\x64;%PATH%"

REM Use the Ninja generator for llama.cpp (llama-helper) CMake builds when
REM available: CUDA is driven by nvcc directly, so the NVIDIA CUDA platform
REM toolset registered inside Visual Studio is not required (not yet shipped
REM for VS 2026). Remove once the CUDA VS integration supports VS 2026.
where ninja >nul 2>&1
if not errorlevel 1 set "CMAKE_GENERATOR=Ninja"
