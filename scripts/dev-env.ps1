# Sets up the MSVC + CMake + libclang environment needed to build oto-win
# (especially whisper-rs-sys / whisper.cpp).
#
# Usage (from repo root):
#   . .\scripts\dev-env.ps1
#   npm run tauri dev

$ErrorActionPreference = "Stop"

function Add-PathFront([string]$dir) {
    if (-not [string]::IsNullOrWhiteSpace($dir) -and (Test-Path $dir)) {
        if ($env:Path -notlike "*$dir*") {
            $env:Path = "$dir;$env:Path"
        }
    }
}

# Rust
Add-PathFront "$env:USERPROFILE\.cargo\bin"

# LLVM / libclang for bindgen (whisper-rs-sys)
$llvmBin = "C:\Program Files\LLVM\bin"
if (Test-Path "$llvmBin\libclang.dll") {
    $env:LIBCLANG_PATH = $llvmBin
    Add-PathFront $llvmBin
    Write-Host "LIBCLANG_PATH=$env:LIBCLANG_PATH"
} else {
    Write-Warning "libclang.dll not found under $llvmBin. Install LLVM (winget install LLVM.LLVM)."
}

# Do NOT set WHISPER_DONT_GENERATE_BINDINGS on Windows — the crate's bundled
# bindings target Unix layouts and fail MSVC size checks. Always generate with
# bindgen + LIBCLANG_PATH.

# Visual Studio Build Tools environment (cl, link, Windows SDK)
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vsPath = $null
if (Test-Path $vswhere) {
    $vsPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null
}
if (-not $vsPath) {
    $vsPath = "C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools"
}

$vsDevCmd = Join-Path $vsPath "Common7\Tools\VsDevCmd.bat"
if (Test-Path $vsDevCmd) {
    Write-Host "Loading VS environment from $vsDevCmd"
    # Capture env after VsDevCmd for x64 host/target
    $tmp = [System.IO.Path]::GetTempFileName()
    cmd /c "`"$vsDevCmd`" -arch=amd64 -host_arch=amd64 >nul && set" | Out-File -FilePath $tmp -Encoding ascii
    Get-Content $tmp | ForEach-Object {
        if ($_ -match '^(.*?)=(.*)$') {
            $name = $matches[1]
            $value = $matches[2]
            # Don't clobber carefully set LIBCLANG_PATH
            if ($name -eq "LIBCLANG_PATH" -and $env:LIBCLANG_PATH) { return }
            [System.Environment]::SetEnvironmentVariable($name, $value, "Process")
        }
    }
    Remove-Item $tmp -Force -ErrorAction SilentlyContinue
} else {
    Write-Warning "VsDevCmd.bat not found. Install VS Build Tools with C++ workload."
}

# CMake from VS if system cmake is missing
if (-not (Get-Command cmake -ErrorAction SilentlyContinue)) {
    $vsCmake = Join-Path $vsPath "Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin"
    Add-PathFront $vsCmake
}

Write-Host "cmake: $((Get-Command cmake -ErrorAction SilentlyContinue).Source)"
Write-Host "cl:    $((Get-Command cl -ErrorAction SilentlyContinue).Source)"
Write-Host "Ready. Run: npm run tauri dev"
