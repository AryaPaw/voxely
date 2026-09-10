$ErrorActionPreference = "Stop"
$vcvars = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
if (-not (Test-Path $vcvars)) {
    throw "MSVC vcvars64.bat not found. Install Visual Studio 2022 Build Tools with C++."
}
cmd /c "call `"$vcvars`" && bun run tauri dev"
