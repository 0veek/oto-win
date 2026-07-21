# Wrapper: load MSVC/LLVM env, then run the Tauri CLI.
# Usage from repo root:
#   npm run tauri:dev
#   npm run tauri:build
#   powershell -File scripts/tauri.ps1 build

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

. "$PSScriptRoot\dev-env.ps1"

# Ensure cargo sees LLVM even if VsDevCmd rearranged PATH
if (Test-Path "C:\Program Files\LLVM\bin\libclang.dll") {
    $env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"
    if ($env:Path -notlike "*C:\Program Files\LLVM\bin*") {
        $env:Path = "C:\Program Files\LLVM\bin;$env:Path"
    }
}

if (-not (Test-Path (Join-Path $env:LIBCLANG_PATH "libclang.dll"))) {
    Write-Error "libclang.dll not found at '$env:LIBCLANG_PATH'. Install LLVM: winget install --id LLVM.LLVM -e"
    exit 1
}

# Never use WHISPER_DONT_GENERATE_BINDINGS on Windows (bundled bindings are Unix).
Remove-Item Env:WHISPER_DONT_GENERATE_BINDINGS -ErrorAction SilentlyContinue

$tauriJs = Join-Path $root "node_modules\@tauri-apps\cli\tauri.js"
if (-not (Test-Path $tauriJs)) {
    Write-Error "Tauri CLI not found at $tauriJs - run npm install first."
    exit 1
}

if ($args.Count -eq 0) {
    & node $tauriJs --help
    exit $LASTEXITCODE
}

$argLine = $args -join " "
Write-Host "LIBCLANG_PATH=$env:LIBCLANG_PATH"
Write-Host "Running: node tauri.js $argLine"
# Call the CLI via node so PowerShell argument splatting works on Windows.
& node $tauriJs @args
exit $LASTEXITCODE
