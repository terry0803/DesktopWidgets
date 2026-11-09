# Builds the release program and installs it into %LOCALAPPDATA%\Programs\DesktopWidgets, where it keeps its
# settings. That's on C:, so it still starts when this source folder's drive is unplugged. Stops a running copy first.
param([switch]$Test)

$install = Join-Path $env:LOCALAPPDATA 'Programs\DesktopWidgets'

# Always the GNU toolchain, even where rustup's default is MSVC: this needs no Visual Studio, and the linker it brings
# is put on PATH below.
$toolchainName = 'stable-x86_64-pc-windows-gnu'
$env:RUSTUP_TOOLCHAIN = $toolchainName
$rustupHome = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $env:USERPROFILE '.rustup' }
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
$toolchain = Join-Path $rustupHome "toolchains\$toolchainName"
$env:Path = "$cargoHome\bin;$toolchain\lib\rustlib\x86_64-pc-windows-gnu\bin\self-contained;$env:Path"
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    "Rust isn't installed: get rustup from https://rustup.rs, then run: rustup toolchain install $toolchainName"
    exit 1
}
if (-not (Test-Path -LiteralPath $toolchain)) {
    "The Rust GNU toolchain isn't installed. Run: rustup toolchain install $toolchainName"
    exit 1
}
# Paths are always taken literally (-LiteralPath, escaped -like): a folder named like "DesktopWidgets[1]" would
# otherwise be read as a wildcard pattern that matches nothing.
Set-Location -LiteralPath $PSScriptRoot

if ($Test) {
    cargo test
    exit $LASTEXITCODE
}

# The program is installed from where cargo says it put it, never a path worked out here: CARGO_TARGET_DIR,
# CARGO_BUILD_TARGET or a cargo config can each move it away from target\release, where an old build may be left.
# Messages go to the console as usual; only cargo's machine-readable report comes back on stdout.
[Console]::OutputEncoding = [Text.Encoding]::UTF8 # cargo writes UTF-8; a path can have any letters in it.
$report = @(cargo build --release --message-format=json-render-diagnostics)
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$built = @($report | Where-Object { $_.StartsWith('{') } | ForEach-Object { $_ | ConvertFrom-Json } |
    Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'desktop-widgets' -and $_.executable } |
    ForEach-Object { $_.executable })
if ($built.Count -ne 1 -or -not (Test-Path -LiteralPath $built[0])) {
    "cargo didn't say where it put desktop-widgets.exe, so nothing was installed"
    exit 1
}
$built = $built[0]
"cargo put it at $built"

# Asked to close the way its own menu does, so that whatever it couldn't save yet is set aside first. Never killed:
# if it won't close, the old program stays and nothing is replaced.
Add-Type -Namespace DesktopWidgets -Name Native -MemberDefinition @'
[DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindowEx(IntPtr parent, IntPtr after, string cls, string title);
[DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window, uint message, IntPtr w, IntPtr l);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
'@
foreach ($running in @(Get-Process desktop-widgets -ErrorAction SilentlyContinue | Where-Object {
    $_.Path -like ([WildcardPattern]::Escape($install) + '\*') -or $_.Path -like ([WildcardPattern]::Escape($PSScriptRoot) + '\app\*')
})) {
    $window = [IntPtr]::Zero
    while (($window = [DesktopWidgets.Native]::FindWindowEx([IntPtr]::Zero, $window, 'DesktopWidgets.Hidden', [NullString]::Value)) -ne [IntPtr]::Zero) {
        [uint32]$owner = 0
        [void][DesktopWidgets.Native]::GetWindowThreadProcessId($window, [ref]$owner)
        if ($owner -eq $running.Id) { [void][DesktopWidgets.Native]::PostMessage($window, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) }
    }
    if (-not $running.WaitForExit(15000)) {
        "desktop-widgets did not close, so it was left running and not replaced"
        exit 1
    }
}
Start-Sleep -Milliseconds 300
try {
    New-Item -ItemType Directory -Force -Path $install -ErrorAction Stop | Out-Null
    Copy-Item -LiteralPath $built -Destination "$install\desktop-widgets.exe" -Force -ErrorAction Stop
} catch {
    "$install\desktop-widgets.exe was not replaced: $($_.Exception.Message)"
    exit 1
}
# Only a copy that matches what was just built counts as done.
if ((Get-FileHash -LiteralPath $built).Hash -ne (Get-FileHash -LiteralPath "$install\desktop-widgets.exe").Hash) {
    "$install\desktop-widgets.exe is not the program just built"
    exit 1
}
# Builds before this one installed into app\ next to the source. The first install here brings that copy's settings,
# to-dos and backups along (never over files already here), and moves the startup entry and Start menu shortcut that
# still open the old copy, which would otherwise stay with it and stop working once this drive is unplugged.
# A marker file stays while the move isn't finished, so a build after a failed one carries on instead of taking the
# files already there for a finished move; until it's finished nothing else is switched over. It records the content
# of each file moved, because the old copy may be used in between: a file changed there since is brought over again
# if the one here is still as it was moved, and otherwise put beside it, so neither side's changes are lost.
$old = Join-Path $PSScriptRoot 'app'
$oldExe = Join-Path $old 'desktop-widgets.exe'
$newExe = Join-Path $install 'desktop-widgets.exe'
$moving = Join-Path $install 'moving-from-app.txt'
$fresh = -not (Test-Path -LiteralPath "$install\widgets.ini") -and -not (Test-Path -LiteralPath "$install\todo.txt")
if ((Test-Path -LiteralPath $old) -and ($fresh -or (Test-Path -LiteralPath $moving))) {
    try {
        $moved = @{}
        if (Test-Path -LiteralPath $moving) {
            foreach ($line in Get-Content -LiteralPath $moving -Encoding UTF8 -ErrorAction Stop) {
                if ($line -match '^moved (\S+) (.+)$') { $moved[$Matches[2]] = $Matches[1] }
            }
        }
        $record = {
            $lines = @("Settings and to-dos are being moved here from $old; build.ps1 removes this once that's done.")
            $lines += @($moved.Keys | Sort-Object | ForEach-Object { "moved $($moved[$_]) $_" })
            Set-Content -LiteralPath $moving -Value $lines -Encoding UTF8 -ErrorAction Stop
        }
        & $record
        foreach ($file in @(Get-ChildItem -LiteralPath $old -File -ErrorAction Stop | Where-Object { $_.Name -ne 'desktop-widgets.exe' })) {
            $target = Join-Path $install $file.Name
            # Copied under another name first, so a copy cut short is never taken for the file, and what's compared
            # below is exactly what would be put in place.
            Copy-Item -LiteralPath $file.FullName -Destination "$target.part" -Force -ErrorAction Stop
            $hash = (Get-FileHash -LiteralPath "$target.part" -ErrorAction Stop).Hash
            $here = if (Test-Path -LiteralPath $target) { (Get-FileHash -LiteralPath $target -ErrorAction Stop).Hash }
            # Missing here (even after it was moved once: it may have been taken away since) → brought over.
            # The same here → nothing to do. Unchanged in app\ since it was moved → what's here is newer and stays.
            # Changed in app\ but untouched here → app\'s newer one replaces it. Changed on both sides → kept beside.
            if (-not $here -or ($here -ne $hash -and $here -eq $moved[$file.Name])) {
                Move-Item -LiteralPath "$target.part" -Destination $target -Force -ErrorAction Stop
                "moved $($file.Name) from app\"
            } elseif ($here -eq $hash -or $moved[$file.Name] -eq $hash) {
                Remove-Item -LiteralPath "$target.part" -ErrorAction Stop
            } else {
                $stem = [IO.Path]::GetFileNameWithoutExtension($file.Name)
                $extension = [IO.Path]::GetExtension($file.Name)
                $number = 1
                do {
                    $beside = Join-Path $install ("$stem.from-app" + $(if ($number -gt 1) { $number }) + $extension)
                    $number++
                } while (Test-Path -LiteralPath $beside)
                Move-Item -LiteralPath "$target.part" -Destination $beside -ErrorAction Stop
                "$($file.Name) changed both here and in app\: kept this one and put app\'s beside it as $(Split-Path $beside -Leaf)"
            }
            $moved[$file.Name] = $hash
            & $record
        }
        Remove-Item -LiteralPath $moving -ErrorAction Stop
    } catch {
        Get-ChildItem -LiteralPath $install -Filter *.part -File -ErrorAction SilentlyContinue | Remove-Item -ErrorAction SilentlyContinue
        "moving settings and to-dos from app\ didn't finish: $($_.Exception.Message)"
        "the startup entry and Start menu shortcut still open the old copy; run build.ps1 again to finish"
        exit 1
    }
}
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$run = (Get-ItemProperty $runKey -ErrorAction SilentlyContinue).DesktopWidgets
if ($run -and $run.Trim().Trim('"') -eq $oldExe) {
    Set-ItemProperty $runKey DesktopWidgets "`"$newExe`""
    "startup entry now opens $newExe"
}
$shortcut = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\桌面小工具.lnk'
if (Test-Path -LiteralPath $shortcut) {
    $link = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcut)
    if ($link.TargetPath -eq $oldExe) {
        $link.TargetPath = $newExe
        $link.WorkingDirectory = $install
        $link.Save()
        "Start menu shortcut now opens $newExe"
    }
}

"built $install\desktop-widgets.exe ($([math]::Round((Get-Item -LiteralPath "$install\desktop-widgets.exe").Length / 1KB)) KB)"

# Started again, in place of the copy closed above; on a first install this is also what makes its Start menu shortcut.
Start-Process $newExe -WorkingDirectory $install
"started $newExe"
