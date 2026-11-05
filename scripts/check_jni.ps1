$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    cargo build --locked -p camerabridge-android
    if ($LASTEXITCODE -ne 0) { throw 'JNI build failed' }
    $classes = Join-Path $root 'build\jni-tests'
    New-Item -ItemType Directory -Force -Path $classes | Out-Null
    javac -d $classes android/app/src/main/java/app/camerabridge/NativeBridge.java tests/jni/app/camerabridge/CameraEngine.java tests/jni/app/camerabridge/CameraService.java
    if ($LASTEXITCODE -ne 0) { throw 'JNI fixture compilation failed' }
    java -Xcheck:jni "-Djava.library.path=$root\target\debug" -cp $classes app.camerabridge.CameraService
    if ($LASTEXITCODE -ne 0) { throw 'JNI runtime checks failed' }
} finally { Pop-Location }
