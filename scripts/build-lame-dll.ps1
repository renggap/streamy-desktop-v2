# build-lame-dll.ps1 - PowerShell script to compile libmp3lame.dll on Windows (MSVC or clang-cl)
# Required to satisfy LGPL §6 (source offer and ability to rebuild/relink the shared library).

param(
    [string]$Configuration = "Release",
    [string]$Platform = "x64",
    [string]$OutputDir = "$PSScriptRoot\..\target\lame-dist"
)

$ErrorActionPreference = "Stop"

$LameCrate = "$PSScriptRoot\..\crates\streamy-lame"
$LameRoot = "$LameCrate\vendor\lame"
$LibSrc = "$LameRoot\libmp3lame"
$IncDir = "$LameRoot\include"

Write-Host "Building libmp3lame.dll ($Platform $Configuration)..." -ForegroundColor Cyan

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

$CmakeLists = "$LameCrate\CMakeLists.txt"

# Check if cmake and CMakeLists.txt are available
if ((Test-Path $CmakeLists) -and (Get-Command cmake -ErrorAction SilentlyContinue)) {
    Write-Host "Using CMake..." -ForegroundColor Green
    $BuildDir = "$PSScriptRoot\..\target\lame-cmake-build"
    New-Item -ItemType Directory -Force -Path $BuildDir | Out-Null
    
    # Configure with CMake
    & cmake -B $BuildDir -S $LameCrate -A $Platform -DCMAKE_BUILD_TYPE=$Configuration
    if ($LASTEXITCODE -ne 0) {
        # Retry without -A in case generator is Ninja or does not support -A
        & cmake -B $BuildDir -S $LameCrate -DCMAKE_BUILD_TYPE=$Configuration
    }
    if ($LASTEXITCODE -ne 0) {
        throw "CMake configuration failed with exit code $LASTEXITCODE"
    }

    # Build with CMake
    & cmake --build $BuildDir --config $Configuration
    if ($LASTEXITCODE -ne 0) {
        throw "CMake build failed with exit code $LASTEXITCODE"
    }

    # Search and copy built DLL and LIB
    $PossibleDllPaths = @(
        "$BuildDir\$Configuration\libmp3lame.dll",
        "$BuildDir\libmp3lame.dll",
        "$BuildDir\Release\libmp3lame.dll"
    )
    $PossibleLibPaths = @(
        "$BuildDir\$Configuration\mp3lame.lib",
        "$BuildDir\mp3lame.lib",
        "$BuildDir\Release\mp3lame.lib"
    )

    foreach ($dll in $PossibleDllPaths) {
        if (Test-Path $dll) {
            Copy-Item $dll "$OutputDir\libmp3lame.dll" -Force
            break
        }
    }

    foreach ($lib in $PossibleLibPaths) {
        if (Test-Path $lib) {
            Copy-Item $lib "$OutputDir\mp3lame.lib" -Force
            break
        }
    }
} elseif (Get-Command cl -ErrorAction SilentlyContinue) {
    Write-Host "Using MSVC cl.exe..." -ForegroundColor Green
    $ObjDir = "$PSScriptRoot\..\target\lame-objs"
    New-Item -ItemType Directory -Force -Path $ObjDir | Out-Null

    $Sources = Get-ChildItem -Path $LibSrc -Filter "*.c" | Where-Object {
        $_.Name -ne "mpglib_interface.c"
    } | Select-Object -ExpandProperty FullName

    $DefFile = "$LameCrate\lame.def"
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

$FinalDll = "$OutputDir\libmp3lame.dll"
if (-not (Test-Path $FinalDll)) {
    throw "libmp3lame.dll was not found at $FinalDll after build."
}

# Distribute DLL to cargo target folders for immediate runtime availability
$TargetDirs = @(
    "$PSScriptRoot\..\target\release",
    "$PSScriptRoot\..\target\debug",
    "$PSScriptRoot\..\src-tauri\target\release",
    "$PSScriptRoot\..\src-tauri\target\debug"
)

foreach ($dir in $TargetDirs) {
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    Copy-Item $FinalDll "$dir\libmp3lame.dll" -Force -ErrorAction SilentlyContinue
    if (Test-Path "$OutputDir\mp3lame.lib") {
        Copy-Item "$OutputDir\mp3lame.lib" "$dir\mp3lame.lib" -Force -ErrorAction SilentlyContinue
    }
}

Write-Host "libmp3lame.dll successfully built at: $FinalDll" -ForegroundColor Green
