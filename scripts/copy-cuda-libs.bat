@echo off
REM Copies required CUDA runtime DLLs into the app's output folder (idempotent).
REM Thin wrapper around copy-cuda-libs.ps1.
REM
REM Usage:
REM   call scripts\copy-cuda-libs.bat
REM   call ..\scripts\copy-cuda-libs.bat   (from frontend)

powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0copy-cuda-libs.ps1"
exit /b %errorlevel%
