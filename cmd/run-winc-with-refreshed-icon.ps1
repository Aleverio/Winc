$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repoRoot
try {
    cargo run -p xtask -- icons target\icon-preview
    if ($LASTEXITCODE -ne 0) {
        throw "Icon generation failed with exit code $LASTEXITCODE."
    }

    Copy-Item target\icon-preview\winc.ico crates\winc-app\assets\winc.ico -Force

    cargo run -p winc
    if ($LASTEXITCODE -ne 0) {
        throw "Winc exited with code $LASTEXITCODE."
    }
}
finally {
    Pop-Location
}
