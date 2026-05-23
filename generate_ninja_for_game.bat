@echo off

pushd "%~dp0"

if not defined ROOT_DIR       set "ROOT_DIR=%~dp0\.."
for %%I in ("%ROOT_DIR%")  do set "ROOT_DIR=%%~fI"

if not defined VOSTOK_DIR     set "VOSTOK_DIR=%ROOT_DIR%\vostok"

set "SLN_PATH=%VOSTOK_DIR%\sources\vostok v2.0.sln"

cargo run --release -- ^
  --sln-path "%SLN_PATH%" ^
  --configuration-platform "Master Gold|Win32" ^
  --project-name "survarium - PC - DirectX 11" 

  :: --project-name "game"

popd
