$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repoRoot
try {
    cargo run -p xtask -- icons target\icon-preview
    if ($LASTEXITCODE -ne 0) {
        throw "Icon generation failed with exit code $LASTEXITCODE."
    }

    Copy-Item target\icon-preview\winct.ico crates\winct-app\assets\winct.ico -Force

    cargo run -p winct
    if ($LASTEXITCODE -ne 0) {
        throw "Winct exited with code $LASTEXITCODE."
    }
}
finally {
    Pop-Location
}
