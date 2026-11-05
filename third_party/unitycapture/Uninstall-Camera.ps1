$ErrorActionPreference = 'Stop'
try {
    $principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'Run this script as administrator. Close video apps first.'
    }
    $destination = [IO.Path]::GetFullPath((Join-Path $env:ProgramFiles 'CameraBridge\Camera'))
    foreach ($bits in @(64,32)) {
        $dll = Join-Path $destination "UnityCaptureFilter$bits.dll"
        if (Test-Path -LiteralPath $dll) {
            $view = if ($bits -eq 64) { [Microsoft.Win32.RegistryView]::Registry64 } else { [Microsoft.Win32.RegistryView]::Registry32 }
            $id = if ($bits -eq 64) { '{5C2CD55C-92AD-4999-8666-912BD3E70010}' } else { '{5C2CD55C-92AD-4999-8666-912BD3E70020}' }
            $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::LocalMachine, $view)
            $key = $base.OpenSubKey("SOFTWARE\Classes\CLSID\$id\InprocServer32")
            try {
                if ($key -and [string]$key.GetValue('') -ne $dll) { throw 'Camera registration belongs to another installation. No changes made to that registration.' }
            } finally { if ($key) { $key.Dispose() }; $base.Dispose() }
            $regsvr = if ($bits -eq 64) { Join-Path $env:SystemRoot 'System32\regsvr32.exe' } else { Join-Path $env:SystemRoot 'SysWOW64\regsvr32.exe' }
            $process = Start-Process -FilePath $regsvr -ArgumentList @('/s','/u',('"' + $dll + '"')) -Wait -PassThru -WindowStyle Hidden
            if ($process.ExitCode -ne 0) { throw "Unregistration failed ($bits bit). Close camera apps and retry." }
            Remove-Item -LiteralPath $dll
        }
    }
    Get-NetFirewallRule -Name 'CameraBridge-Receiver' -ErrorAction SilentlyContinue | Remove-NetFirewallRule
    Write-Host 'CameraBridge webcam removed.' -ForegroundColor Green
} catch { Write-Host $_.Exception.Message -ForegroundColor Red; Read-Host 'Press Enter to close'; exit 1 }
Read-Host 'Press Enter to close'
