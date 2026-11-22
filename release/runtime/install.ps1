#requires -Version 5.1
<#
One-command Winct bootstrap:
  irm https://raw.githubusercontent.com/Aleverio/Winc/dev/release/runtime/install.ps1 | iex

The runtime is downloaded and checked against CHECKSUMS before it is allowed to run.
#>
[CmdletBinding()]
param(
    [string]$Channel,
    [string]$Repo,
    [string]$Ref,
    [string]$HomeDir,
    [string]$BinDir
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

if (-not $Repo) { $Repo = if ($env:WINCT_RELEASE_REPO) { $env:WINCT_RELEASE_REPO } else { 'Aleverio/Winc' } }
if (-not $Ref) { $Ref = if ($env:WINCT_RUNTIME_REF) { $env:WINCT_RUNTIME_REF } else { 'dev' } }
if (-not $HomeDir) {
    if ($env:WINCT_HOME) { $HomeDir = $env:WINCT_HOME }
    elseif ($env:LOCALAPPDATA) { $HomeDir = Join-Path $env:LOCALAPPDATA 'winct' }
    else { $HomeDir = Join-Path (Join-Path $HOME '.local/share') 'winct' }
}
if (-not $BinDir) { $BinDir = if ($env:WINCT_BIN) { $env:WINCT_BIN } else { Join-Path $HomeDir 'bin' } }

$RawBase = "https://raw.githubusercontent.com/$Repo/$Ref/release/runtime"
$Tmp = Join-Path ([IO.Path]::GetTempPath()) ('winctup-bootstrap-' + [guid]::NewGuid().ToString('N'))

function Fail([string]$Message) { throw "winctup bootstrap: $Message" }

try {
    if ($Repo -notmatch '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$') { Fail 'WINCT_RELEASE_REPO must be owner/repo' }
    if ($Ref -notmatch '^[A-Za-z0-9._/-]+$') { Fail 'WINCT_RUNTIME_REF has an invalid character' }
    New-Item -ItemType Directory -Force -Path $Tmp | Out-Null
    Invoke-WebRequest -Uri "$RawBase/CHECKSUMS" -OutFile (Join-Path $Tmp 'CHECKSUMS') -UseBasicParsing -TimeoutSec 60
    Invoke-WebRequest -Uri "$RawBase/winctup.ps1" -OutFile (Join-Path $Tmp 'winctup.ps1') -UseBasicParsing -TimeoutSec 60

    $expected = ''
    foreach ($line in Get-Content -LiteralPath (Join-Path $Tmp 'CHECKSUMS')) {
        if ($line -match '^\s*([0-9a-fA-F]{64})\s+\*?winctup\.ps1\s*$') {
            $expected = $Matches[1].ToLowerInvariant()
            break
        }
    }
    if (-not $expected) { Fail 'CHECKSUMS has no winctup.ps1 entry' }
    $actual = (Get-FileHash -LiteralPath (Join-Path $Tmp 'winctup.ps1') -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $expected) { Fail 'winctup.ps1 does not match CHECKSUMS' }

    $runtimeDir = Join-Path $HomeDir 'runtime'
    New-Item -ItemType Directory -Force -Path $runtimeDir | Out-Null
    $script = Join-Path $runtimeDir 'winctup.ps1'
    Copy-Item -LiteralPath (Join-Path $Tmp 'winctup.ps1') -Destination $script -Force
    Copy-Item -LiteralPath (Join-Path $Tmp 'CHECKSUMS') -Destination $runtimeDir -Force
    Write-Host 'winctup bootstrap: runtime verified; installing Winct'

    $runtimeArgs = @('-Command', 'install')
    if ($Channel) { $runtimeArgs += @('-Channel', $Channel) }
    if ($Repo) { $runtimeArgs += @('-Repo', $Repo) }
    if ($Ref) { $runtimeArgs += @('-Ref', $Ref) }
    if ($HomeDir) { $runtimeArgs += @('-HomeDir', $HomeDir) }
    if ($BinDir) { $runtimeArgs += @('-BinDir', $BinDir) }
    & $script @runtimeArgs
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
} finally {
    Remove-Item -LiteralPath $Tmp -Recurse -Force -ErrorAction SilentlyContinue
}
