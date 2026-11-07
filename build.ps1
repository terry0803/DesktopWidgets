# Builds the release program into app\, where it keeps its settings. Stops a running copy first.
param([switch]$Test)

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
foreach ($running in @(Get-Process desktop-widgets -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$PSScriptRoot\app\*" })) {
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
    New-Item -ItemType Directory -Force app -ErrorAction Stop | Out-Null
    Copy-Item target\release\desktop-widgets.exe app\desktop-widgets.exe -Force -ErrorAction Stop
} catch {
    "app\desktop-widgets.exe was not replaced: $($_.Exception.Message)"
    exit 1
}
# Only a copy that matches what was just built counts as done.
if ((Get-FileHash target\release\desktop-widgets.exe).Hash -ne (Get-FileHash app\desktop-widgets.exe).Hash) {
    "app\desktop-widgets.exe is not the program just built"
    exit 1
}
"built app\desktop-widgets.exe ($([math]::Round((Get-Item app\desktop-widgets.exe).Length / 1KB)) KB)"
