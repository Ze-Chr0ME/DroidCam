param([string]$ReceiverPath)
$ErrorActionPreference = 'Stop'
try {
    $principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'Run this setup as administrator, or use Set up Windows webcam in CameraBridge.'
    }
    $destination = Join-Path $env:ProgramFiles 'CameraBridge\Camera'
    $classIds = @('{5C2CD55C-92AD-4999-8666-912BD3E70010}', '{5C2CD55C-92AD-4999-8666-912BD3E70020}')
    # Upstream shares its registration identity with UnityCapture. Never replace an existing installation.
    foreach ($view in @([Microsoft.Win32.RegistryView]::Registry64, [Microsoft.Win32.RegistryView]::Registry32)) {
        $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::LocalMachine, $view)
        try {
            foreach ($classId in $classIds) {
                $key = $base.OpenSubKey("SOFTWARE\Classes\CLSID\$classId\InprocServer32")
                if ($key) {
                    try {
                        $existing = [string]$key.GetValue('')
                        if ($existing -and -not $existing.StartsWith($destination + '\', [StringComparison]::OrdinalIgnoreCase)) {
                            throw 'Another UnityCapture installation is present. Remove it using its own uninstaller before setting up CameraBridge.'
                        }
                    } finally { $key.Dispose() }
                }
            }
        } finally { $base.Dispose() }
    }
    New-Item -ItemType Directory -Force -Path $destination | Out-Null
    foreach ($bits in @(64,32)) {
        $dllName = "UnityCaptureFilter$bits.dll"
        $dll = Join-Path $destination $dllName
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot $dllName) -Destination $dll -Force
        $regsvr = if ($bits -eq 64) { Join-Path $env:SystemRoot 'System32\regsvr32.exe' } else { Join-Path $env:SystemRoot 'SysWOW64\regsvr32.exe' }
        $process = Start-Process -FilePath $regsvr -ArgumentList @('/s', '/i:UnityCaptureName=CameraBridge', ('"' + $dll + '"')) -Wait -PassThru -WindowStyle Hidden
        if ($process.ExitCode -ne 0) { throw "Camera registration failed for $bits-bit apps (code $($process.ExitCode))." }
    }
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'Uninstall-Camera.ps1') -Destination $destination -Force
    if ($ReceiverPath -and (Test-Path -LiteralPath $ReceiverPath -PathType Leaf)) {
        $rule = Get-NetFirewallRule -Name 'CameraBridge-Receiver' -ErrorAction SilentlyContinue
        if ($rule) { $rule | Remove-NetFirewallRule }
        New-NetFirewallRule -Name 'CameraBridge-Receiver' -DisplayName 'CameraBridge local phone video' -Direction Inbound -Action Allow -Protocol TCP -LocalPort 47950 -Program $ReceiverPath -Profile Private -RemoteAddress LocalSubnet | Out-Null
    }
    Write-Host 'CameraBridge webcam installed. Restart your video apps, then select CameraBridge.' -ForegroundColor Green
} catch {
    Write-Host $_.Exception.Message -ForegroundColor Red
    Read-Host 'Press Enter to close'
    exit 1
}
Read-Host 'Press Enter to close'
