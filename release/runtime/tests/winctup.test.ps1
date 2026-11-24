#requires -Version 5.1
# Offline tests for the Windows side of winctup. No network, no elevation, no release assets.
param()

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$root = (Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$tmp = Join-Path ([IO.Path]::GetTempPath()) ('winctup-tests-' + [guid]::NewGuid().ToString('N'))
$testHome = Join-Path $tmp 'home'
$testBin = Join-Path $tmp 'bin'

function Assert-Equal([object]$Expected, [object]$Actual, [string]$Message) {
    if ($Expected -ne $Actual) { throw "not ok - $Message (expected '$Expected', got '$Actual')" }
    Write-Host "ok - $Message"
}

try {
    New-Item -ItemType Directory -Force -Path $testHome, $testBin | Out-Null
    # The invocation guard in winctup.ps1 keeps its command dispatcher from running here.
    . (Join-Path $root 'release/runtime/winctup.ps1') -HomeDir $testHome -BinDir $testBin

    Assert-Equal $true (Test-Tag 'v1.2.3' 'stable') 'stable accepts a release tag'
    Assert-Equal $false (Test-Tag 'v1.2.3-beta.1' 'stable') 'stable rejects a prerelease tag'
    Assert-Equal $true (Test-Tag 'v1.2.3-beta.1' 'beta') 'beta accepts a prerelease tag'
    Assert-Equal $false (Test-Tag 'latest' 'beta') 'an arbitrary name is not a tag'

    Set-State 'VERSION' '1.2.3'
    Set-State 'PREVIOUS' '1.2.2'
    Set-State 'VERSION' '1.2.4'
    Assert-Equal '1.2.4' (Get-State 'VERSION') 'state keys can be replaced'
    Assert-Equal '1.2.2' (Get-State 'PREVIOUS') 'unrelated state keys survive a replacement'

    Set-Current '1.2.3'
    Write-Launchers
    Assert-Equal '1.2.3' (Get-Content -LiteralPath (Join-Path $testHome 'current') -Raw).Trim() 'current holds the active version'
    $launcher = Get-Content -LiteralPath (Join-Path $testBin 'winct.cmd') -Raw
    if ($launcher -notlike "*$testHome*" -or $launcher -notlike '*current*' -or
        $launcher -notlike '*releases\%WINCTT_VERSION%\winc.exe*' -or $launcher -like '*\app\winc.exe*') {
        throw 'not ok - the launcher resolves the active release'
    }
    Assert-Equal $true (Test-Path -LiteralPath (Join-Path $testBin 'winc.cmd')) 'the compatibility launcher exists'
    $commandLauncher = Get-Content -LiteralPath (Join-Path $testBin 'winctup.cmd') -Raw
    if ($commandLauncher -notlike '*-Command "%WINCTT_CMD%" %1*') {
        throw 'not ok - the command launcher forwards the first option after SHIFT'
    }

    foreach ($version in @('1.0.0', '1.1.0', '1.2.0', '1.2.1-beta.1', '1.3.0')) {
        New-Item -ItemType Directory -Force -Path (Join-Path $testHome "releases\$version") | Out-Null
    }
    $KEEP = 1
    Prune-Releases '1.3.0' '1.2.0'
    Assert-Equal $true (Test-Path -LiteralPath (Join-Path $testHome 'releases\1.2.0')) 'pruning keeps the previous release'
    Assert-Equal $false (Test-Path -LiteralPath (Join-Path $testHome 'releases\1.1.0')) 'pruning removes older releases'
    Assert-Equal $false (Test-Path -LiteralPath (Join-Path $testHome 'releases\1.2.1-beta.1')) 'pruning also removes prereleases'

    # Rollback memakai winc.exe di root release, bukan releases/<versi>/app/.
    New-Item -ItemType File -Force -Path (Join-Path $testHome 'releases\1.2.0\winc.exe') | Out-Null
    Set-State 'VERSION' '1.3.0'
    Set-State 'PREVIOUS' '1.2.0'
    Do-Rollback
    Assert-Equal '1.2.0' (Get-Content -LiteralPath (Join-Path $testHome 'current') -Raw).Trim() 'rollback restores the previous release'
    Assert-Equal '' (Get-State 'ARTIFACT') 'rollback clears the stale artifact state'

    $checksums = Join-Path $tmp 'SHA256SUMS'
    $digest = (Get-FileHash -LiteralPath (Join-Path $testHome 'state') -Algorithm SHA256).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText($checksums, "$digest  winctup.ps1`n")
    Assert-Equal $digest (Get-ExpectedHash $checksums 'winctup.ps1') 'checksum parsing finds the named asset'
} finally {
    Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
