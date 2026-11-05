$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Add-Type -AssemblyName System.Drawing
Add-Type -ReferencedAssemblies System.Drawing @'
using System;
using System.Drawing;
using System.Runtime.InteropServices;
public static class DocsCapture {
    [StructLayout(LayoutKind.Sequential)] struct Rect { public int L,T,R,B; }
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr hwnd, out Rect rect);
    [DllImport("user32.dll")] static extern bool PrintWindow(IntPtr hwnd, IntPtr dc, uint flags);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr hwnd, uint msg, IntPtr w, IntPtr l);
    public static void Save(IntPtr hwnd, string path) {
        Rect r; if (!GetWindowRect(hwnd, out r)) throw new Exception("Window bounds unavailable");
        using (var bitmap = new Bitmap(r.R-r.L, r.B-r.T)) {
            using (var g = Graphics.FromImage(bitmap)) {
                var dc = g.GetHdc();
                try { if (!PrintWindow(hwnd, dc, 2)) throw new Exception("Window capture failed"); }
                finally { g.ReleaseHdc(dc); }
            }
            bitmap.Save(path, System.Drawing.Imaging.ImageFormat.Png);
        }
    }
}
'@
$output = Join-Path $root 'docs/screenshots'
New-Item -ItemType Directory -Force -Path $output | Out-Null
# Never publish the user's real pairing identity in documentation.
$profile = Join-Path $root ('build/docs-profile-' + [guid]::NewGuid().ToString('N'))
$info = New-Object System.Diagnostics.ProcessStartInfo
$info.FileName = Join-Path $root 'dist/CameraBridge-Rust/CameraBridge.exe'
$info.UseShellExecute = $false
$info.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Hidden
$info.EnvironmentVariables['LOCALAPPDATA'] = $profile
$app = [System.Diagnostics.Process]::Start($info)
try {
    if (-not $app.WaitForInputIdle(5000)) { throw 'UI startup timed out' }
    Start-Sleep -Milliseconds 700
    $app.Refresh()
    $window = $app.MainWindowHandle
    if ($window -eq [IntPtr]::Zero) { throw 'Main window not found' }
    [DocsCapture]::Save($window, (Join-Path $output 'windows-pairing.png'))
    [DocsCapture]::SendMessage($window, 0x111, [IntPtr]105, [IntPtr]::Zero) | Out-Null
    Start-Sleep -Milliseconds 200
    [DocsCapture]::Save($window, (Join-Path $output 'windows-settings.png'))
} finally {
    if (-not $app.HasExited) {
        $app.CloseMainWindow() | Out-Null
        if (-not $app.WaitForExit(5000)) { $app.Kill(); $app.WaitForExit() }
    }
    $app.Dispose()
    $resolved = [IO.Path]::GetFullPath($profile)
    $build = [IO.Path]::GetFullPath((Join-Path $root 'build')) + [IO.Path]::DirectorySeparatorChar
    if (-not $resolved.StartsWith($build, [StringComparison]::OrdinalIgnoreCase)) { throw 'Unsafe profile cleanup path' }
    if (Test-Path -LiteralPath $resolved) { Remove-Item -LiteralPath $resolved -Recurse -Force }
}
