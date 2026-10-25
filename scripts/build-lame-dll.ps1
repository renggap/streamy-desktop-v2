# build-lame-dll.ps1 - PowerShell script to compile libmp3lame.dll on Windows (MSVC or clang-cl)
# Required to satisfy LGPL §6 (source offer and ability to rebuild/relink the shared library).

param(
    [string]$Configuration = "Release",
    [string]$Platform = "x64",
    [string]$OutputDir = "$PSScriptRoot\..\target\lame-dist"
)

$ErrorActionPreference = "Stop"

$LameRoot = "$PSScriptRoot\..\crates\streamy-lame\vendor\lame"
$LibSrc = "$LameRoot\libmp3lame"
$IncDir = "$LameRoot\include"

Write-Host "Building libmp3lame.dll ($Platform $Configuration)..." -ForegroundColor Cyan

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

$Sources = Get-ChildItem -Path $LibSrc -Filter "*.c" | Where-Object {
    # Exclude mpglib_interface.c if present (HAVE_MPGLIB not defined)
    $_.Name -ne "mpglib_interface.c"
} | Select-Object -ExpandProperty FullName

$DefFile = "$PSScriptRoot\..\crates\streamy-lame\lame.def"

# Check if cmake is available
if (Get-Command cmake -ErrorAction SilentlyContinue) {
    Write-Host "Using CMake..." -ForegroundColor Green
    $BuildDir = "$PSScriptRoot\..\target\lame-cmake-build"
    New-Item -ItemType Directory -Force -Path $BuildDir | Out-Null
    
    cmake -B $BuildDir -S "$PSScriptRoot\..\crates\streamy-lame" -DCMAKE_BUILD_TYPE=$Configuration
    cmake --build $BuildDir --config $Configuration
    
    Copy-Item "$BuildDir\$Configuration\libmp3lame.dll" "$OutputDir\libmp3lame.dll" -ErrorAction SilentlyContinue
    Copy-Item "$BuildDir\libmp3lame.dll" "$OutputDir\libmp3lame.dll" -ErrorAction SilentlyContinue
    Copy-Item "$BuildDir\$Configuration\mp3lame.lib" "$OutputDir\mp3lame.lib" -ErrorAction SilentlyContinue
    Copy-Item "$BuildDir\mp3lame.lib" "$OutputDir\mp3lame.lib" -ErrorAction SilentlyContinue
} elseif (Get-Command cl -ErrorAction SilentlyContinue) {
    Write-Host "Using MSVC cl.exe..." -ForegroundColor Green
    $ObjDir = "$PSScriptRoot\..\target\lame-objs"
    New-Item -ItemType Directory -Force -Path $ObjDir | Out-Null
    
    $CFlags = "/O2 /W0 /DHAVE_CONFIG_H /I`"$LibSrc`" /I`"$IncDir`" /c /Fo`"$ObjDir\\`""
    foreach ($src in $Sources) {
        Invoke-Expression "cl $CFlags `"$src`""
    }
    
    $Objs = Get-ChildItem -Path $ObjDir -Filter "*.obj" | Select-Object -ExpandProperty FullName
    $LinkCmd = "link /DLL /DEF:`"$DefFile`" /OUT:`"$OutputDir\libmp3lame.dll`" /IMPLIB:`"$OutputDir\mp3lame.lib`" " + ($Objs -join " ")
    Invoke-Expression $LinkCmd
} else {
    Write-Error "Neither cmake nor cl.exe found in PATH. Please run from VS Native Tools Command Prompt or install CMake."
}

Write-Host "libmp3lame.dll successfully built at: $OutputDir\libmp3lame.dll" -ForegroundColor Green
