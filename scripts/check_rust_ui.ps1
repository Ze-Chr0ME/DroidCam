$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$exe = Join-Path $root 'dist\CameraBridge-Rust\CameraBridge.exe'
Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class CameraBridgeUiProbe {
    delegate bool Callback(IntPtr h, IntPtr p);
    [DllImport("user32.dll")] static extern bool EnumWindows(Callback cb, IntPtr p);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder text, int n);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint message, IntPtr w, IntPtr l);
    public static IntPtr Find(uint pid) {
        IntPtr found=IntPtr.Zero;
        EnumWindows((h,p) => {
            uint owner; GetWindowThreadProcessId(h,out owner);
            if(owner==pid) { var text=new StringBuilder(256); GetWindowText(h,text,256); if(text.ToString().Equals("CameraBridge")) found=h; }
            return true;
        },IntPtr.Zero);
        return found;
    }
}
'@
$app = Start-Process -FilePath $exe -WindowStyle Hidden -PassThru
try {
    if (-not $app.WaitForInputIdle(5000)) { throw 'Receiver UI did not become idle.' }
    $app.Refresh()
    $window = [CameraBridgeUiProbe]::Find($app.Id)
    if ($app.HasExited -or $window -eq [IntPtr]::Zero) {
        throw "Receiver failed to create its main UI: $($app.MainWindowTitle)"
    }
    $before = $app.TotalProcessorTime.TotalMilliseconds
    Start-Sleep -Milliseconds 1000
    $app.Refresh()
    $result = [ordered]@{ok=$true; working_set_bytes=$app.WorkingSet64; private_bytes=$app.PrivateMemorySize64; idle_cpu_ms_per_second=($app.TotalProcessorTime.TotalMilliseconds-$before)}
    if (-not [CameraBridgeUiProbe]::PostMessage($window,0x0010,[IntPtr]::Zero,[IntPtr]::Zero)) { throw 'Receiver UI did not accept close.' }
    if (-not $app.WaitForExit(5000)) { throw 'Receiver did not stop its worker on close.' }
    if ($app.ExitCode -ne 0) { throw "Receiver exited with $($app.ExitCode)" }
    $result | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $root 'build\rust-ui-check.json') -Encoding utf8
    $result | ConvertTo-Json
} finally {
    # This helper owns only the process it launched.
    if (-not $app.HasExited) { $app.Kill() }
    $app.Dispose()
}
