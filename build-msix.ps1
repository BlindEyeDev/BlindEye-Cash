#!/usr/bin/env pwsh
# BlindEye MSIX Packaging Script
# Usage: .\build-msix.ps1 [-SignCertificatePath <path>] [-SignCertificatePassword <password>]

param(
    [string]$SignCertificatePath = "",
    [string]$SignCertificatePassword = ""
)

$ErrorActionPreference = "Stop"

Write-Host "[MSIX] Building BlindEye MSIX package..." -ForegroundColor Cyan

# Build the Rust binary in release mode
Write-Host "[MSIX] Building Rust binary..." -ForegroundColor Yellow
$cargoCommand = Get-Command "cargo.exe" -ErrorAction SilentlyContinue
if ($null -eq $cargoCommand) {
    $fallbackCargoPath = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
    if (Test-Path $fallbackCargoPath) {
        $cargoPath = $fallbackCargoPath
    } else {
        Write-Error "cargo.exe not found. Please install Rust or add cargo to PATH."
    }
} else {
    $cargoPath = $cargoCommand.Source
}

$cargoProcess = Start-Process -FilePath $cargoPath -ArgumentList @("build", "--release") -NoNewWindow -Wait -PassThru
if ($cargoProcess.ExitCode -ne 0) {
    Write-Error "Cargo build failed"
}

# Prepare MSIX folder structure
$msixDir = "msix_build"
$packageDir = "$msixDir\BlindEyeWallet"

if (Test-Path $msixDir) {
    Remove-Item $msixDir -Recurse -Force
}

New-Item -ItemType Directory -Path "$packageDir\Assets" -Force | Out-Null

Write-Host "[MSIX] Copying files..." -ForegroundColor Yellow

# Copy binary
Copy-Item "target\release\blindeye.exe" -Destination $packageDir -Force

# Copy manifest
Copy-Item "Package.appxmanifest" -Destination $packageDir -Force

# Create placeholder assets (you should replace these with proper icons)
if (Test-Path "assets") {
    Copy-Item "assets\*" -Destination "$packageDir\Assets" -Force
} else {
    Write-Host "[MSIX] Warning: assets folder not found. Creating placeholder assets." -ForegroundColor Yellow
    
    # Create a simple PNG placeholder (1x1 transparent)
    $pngBytes = @(0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
                  0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
                  0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0x00, 0x01, 0x00, 0x00,
                  0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
                  0x42, 0x60, 0x82)
    
    [System.IO.File]::WriteAllBytes("$packageDir\Assets\Square150x150Logo.png", $pngBytes)
    [System.IO.File]::WriteAllBytes("$packageDir\Assets\Square44x44Logo.png", $pngBytes)
    [System.IO.File]::WriteAllBytes("$packageDir\Assets\StoreLogo.png", $pngBytes)
    [System.IO.File]::WriteAllBytes("$packageDir\Assets\SplashScreen.png", $pngBytes)
}

# Get package version from Cargo.toml
$cargoToml = Get-Content "Cargo.toml" -Raw
$versionMatch = [regex]::Match($cargoToml, 'version\s*=\s*"([^"]+)"')
if ($versionMatch.Success) {
    $version = $versionMatch.Groups[1].Value
    Write-Host "[MSIX] Using version: $version" -ForegroundColor Green
} else {
    $version = "0.1.0"
    Write-Host "[MSIX] Version not found in Cargo.toml, using default: $version" -ForegroundColor Yellow
}

# Get Windows 10 SDK path
$sdkPath = Get-ItemProperty -Path "HKLM:\Software\Microsoft\Windows Kits\Installed Roots" -Name "KitsRoot10" -ErrorAction SilentlyContinue
if ($null -eq $sdkPath) {
    Write-Error "Windows 10 SDK not found. Please install Windows 10 SDK to build MSIX packages."
}

$sdkRoot = $sdkPath.KitsRoot10
$makeAppx = "$sdkRoot\bin\*\makeappx.exe"
$makeAppxPath = (Resolve-Path $makeAppx -ErrorAction SilentlyContinue | Select-Object -First 1).Path

if ([string]::IsNullOrEmpty($makeAppxPath) -or -not (Test-Path $makeAppxPath)) {
    Write-Error "makeappx.exe not found in Windows SDK. Please ensure Windows 10 SDK is properly installed."
}

# Create MSIX package
$msixPath = "BlindEyeWallet-$version-x64.msix"
Write-Host "[MSIX] Creating MSIX package: $msixPath" -ForegroundColor Yellow

& $makeAppxPath pack /d $packageDir /p $msixPath /o 2>&1
if ($LASTEXITCODE -ne 0) {
    Write-Error "makeappx.exe failed to create package"
}

# Sign the package if certificate provided
if (-not [string]::IsNullOrEmpty($SignCertificatePath) -and (Test-Path $SignCertificatePath)) {
    Write-Host "[MSIX] Signing MSIX package..." -ForegroundColor Yellow
    
    $signtool = "$sdkRoot\bin\*\signtool.exe"
    $signtoolPath = (Resolve-Path $signtool -ErrorAction SilentlyContinue | Select-Object -First 1).Path
    
    if ([string]::IsNullOrEmpty($signtoolPath) -or -not (Test-Path $signtoolPath)) {
        Write-Warning "signtool.exe not found. Package will not be signed."
    } else {
        $signArgs = @("sign", "/f", $SignCertificatePath, "/p", $SignCertificatePassword, "/fd", "SHA256", $msixPath)
        & $signtoolPath @signArgs 2>&1
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "Package signing failed, but MSIX was created unsigned."
        } else {
            Write-Host "[MSIX] Package signed successfully" -ForegroundColor Green
        }
    }
}

# Cleanup
Remove-Item $msixDir -Recurse -Force

Write-Host "[MSIX] MSIX package created successfully: $msixPath" -ForegroundColor Green
Write-Host "[MSIX] To install: Add-AppxPackage -Path $msixPath" -ForegroundColor Cyan
