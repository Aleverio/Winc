param([string]$Target = 'x86_64-pc-windows-msvc')
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path "$PSScriptRoot/../..").Path
$version = (& node "$PSScriptRoot/../common/version.mjs").Trim()
if ($LASTEXITCODE -ne 0) { throw 'cannot read the VERSION file' }
$bundle = "$root/target/$Target/release/bundle"
$work = Join-Path $env:RUNNER_TEMP 'winct-installer-verification'
New-Item -ItemType Directory -Force $work | Out-Null
$update = "$root/crates/winct-app/src/updates/installer/update.ps1"
# The NSIS installer changes the user's Path and the MSI changes the machine's.
function Get-CommandPath([string]$Kind) {
    $scope = if ($Kind -eq 'nsis') { 'User' } else { 'Machine' }
    @([Environment]::GetEnvironmentVariable('Path', $scope) -split ';' | ForEach-Object { $_.TrimEnd('\') })
}
# File associations and the file manager take the icon from the executable itself.
$shell = Add-Type -Name Shell -Namespace WinctVerify -PassThru -MemberDefinition '[DllImport("shell32.dll", CharSet = CharSet.Unicode)] public static extern uint ExtractIconEx(string file, int index, IntPtr large, IntPtr small, uint count);'
if ($shell::ExtractIconEx("$root/target/$Target/release/winct.exe", -1, [IntPtr]::Zero, [IntPtr]::Zero, 0) -lt 1) { throw 'winct.exe has no embedded icon' }
foreach ($kind in @('nsis', 'msi')) {
    $directory = Join-Path $work "$kind installation with spaces"
    $suffix = if ($kind -eq 'nsis') { '-setup.exe' } else { '.msi' }
    # msiexec cannot open a package path written with forward slashes (error 1619).
    $setup = (Resolve-Path "$bundle/$kind/Winct-$version-windows-x64$suffix").Path
    $classes = if ($kind -eq 'nsis') { 'HKCU:\Software\Classes' } else { 'HKLM:\Software\Classes' }
    # An earlier NSIS version listed winct in each type's OpenWithProgids; installing must remove it.
    # `New-Item -Force` also creates missing parent keys, but it would recreate an existing key and
    # drop other programs' entries, so it runs only when the key is missing.
    # The Start menu and Windows' app lists show the name Winct. The NSIS shortcut goes in the user's
    # Start menu, the MSI one in a Winct folder of the machine's; an existing lowercase shortcut must
    # not keep its name.
    $menu = if ($kind -eq 'nsis') { [Environment]::GetFolderPath('Programs') } else { Join-Path ([Environment]::GetFolderPath('CommonPrograms')) 'Winct' }
    if ($kind -eq 'nsis') {
        Set-Content -LiteralPath (Join-Path $menu 'winct.lnk') -Value ''
        $listing = "$classes\.json\OpenWithProgids"
        if (-not (Test-Path -LiteralPath $listing)) { New-Item -Force -Path $listing | Out-Null }
        New-ItemProperty -Force -LiteralPath $listing -Name 'winct.JSON' -Value '' | Out-Null
        # An earlier version also added the context menu to each type; installing must remove it.
        New-Item -Force -Path "$classes\SystemFileAssociations\.json\shell\winct\command" | Out-Null
    }
    try {
        if ($kind -eq 'nsis') {
            $process = Start-Process -FilePath $setup -ArgumentList "/S /D=$directory" -Wait -PassThru
        } else {
            $process = Start-Process msiexec.exe -ArgumentList "/i `"$setup`" /qn /norestart INSTALLFOLDER=`"$directory`"" -Wait -PassThru
        }
        if ($process.ExitCode -notin @(0, 1641, 3010)) { throw "$kind installation failed: $($process.ExitCode)" }
        foreach ($file in @('winct.exe', 'winct-pty.exe', 'conpty.dll', 'OpenConsole.exe', 'winct-installation', 'winct-watch', 'winct-server-Linux-x86_64', 'winct-server-Linux-aarch64', 'bin/winct', 'bin/winct.cmd')) {
            if (-not (Test-Path "$directory/$file")) { throw "${kind}: missing $file" }
        }
        if ((Get-Content "$directory/bin/winct" -Raw) -match "`r") { throw "${kind}: bin/winct has CRLF line endings" }
        if ((Get-CommandPath $kind) -notcontains "$directory\bin") { throw "${kind}: bin is not on the Path" }
        if ((Get-Content "$directory/winct-installation" -Raw).Trim() -ne $kind) { throw 'incorrect installation marker' }
        # Each type's ProgID shows the document icon of its group.
        foreach ($pair in @(@('JSON', 'json'), @('MD', 'markdown'), @('RS', 'rust'), @('SQL', 'code'), @('TEX', 'markup'), @('PDF', 'text'), @('WINCT-PROJECT', 'project'))) {
            $icon = (Get-ItemProperty -LiteralPath "$classes\winct.$($pair[0])\DefaultIcon").'(default)'
            if ($icon -ne "$directory\icons\$($pair[1]).ico") { throw "${kind}: wrong icon for $($pair[0]): $icon" }
            if (-not (Test-Path -LiteralPath $icon)) { throw "${kind}: missing $icon" }
        }
        # .winct-project is Winct's own type and opens with it by default.
        $project = (Get-ItemProperty -LiteralPath "$classes\.winct-project").'(default)'
        if ($project -ne 'winct.WINCT-PROJECT') { throw "${kind}: .winct-project does not open with winct: $project" }
        $command = (Get-ItemProperty -LiteralPath "$classes\winct.JSON\shell\open\command").'(default)'
        if ($command -ne "`"$directory\winct.exe`" `"%1`"") { throw "${kind}: wrong open command: $command" }
        if ($null -eq (Get-ItemProperty -LiteralPath "$classes\Applications\winct.exe\SupportedTypes").'.json') { throw "${kind}: json is not a supported type" }
        if ((Get-ItemProperty -LiteralPath "$classes\Applications\winct.exe").FriendlyAppName -cne 'Winct') { throw "${kind}: the application is not named Winct" }
        # Explorer context menu: supported types open the file, folders and their empty space open
        # the folder. The label follows the installer's language.
        foreach ($verb in @(@('SystemFileAssociations\.json', '%1'), @('Directory', '%V'), @('Directory\Background', '%V'))) {
            $key = "$classes\$($verb[0])\shell\winct"
            if (-not (Get-ItemProperty -LiteralPath $key -ErrorAction SilentlyContinue).'(default)') { throw "${kind}: no context menu label under $($verb[0])" }
            $command = (Get-ItemProperty -LiteralPath "$key\command").'(default)'
            if ($command -ne "`"$directory\winct.exe`" `"$($verb[1])`"") { throw "${kind}: wrong context menu command under $($verb[0]): $command" }
        }
        # The winct: URL scheme hands the whole link to winct.exe.
        if ($null -eq (Get-ItemProperty -LiteralPath "$classes\winct").'URL Protocol') { throw "${kind}: winct is not a URL scheme" }
        $command = (Get-ItemProperty -LiteralPath "$classes\winct\shell\open\command").'(default)'
        if ($command -ne "`"$directory\winct.exe`" `"%1`"") { throw "${kind}: wrong URL scheme command: $command" }
        $link = Get-ChildItem -LiteralPath $menu -Filter 'winct.lnk'
        if (@($link).Count -ne 1 -or $link.Name -cne 'Winct.lnk') { throw "${kind}: the Start menu shortcut is not named Winct" }
        if ($kind -eq 'msi' -and (Get-Item -LiteralPath $menu).Name -cne 'Winct') { throw 'msi: the Start menu folder is not named Winct' }
        if ($kind -eq 'nsis' -and (Get-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\winct').DisplayName -cne 'Winct') { throw 'nsis: the installed app is not listed as Winct' }
        # A handler listed under a type makes Windows ask which program to use, and for types it runs
        # on double-click (BAT, CMD) the question replaces running the script.
        foreach ($type in @('json', 'txt', 'bat', 'cmd')) {
            $listed = Get-ItemProperty -LiteralPath "$classes\.$type\OpenWithProgids" -ErrorAction SilentlyContinue
            if ($null -ne $listed -and $null -ne $listed."winct.$type") { throw "${kind}: winct is listed under .$type" }
        }
        foreach ($type in @('bat', 'cmd')) {
            if (& $listed $type) { throw "${kind}: winct is listed under .$type" }
            if (Test-Path -LiteralPath "$classes\winct.$type") { throw "${kind}: $type must not be registered" }
        }
        $inMenu = { Test-Path -LiteralPath "$classes\*\shell\winct" }
        if ($kind -eq 'nsis') {
            # Both are the user's choice: once made it survives a later silent installation, which is
            # how updates run, until the other value is passed.
            $steps = @(
                @('/FILETYPES=0', $false, $false), @('', $false, $false), @('/FILETYPES=1', $true, $false), @('', $true, $false),
                @('/CONTEXTMENU=1', $true, $true), @('', $true, $true), @('/CONTEXTMENU=0', $true, $false), @('', $true, $false)
            )
            foreach ($step in $steps) {
                $process = Start-Process -FilePath $setup -ArgumentList "/S $($step[0]) /D=$directory" -Wait -PassThru
                if ($process.ExitCode -ne 0) { throw "nsis installation with '$($step[0])' failed: $($process.ExitCode)" }
                if ((& $listed 'json') -ne $step[1]) { throw "nsis: after '$($step[0])' the json listing is not $($step[1])" }
                if ((& $inMenu) -ne $step[2]) { throw "nsis: after '$($step[0])' the context menu is not $($step[2])" }
            }
            # An installation from before the choices were recorded keeps what it had: the context
            # menu, which was always added, and no listing.
            Remove-ItemProperty -LiteralPath 'HKCU:\Software\DEEIX\winct' -Name 'ContextMenu', 'FileTypes'
            $process = Start-Process -FilePath $setup -ArgumentList "/S /D=$directory" -Wait -PassThru
            if ($process.ExitCode -ne 0) { throw "nsis installation over an earlier version failed: $($process.ExitCode)" }
            if ((& $listed 'json') -or -not (& $inMenu)) { throw 'nsis: an earlier installation did not keep its context menu and listing' }
        } else {
            $process = Start-Process msiexec.exe -ArgumentList "/i `"$setup`" /qn /norestart ADDLOCAL=ContextMenu INSTALLFOLDER=`"$directory`"" -Wait -PassThru
            if ($process.ExitCode -notin @(0, 1641, 3010)) { throw "msi: adding the context menu failed: $($process.ExitCode)" }
        }
        # Explorer context menu, once chosen: one entry for every file opens the file, folders and
        # their empty space open the folder. The label follows the installer's language.
        foreach ($verb in @(@('*', '%1'), @('Directory', '%V'), @('Directory\Background', '%V'))) {
            $key = "$classes\$($verb[0])\shell\winct"
            if (-not (Get-ItemProperty -LiteralPath $key -ErrorAction SilentlyContinue).'(default)') { throw "${kind}: no context menu label under $($verb[0])" }
            $command = (Get-ItemProperty -LiteralPath "$key\command").'(default)'
            if ($command -ne "`"$directory\winct.exe`" `"$($verb[1])`"") { throw "${kind}: wrong context menu command under $($verb[0]): $command" }
        }
        # A short-lived parent exercises the updater's wait before reinstalling.
        $parent = Start-Process powershell.exe -ArgumentList '-NoProfile -Command "Start-Sleep -Seconds 2"' -PassThru -WindowStyle Hidden
        # The updater is started the way the application starts it: `-Command` with the script
        # inlined, and its parameters in `WINCT_UPDATE_*` environment variables. Passing them as
        # `-File` arguments would reach a script that no longer declares `param`, leaving every
        # parameter empty.
        $env:WINCT_UPDATE_PARENT = "$($parent.Id)"
        $env:WINCT_UPDATE_SETUP = $setup
        $env:WINCT_UPDATE_EXE = "$directory/winct.exe"
        $env:WINCT_UPDATE_KIND = $kind
        $env:WINCT_UPDATE_RELAUNCH = '0'
        & powershell.exe -NoProfile -NonInteractive -Command (Get-Content -LiteralPath $update -Raw)
        $status = $LASTEXITCODE
        Remove-Item Env:WINCT_UPDATE_*
        if ($status -ne 0) { throw "$kind updater failed" }
        foreach ($file in @('winct.exe', 'winct-pty.exe')) {
            if ((Get-FileHash "$directory/$file").Hash -ne (Get-FileHash "$root/target/$Target/release/$file").Hash) {
                throw "$kind update changed $file"
            }
        }
    } finally {
        if ($kind -eq 'nsis' -and (Test-Path "$directory/uninstall.exe")) {
            $uninstall = Start-Process "$directory/uninstall.exe" -ArgumentList '/S' -Wait -PassThru
        } elseif ($kind -eq 'msi') {
            $uninstall = Start-Process msiexec.exe -ArgumentList "/x `"$setup`" /qn /norestart" -Wait -PassThru
        }
    }
    foreach ($file in @('conpty.dll', 'OpenConsole.exe')) {
        if (Test-Path "$directory/$file") { throw "${kind}: uninstall left $file behind" }
    }
    if ((Get-CommandPath $kind) -contains "$directory\bin") { throw "${kind}: uninstall left bin on the Path" }
    if (Test-Path -LiteralPath "$classes\winct.JSON") { throw "${kind}: uninstall left the file type behind" }
    if (Test-Path "$directory/icons") { throw "${kind}: uninstall left the icons behind" }
    if (Test-Path -LiteralPath "$classes\Applications\winct.exe") { throw "${kind}: uninstall left the application behind" }
    foreach ($key in @('*', 'SystemFileAssociations\.json', 'Directory', 'Directory\Background')) {
        if (Test-Path -LiteralPath "$classes\$key\shell\winct") { throw "${kind}: uninstall left the context menu under $key" }
    }
    if (Test-Path -LiteralPath "$classes\winct") { throw "${kind}: uninstall left the URL scheme behind" }
    if (Test-Path -LiteralPath (Join-Path $menu 'Winct.lnk')) { throw "${kind}: uninstall left the Start menu shortcut behind" }
}
Expand-Archive "$bundle/portable/Winct-$version-windows-x64-portable.zip" "$work/portable" -Force
if ((Test-Path "$work/portable/winct-installation") -or -not (Test-Path "$work/portable/winct-pty.exe") -or -not (Test-Path "$work/portable/conpty.dll") -or -not (Test-Path "$work/portable/OpenConsole.exe") -or -not (Test-Path "$work/portable/bin/winct") -or -not (Test-Path "$work/portable/winct-watch") -or -not (Test-Path "$work/portable/winct-server-Linux-x86_64") -or -not (Test-Path "$work/portable/winct-server-Linux-aarch64") -or -not (Test-Path "$work/portable/data/README.txt")) { throw 'invalid portable layout' }
