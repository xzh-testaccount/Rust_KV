param(
    [string]$Toolchain = "stable-x86_64-pc-windows-gnu",
    [string]$ControllerBind = "127.0.0.1:7879",
    [string]$ServerBind = "127.0.0.1:7878",
    [string]$WalPath = "data/kv.wal"
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repoRoot
try {
    & cargo "+$Toolchain" build --bins
    if ($LASTEXITCODE -ne 0) {
        throw "Rust build failed"
    }

    $controller = Join-Path $repoRoot "target\debug\kv-controller.exe"
    $controllerArgs = @(
        "--bind", $ControllerBind,
        "--server-bind", $ServerBind,
        "--data", $WalPath
    )
    & $controller @controllerArgs
}
finally {
    Pop-Location
}
