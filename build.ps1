# Builds the release program and installs it into %LOCALAPPDATA%\Programs\DesktopWidgets, where it keeps its
# settings. That's on C:, so it still starts when this source folder's drive is unplugged. Stops a running copy first.
param([switch]$Test)

$install = Join-Path $env:LOCALAPPDATA 'Programs\DesktopWidgets'

$toolchain = Join-Path $env:USERPROFILE '.rustup\toolchains\stable-x86_64-pc-windows-gnu'
$env:Path = "$env:USERPROFILE\.cargo\bin;$toolchain\lib\rustlib\x86_64-pc-windows-gnu\bin\self-contained;$env:Path"
Set-Location $PSScriptRoot

if ($Test) {
    cargo test
    exit $LASTEXITCODE
}

cargo build --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# Asked to close the way its own menu does, so that whatever it couldn't save yet is set aside first. Never killed:
# if it won't close, the old program stays and nothing is replaced.
Add-Type -Namespace DesktopWidgets -Name Native -MemberDefinition @'
[DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindowEx(IntPtr parent, IntPtr after, string cls, string title);
[DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window, uint message, IntPtr w, IntPtr l);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
'@
foreach ($running in @(Get-Process desktop-widgets -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$install\*" -or $_.Path -like "$PSScriptRoot\app\*" })) {
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
    New-Item -ItemType Directory -Force $install -ErrorAction Stop | Out-Null
    Copy-Item target\release\desktop-widgets.exe "$install\desktop-widgets.exe" -Force -ErrorAction Stop
} catch {
    "$install\desktop-widgets.exe was not replaced: $($_.Exception.Message)"
    exit 1
}
# Only a copy that matches what was just built counts as done.
if ((Get-FileHash target\release\desktop-widgets.exe).Hash -ne (Get-FileHash "$install\desktop-widgets.exe").Hash) {
    "$install\desktop-widgets.exe is not the program just built"
    exit 1
}
# Builds before this one installed into app\ next to the source. The first install here brings that copy's settings,
# to-dos and backups along (never over files already here), and moves the startup entry and Start menu shortcut that
# still open the old copy, which would otherwise stay with it and stop working once this drive is unplugged.
# A marker file stays while the move isn't finished, so a build after a failed one carries on with what's missing
# instead of taking the files already there for a finished move; until it's finished nothing else is switched over.
$old = Join-Path $PSScriptRoot 'app'
$oldExe = Join-Path $old 'desktop-widgets.exe'
$newExe = Join-Path $install 'desktop-widgets.exe'
$moving = Join-Path $install 'moving-from-app.txt'
$fresh = -not (Test-Path "$install\widgets.ini") -and -not (Test-Path "$install\todo.txt")
if ((Test-Path $old) -and ($fresh -or (Test-Path $moving))) {
    try {
        Set-Content $moving "Settings and to-dos are being moved here from $old; build.ps1 removes this once that's done." -ErrorAction Stop
        foreach ($file in @(Get-ChildItem $old -File -ErrorAction Stop | Where-Object { $_.Name -ne 'desktop-widgets.exe' })) {
            $target = Join-Path $install $file.Name
            if (-not (Test-Path $target)) {
                # Copied under another name first, so a copy cut short is never taken for the file.
                Copy-Item $file.FullName "$target.part" -Force -ErrorAction Stop
                Move-Item "$target.part" $target -ErrorAction Stop
                "moved $($file.Name) from app\"
            }
        }
        Remove-Item $moving -ErrorAction Stop
    } catch {
        Remove-Item "$install\*.part" -ErrorAction SilentlyContinue
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
if (Test-Path $shortcut) {
    $link = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcut)
    if ($link.TargetPath -eq $oldExe) {
        $link.TargetPath = $newExe
        $link.WorkingDirectory = $install
        $link.Save()
        "Start menu shortcut now opens $newExe"
    }
}

"built $install\desktop-widgets.exe ($([math]::Round((Get-Item "$install\desktop-widgets.exe").Length / 1KB)) KB)"
