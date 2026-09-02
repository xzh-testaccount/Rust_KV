$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
$frontendRoot = Join-Path $repoRoot "prototype"

Push-Location $frontendRoot
try {
    if (-not (Test-Path "node_modules")) {
        $npmArgs = @(
            "ci",
            "--offline=false",
            "--no-audit",
            "--no-fund",
            "--registry=https://registry.npmjs.org",
            "--replace-registry-host=always"
        )
        & npm @npmArgs
        if ($LASTEXITCODE -ne 0) {
            throw "Frontend dependency installation failed"
        }
    }
    npm run dev
}
finally {
    Pop-Location
}
