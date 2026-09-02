param(
  [switch]$OpenVirusTotal = $true,
  [switch]$Rebuild = $false
)

$ErrorActionPreference = "Stop"
$repoRoot = Resolve-Path "$PSScriptRoot\.."
$releaseExe = Join-Path $repoRoot "dist\release\SwitchAI.exe"

if ($Rebuild -or !(Test-Path -LiteralPath $releaseExe)) {
  Write-Host "[submit-wdsi] Release binary not found or rebuild requested. Building release..." -ForegroundColor Cyan
  & powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "build-release.ps1")
  if ($LASTEXITCODE -ne 0 -or !(Test-Path -LiteralPath $releaseExe)) {
    throw "[submit-wdsi] Build failed. Cannot submit."
  }
}

$hasher = [System.Security.Cryptography.SHA256]::Create()
$stream = [System.IO.File]::OpenRead($releaseExe)
$hashBytes = $hasher.ComputeHash($stream)
$stream.Close()
$hash = [BitConverter]::ToString($hashBytes) -replace '-'
$fileSizeMb = [math]::Round(((Get-Item -LiteralPath $releaseExe).Length / 1MB), 2)

try {
  $releaseExe | clip
} catch {}

Write-Host "`n========================================================" -ForegroundColor Green
Write-Host " SwitchAI Defender & VirusTotal Submission Helper" -ForegroundColor Green
Write-Host "========================================================" -ForegroundColor Green
Write-Host " File:      $releaseExe" -ForegroundColor White
Write-Host " Size:      $fileSizeMb MB" -ForegroundColor White
Write-Host " SHA-256:   $hash" -ForegroundColor Yellow
Write-Host " Clipboard: Path copied to clipboard!" -ForegroundColor Cyan
Write-Host "--------------------------------------------------------" -ForegroundColor Gray
Write-Host " Opening Microsoft Security Intelligence portal in browser..." -ForegroundColor Cyan

$wdsiUrl = "https://www.microsoft.com/en-us/wdsi/filesubmission"
Start-Process $wdsiUrl

if ($OpenVirusTotal) {
  Write-Host " Opening VirusTotal in browser..." -ForegroundColor Cyan
  $vtUrl = "https://www.virustotal.com/gui/home/upload"
  Start-Process $vtUrl
}

Start-Process explorer.exe -ArgumentList "/select,`"$releaseExe`""

Write-Host "`nInstructions for Microsoft Defender Whitelisting:" -ForegroundColor White
Write-Host " 1. Select 'Software developer' -> 'Incorrectly detected as malware (false positive)'" -ForegroundColor White
Write-Host " 2. Click 'Browse file' and paste the path from clipboard (Ctrl+V)" -ForegroundColor White
Write-Host " 3. Enter product name: SwitchAI Account Manager v1.0.0" -ForegroundColor White
Write-Host " 4. Detection name: Trojan:Win32/Wacatac.C!ml (False Positive)" -ForegroundColor White
Write-Host " 5. Click Submit." -ForegroundColor White
Write-Host "`nMicrosoft automatically verifies and whitelists clean hashes within 1-3 hours." -ForegroundColor Green
Write-Host "========================================================`n" -ForegroundColor Green
