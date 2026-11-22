#requires -Version 5.1
<#
winctup - the Winct install/update runtime for Windows.

Bootstrap:
  irm https://raw.githubusercontent.com/Aleverio/Winc/dev/release/runtime/install.ps1 | iex

The runtime resolves releases, verifies their SHA-256 against SHA256SUMS, expands the portable ZIP
into a staging directory, flips a small `current` version file atomically, keeps the previous release
for rollback, and prunes old ones. No administrator rights and no package repository are needed.
#>
[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [ValidateSet('install', 'update', 'status', 'rollback', 'self-update', 'uninstall', 'doctor', 'help')]
    [string]$Command = 'help',
    [string]$Channel,
    [string]$Repo,
    [string]$Ref,
    [string]$HomeDir,
    [string]$BinDir,
    [switch]$Yes
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$RUNTIME_VERSION = '0.1.0'
$KEEP = 3

if (-not $Repo) { $Repo = if ($env:WINCT_RELEASE_REPO) { $env:WINCT_RELEASE_REPO } else { 'Aleverio/Winc' } }
if (-not $Ref) { $Ref = if ($env:WINCT_RUNTIME_REF) { $env:WINCT_RUNTIME_REF } else { 'dev' } }
if (-not $Channel) { $Channel = if ($env:WINCT_CHANNEL) { $env:WINCT_CHANNEL } else { 'stable' } }
if (-not $HomeDir) {
    if ($env:WINCT_HOME) { $HomeDir = $env:WINCT_HOME }
    elseif ($env:LOCALAPPDATA) { $HomeDir = Join-Path $env:LOCALAPPDATA 'winct' }
    else { $HomeDir = Join-Path (Join-Path $HOME '.local/share') 'winct' }
}
if (-not $BinDir) { $BinDir = if ($env:WINCT_BIN) { $env:WINCT_BIN } else { Join-Path $HomeDir 'bin' } }

$RawBase = "https://raw.githubusercontent.com/$Repo/$Ref/release/runtime"

function Fail([string]$Message) { throw "winctup: $Message" }
function Info([string]$Message) { Write-Host "winctup: $Message" }

function Get-GitHubHeaders {
    $headers = @{ 'User-Agent' = 'winctup'; 'Accept' = 'application/vnd.github+json' }
    if ($env:WINCT_GITHUB_TOKEN) { $headers['Authorization'] = "Bearer $env:WINCT_GITHUB_TOKEN" }
    return $headers
}

function Invoke-Download([string]$Url, [string]$Path) {
    for ($attempt = 1; $attempt -le 3; $attempt++) {
        try {
            Invoke-WebRequest -Uri $Url -OutFile $Path -UseBasicParsing -TimeoutSec 300
            if ((Get-Item -LiteralPath $Path).Length -gt 0) { return }
            throw 'empty download'
        } catch {
            if ($attempt -eq 3) { Fail "download failed: $Url ($($_.Exception.Message))" }
        }
    }
}

function Test-Tag([string]$Tag, [string]$RequestedChannel) {
    if ($RequestedChannel -eq 'stable') { return $Tag -match '^v\d+\.\d+\.\d+$' }
    return $Tag -match '^v\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$'
}

function Get-LatestTag([string]$RequestedChannel) {
    $releases = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases?per_page=30" `
        -Headers (Get-GitHubHeaders) -TimeoutSec 60
    if ($RequestedChannel -eq 'stable') {
        $release = $releases | Where-Object {
            (Test-Tag $_.tag_name $RequestedChannel) -and -not $_.draft -and -not $_.prerelease
        } | Select-Object -First 1
    } else {
        $release = $releases | Where-Object {
            (Test-Tag $_.tag_name $RequestedChannel) -and -not $_.draft
        } | Select-Object -First 1
    }
    if (-not $release) { Fail "cannot resolve the latest $RequestedChannel release" }
    return [string]$release.tag_name
}

function Get-State([string]$Key) {
    $file = Join-Path $HomeDir 'state'
    if (-not (Test-Path -LiteralPath $file)) { return '' }
    $line = @(Get-Content -LiteralPath $file | Where-Object { $_ -like "$Key=*" } | Select-Object -First 1)
    if ($line.Count -eq 0) { return '' }
    return $line[0].Substring($Key.Length + 1)
}

function Set-State([string]$Key, [string]$Value) {
    $lines = @()
    $file = Join-Path $HomeDir 'state'
    if (Test-Path -LiteralPath $file) { $lines = @(Get-Content -LiteralPath $file) }
    $seen = $false
    $updated = @($lines | ForEach-Object {
        if ($_ -like "$Key=*") { $seen = $true; "$Key=$Value" } else { $_ }
    })
    if (-not $seen) { $updated += "$Key=$Value" }
    [IO.File]::WriteAllLines($file, [string[]]$updated)
}

function Enter-Lock {
    $lock = Join-Path $HomeDir '.lock'
    for ($i = 0; $i -lt 60; $i++) {
        try { New-Item -ItemType Directory -Path $lock -ErrorAction Stop | Out-Null; return $lock }
        catch { Start-Sleep -Seconds 1 }
    }
    Fail 'another winctup process holds the lock'
}

function Exit-Lock([string]$Lock) {
    if (Test-Path -LiteralPath $Lock) { Remove-Item -LiteralPath $Lock -Force -ErrorAction SilentlyContinue }
}

function Get-ExpectedHash([string]$Checksums, [string]$Name) {
    foreach ($line in Get-Content -LiteralPath $Checksums) {
        if ($line -match '^\s*([0-9a-fA-F]{64})\s+\*?(.+?)\s*$') {
            if ($Matches[2] -eq $Name) { return $Matches[1].ToLowerInvariant() }
        }
    }
    Fail "no checksum for $Name"
}

function Get-FileHashLower([string]$Path) {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Write-Launchers {
    New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
    $template = @'
@echo off
setlocal EnableExtensions
set "WINCT_HOME=__HOME__"
for /f "usebackq delims=" %%v in ("%WINCT_HOME%\current") do set "WINCT_VERSION=%%v"
"%WINCT_HOME%\releases\%WINCT_VERSION%\app\winc.exe" %*
exit /b %ERRORLEVEL%
'@
    $content = $template.Replace('__HOME__', $HomeDir)
    foreach ($name in @('winct.cmd', 'winc.cmd')) {
        [IO.File]::WriteAllText((Join-Path $BinDir $name), $content)
    }

    # A command-shaped shim so `winctup status` works on Windows too. SHIFT leaves %* unchanged, so
    # forward the remaining arguments explicitly.
    $runtimeTemplate = @'
@echo off
setlocal EnableExtensions
set "WINCT_HOME=__HOME__"
set "WINCT_BIN=__BIN__"
if "%~1"=="" goto showhelp
set "WINCT_CMD=%~1"
shift
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%WINCT_HOME%\runtime\winctup.ps1" -HomeDir "%WINCT_HOME%" -BinDir "%WINCT_BIN%" -Command "%WINCT_CMD%" %2 %3 %4 %5 %6 %7 %8 %9
exit /b %ERRORLEVEL%
:showhelp
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%WINCT_HOME%\runtime\winctup.ps1" -HomeDir "%WINCT_HOME%" -BinDir "%WINCT_BIN%" -Command help
exit /b %ERRORLEVEL%
'@
    $runtimeContent = $runtimeTemplate.Replace('__HOME__', $HomeDir).Replace('__BIN__', $BinDir)
    [IO.File]::WriteAllText((Join-Path $BinDir 'winctup.cmd'), $runtimeContent)
    Info "launchers: $(Join-Path $BinDir 'winct.cmd'), $(Join-Path $BinDir 'winc.cmd'), $(Join-Path $BinDir 'winctup.cmd')"
}

function Set-Current([string]$Version) {
    $temp = Join-Path $HomeDir 'current.new'
    [IO.File]::WriteAllText($temp, $Version)
    Move-Item -LiteralPath $temp -Destination (Join-Path $HomeDir 'current') -Force
}

function Prune-Releases([string]$Current, [string]$Previous) {
    $root = Join-Path $HomeDir 'releases'
    if (-not (Test-Path -LiteralPath $root)) { return }
    $entries = @(Get-ChildItem -LiteralPath $root -Directory | ForEach-Object {
        if ($_.Name -match '^\d+\.\d+\.\d+$') {
            [PSCustomObject]@{ Name = $_.Name; Version = [version]$_.Name; Path = $_.FullName }
        }
    } | Sort-Object -Property Version)
    $count = 0
    foreach ($entry in $entries) {
        if ($entry.Name -eq $Current -or $entry.Name -eq $Previous) { continue }
        $count++
        if ($count -gt $KEEP) { Remove-Item -LiteralPath $entry.Path -Recurse -Force }
    }
}

function Do-Install {
    if ($Repo -notmatch '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$') { Fail 'WINCT_RELEASE_REPO must be owner/repo' }
    if ($Ref -notmatch '^[A-Za-z0-9._/-]+$') { Fail 'WINCT_RUNTIME_REF has an invalid character' }

    $releasesRoot = Join-Path $HomeDir 'releases'
    $cacheRoot = Join-Path $HomeDir 'cache'
    New-Item -ItemType Directory -Force -Path $releasesRoot, $cacheRoot | Out-Null
    $lock = Enter-Lock
    try {
        $requested = $Channel
        if (-not $requested) { $requested = Get-State 'CHANNEL'; if (-not $requested) { $requested = 'stable' } }
        if ($requested -notin @('stable', 'beta')) { Fail "unknown channel: $requested" }

        $tag = Get-LatestTag $requested
        $version = $tag.TrimStart('v')
        $old = Get-State 'VERSION'
        if ($old -eq $version -and (Test-Path -LiteralPath (Join-Path $HomeDir 'current')) -and
            ((Get-State 'CHANNEL') -eq $requested)) {
            Info "Winct $version is already installed ($requested channel)"
            return
        }

        $asset = "Winc-$version-windows-x64-portable.zip"
        $base = "https://github.com/$Repo/releases/download/$tag"
        $tmp = Join-Path $cacheRoot ('.tmp-' + [guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Force -Path $tmp | Out-Null
        try {
            Invoke-Download "$base/SHA256SUMS" (Join-Path $tmp 'SHA256SUMS')
            Invoke-Download "$base/$asset" (Join-Path $tmp $asset)
            $expected = Get-ExpectedHash (Join-Path $tmp 'SHA256SUMS') $asset
            $actual = Get-FileHashLower (Join-Path $tmp $asset)
            if ($actual -ne $expected) { Fail 'download does not match SHA256SUMS' }

            Info "installing Winct $version from $tag"
            $stage = Join-Path $releasesRoot ".$version.staging"
            if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
            New-Item -ItemType Directory -Force -Path $stage | Out-Null
            Expand-Archive -LiteralPath (Join-Path $tmp $asset) -DestinationPath $stage -Force
            if (-not (Test-Path -LiteralPath (Join-Path $stage 'winc.exe'))) {
                Fail 'the portable archive has no winc.exe'
            }
            & (Join-Path $stage 'winc.exe') --version | Out-Null

            $target = Join-Path $releasesRoot $version
            if (Test-Path -LiteralPath $target) { Remove-Item -LiteralPath $target -Recurse -Force }
            Move-Item -LiteralPath $stage -Destination $target
            Set-Current $version
            Set-State 'PREVIOUS' $old
            Set-State 'CHANNEL' $requested
            Set-State 'ARTIFACT' $asset
            Set-State 'INSTALLED_AT' ([DateTime]::UtcNow.ToString('o'))
            Write-Launchers
            Prune-Releases $version $old
        } finally {
            Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
        }
        Info "Winct $version is ready"
    } finally {
        Exit-Lock $lock
    }
}

function Do-Status {
    $installed = Get-State 'VERSION'
    $channel = Get-State 'CHANNEL'
    if (-not $channel) { $channel = 'stable' }
    $latest = Get-LatestTag $channel
    Write-Host "runtime:   winctup $RUNTIME_VERSION"
    Write-Host "channel:   $channel"
    Write-Host "installed: $(if ($installed) { $installed } else { 'none' })"
    Write-Host "latest:    $latest"
    Write-Host "home:      $HomeDir"
    Write-Host "bin:       $BinDir"
}

function Do-Rollback {
    $lock = Enter-Lock
    try {
        $current = Get-State 'VERSION'
        $previous = Get-State 'PREVIOUS'
        if (-not $previous -or -not (Test-Path -LiteralPath (Join-Path $HomeDir "releases\$previous\app\winc.exe"))) {
            Fail 'no previous release is available to roll back to'
        }
        Set-Current $previous
        Set-State 'PREVIOUS' $current
        Info "rolled back to Winct $previous"
    } finally {
        Exit-Lock $lock
    }
}

function Do-SelfUpdate {
    if ($Ref -notmatch '^[A-Za-z0-9._/-]+$') { Fail 'WINCT_RUNTIME_REF has an invalid character' }
    $tmp = Join-Path ([IO.Path]::GetTempPath()) ('winctup-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Force -Path $tmp | Out-Null
    try {
        Invoke-Download "$RawBase/CHECKSUMS" (Join-Path $tmp 'CHECKSUMS')
        foreach ($name in @('winctup.ps1', 'install.ps1')) {
            Invoke-Download "$RawBase/$name" (Join-Path $tmp $name)
            $expected = Get-ExpectedHash (Join-Path $tmp 'CHECKSUMS') $name
            $actual = Get-FileHashLower (Join-Path $tmp $name)
            if ($actual -ne $expected) { Fail "$name does not match CHECKSUMS" }
        }
        $runtimeDir = Join-Path $HomeDir 'runtime'
        New-Item -ItemType Directory -Force -Path $runtimeDir | Out-Null
        Copy-Item -LiteralPath (Join-Path $tmp 'CHECKSUMS') -Destination $runtimeDir -Force
        foreach ($name in @('winctup.ps1', 'install.ps1')) {
            Copy-Item -LiteralPath (Join-Path $tmp $name) -Destination $runtimeDir -Force
        }
    } finally {
        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }
    Info 'winctup runtime updated and verified'
}

function Do-Uninstall {
    if (-not $Yes) { Fail 'pass -Yes to confirm uninstall' }
    if ((Split-Path -Leaf $HomeDir) -notmatch '^winct(up)?$') {
        Fail "refusing to remove ${HomeDir}: WINCT_HOME must end in winct or winctup"
    }
    Remove-Item -LiteralPath $HomeDir -Recurse -Force
    foreach ($name in @('winct.cmd', 'winc.cmd', 'winctup.cmd')) {
        Remove-Item -LiteralPath (Join-Path $BinDir $name) -Force -ErrorAction SilentlyContinue
    }
    Info 'winctup and its releases were removed; application settings were kept'
}

function Do-Doctor {
    foreach ($command in @('Expand-Archive', 'Get-FileHash')) {
        if (-not (Get-Command $command -ErrorAction SilentlyContinue)) { Fail "missing command: $command" }
    }
    Write-Host "runtime: winctup $RUNTIME_VERSION"
    Write-Host "home:    $HomeDir"
    Write-Host "bin:     $BinDir"
    Write-Host "state:   $(if (Get-State 'VERSION') { Get-State 'VERSION' } else { 'none' })"
    Info 'environment looks healthy'
}

function Show-Help {
    @'
winctup - install and update Winct from GitHub Releases

Usage: winctup <command> [options]

Commands:
  install [-Channel stable|beta]  install Winct (no-op when already current)
  update  [-Channel stable|beta]  check for a release and install it
  status                          show installed, latest, channel and paths
  rollback                        return to the previous release
  self-update                     verify and replace this runtime
  uninstall -Yes                  remove the runtime and its launchers
  doctor                          check the local environment

Environment:
  WINCT_RELEASE_REPO, WINCT_RUNTIME_REF, WINCT_CHANNEL, WINCT_HOME, WINCT_BIN
'@ | Write-Host
}

# Dot-sourcing exposes the helpers to the offline test suite without running a command.
if ($MyInvocation.InvocationName -ne '.') {
    try {
        switch ($Command) {
            'install' { Do-Install }
            'update' { Do-Install }
            'status' { Do-Status }
            'rollback' { Do-Rollback }
            'self-update' { Do-SelfUpdate }
            'uninstall' { Do-Uninstall }
            'doctor' { Do-Doctor }
            default { Show-Help }
        }
    } catch {
        [Console]::Error.WriteLine($_.Exception.Message)
        exit 1
    }
}
