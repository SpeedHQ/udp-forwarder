$ErrorActionPreference = "Stop"

$Repo = "SpeedHQ/udp-forwarder"
$Target = "x86_64-pc-windows-msvc"
$InstallDir = "$env:LOCALAPPDATA\udp-forwarder"

Write-Host "Fetching latest release..."
$Release = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest"
$Version = $Release.tag_name -replace '^v', ''

$Filename = "udp-forwarder-v$Version-$Target.zip"
$Asset = $Release.assets | Where-Object { $_.name -eq $Filename }

if (-not $Asset) {
    Write-Host "Release asset not found: $Filename" -ForegroundColor Red
    exit 1
}

$TmpDir = Join-Path $env:TEMP "udp-forwarder-install"
if (Test-Path $TmpDir) { Remove-Item $TmpDir -Recurse -Force }
New-Item -ItemType Directory -Path $TmpDir | Out-Null

Write-Host "Downloading udp-forwarder v$Version..."
Invoke-WebRequest $Asset.browser_download_url -OutFile "$TmpDir\$Filename"

Write-Host "Extracting..."
Expand-Archive "$TmpDir\$Filename" -DestinationPath $TmpDir -Force

# Install
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir | Out-Null
}

Copy-Item "$TmpDir\udp-forwarder.exe" "$InstallDir\udp-forwarder.exe" -Force

# Copy default config to current directory if none exists
if (-not (Test-Path ".\config.ini")) {
    Copy-Item "$TmpDir\config.ini" ".\config.ini"
    Write-Host "Created config.ini in current directory - edit it before running."
}

# Clean up
Remove-Item $TmpDir -Recurse -Force

Write-Host ""
Write-Host "Installed udp-forwarder v$Version to $InstallDir\udp-forwarder.exe"

# Add to PATH if not present
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    $AddToPath = Read-Host "Add to PATH? (Y/n)"
    if ($AddToPath -ne "n") {
        [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
        Write-Host "Added to PATH. Restart your terminal for changes to take effect."
    }
}
