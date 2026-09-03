$ErrorActionPreference = "Stop"

function Initialize-MsvcBuildEnvironment {
  param(
    [Parameter(Mandatory = $true)]
    [string]$LogPrefix
  )

  $msvcToolchain = "stable-x86_64-pc-windows-msvc"
  $cargoCommand = Get-Command cargo -ErrorAction SilentlyContinue
  if ($cargoCommand) {
    $cargoBin = Split-Path -Parent $cargoCommand.Source
  }
  else {
    $toolchainBin = Join-Path $env:USERPROFILE ".rustup\toolchains\$msvcToolchain\bin"
    if (!(Test-Path (Join-Path $toolchainBin "cargo.exe"))) {
      throw "$LogPrefix cargo.exe was not found. Install Rust toolchain $msvcToolchain."
    }
    $cargoBin = $toolchainBin
  }

  if (Get-Command rustup -ErrorAction SilentlyContinue) {
    $hasToolchain = [bool]((rustup toolchain list) | Select-String -SimpleMatch $msvcToolchain)
    if (!$hasToolchain) {
      throw "$LogPrefix Rust toolchain $msvcToolchain is not installed. Run: rustup toolchain install $msvcToolchain"
    }
  }

  $env:RUSTUP_TOOLCHAIN = $msvcToolchain
  if ([string]::IsNullOrWhiteSpace($env:CARGO_BUILD_JOBS)) {
    $env:CARGO_BUILD_JOBS = "6"
  }
  Remove-Item Env:\CARGO_BUILD_TARGET -ErrorAction SilentlyContinue

  $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
  if (!(Test-Path $vswhere)) {
    throw "$LogPrefix Visual Studio Build Tools not found. Install Visual Studio 2022 Build Tools with C++ build tools."
  }

  $installPath = (& $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath)
  if ([string]::IsNullOrWhiteSpace($installPath)) {
    throw "$LogPrefix Visual C++ build tools were not found. Modify Visual Studio Build Tools and add C++ build tools."
  }

  $devCmd = Join-Path $installPath "Common7\Tools\VsDevCmd.bat"
  if (!(Test-Path $devCmd)) {
    throw "$LogPrefix VsDevCmd.bat not found at $devCmd"
  }

  Write-Host "$LogPrefix Initializing MSVC environment: $devCmd"
  $envDump = & cmd.exe /d /s /c "`"$devCmd`" -arch=x64 -host_arch=x64 >nul && set"

  foreach ($line in $envDump) {
    $parts = $line -split "=", 2
    if ($parts.Length -eq 2 -and ![string]::IsNullOrWhiteSpace($parts[0]) -and !$parts[0].StartsWith("=")) {
      [Environment]::SetEnvironmentVariable($parts[0], $parts[1], "Process")
    }
  }

  if (($env:Path -split ";") -notcontains $cargoBin) {
    $env:Path = "$cargoBin;$env:Path"
  }

  if (!(Get-Command link.exe -ErrorAction SilentlyContinue)) {
    throw "$LogPrefix MSVC link.exe is still not available after VsDevCmd initialization."
  }

  Write-Host "$LogPrefix Using Rust toolchain: $env:RUSTUP_TOOLCHAIN"
  Write-Host "$LogPrefix Using Cargo build jobs: $env:CARGO_BUILD_JOBS"
  Write-Host "$LogPrefix Using linker: $((Get-Command link.exe).Source)"
}

function Invoke-Checked {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Command,
    [Parameter(Mandatory = $true)]
    [string]$ErrorMessage
  )

  Invoke-Expression $Command
  if ($LASTEXITCODE -ne 0) {
    throw $ErrorMessage
  }
}

function Remove-UnsafeReleaseArtifacts {
  param([Parameter(Mandatory = $true)][string]$ReleaseDirectory)

  if (!(Test-Path -LiteralPath $ReleaseDirectory)) {
    return
  }
  foreach ($item in Get-ChildItem -LiteralPath $ReleaseDirectory -Force) {
    $allowedFile = !$item.PSIsContainer -and (
      $item.Name -eq "SwitchAI.exe" -or
      $item.Name -eq "SwitchAI.exe.sig" -or
      $item.Name -eq "latest.json" -or
      $item.Name -eq "SHA256SUMS.txt"
    )
    if (!$allowedFile) {
      Remove-Item -LiteralPath $item.FullName -Recurse -Force
    }
  }
}

function Assert-SafeReleaseArtifacts {
  param([Parameter(Mandatory = $true)][string]$ReleaseDirectory)

  $unexpected = @(Get-ChildItem -LiteralPath $ReleaseDirectory -Recurse -Force | Where-Object {
    $_.PSIsContainer -or (
      $_.Name -ne "SwitchAI.exe" -and
      $_.Name -ne "SwitchAI.exe.sig" -and
      $_.Name -ne "latest.json" -and
      $_.Name -ne "SHA256SUMS.txt"
    )
  })
  if ($unexpected.Count -gt 0) {
    $names = ($unexpected | ForEach-Object { $_.FullName }) -join ", "
    throw "Unsafe release artifacts detected: $names"
  }
}

$targetDir = "C:\Temp\switchai-msvc-target"
$repoRoot = Resolve-Path "$PSScriptRoot\.."
$distRoot = Join-Path $repoRoot "dist\release"
$nodeCommand = (Get-Command node -ErrorAction Stop).Source
$viteCli = Join-Path $repoRoot "ui\node_modules\vite\bin\vite.js"
$tauriCli = Join-Path $repoRoot "node_modules\@tauri-apps\cli\tauri.js"
$tauriBuildConfig = Join-Path $env:TEMP "switchai-tauri-build.json"

$exeSource = Join-Path $targetDir "release\switchai.exe"
$releaseExe = Join-Path $distRoot "SwitchAI.exe"
$uiDist = Join-Path $repoRoot "ui\dist"
$buildCompleted = $false

Write-Host "[build-release] Using CARGO_TARGET_DIR=$targetDir"
$env:CARGO_TARGET_DIR = $targetDir

if (!(Test-Path $viteCli)) {
  throw "Vite CLI not found at $viteCli. Run: npm --prefix ui install"
}

Set-Content -LiteralPath $tauriBuildConfig -Value '{"build":{"beforeBuildCommand":null}}' -Encoding ASCII

Write-Host "[build-release] Building frontend..."
Push-Location (Join-Path $repoRoot "ui")
try {
  & $nodeCommand $viteCli build
  if ($LASTEXITCODE -ne 0) {
    throw "Frontend build failed"
  }
}
finally {
  Pop-Location
}

Initialize-MsvcBuildEnvironment -LogPrefix "[build-release]"

Push-Location $repoRoot
try {
  New-Item -ItemType Directory -Force -Path $distRoot | Out-Null
  Remove-UnsafeReleaseArtifacts -ReleaseDirectory $distRoot

  $cleanupItems = @(
    (Join-Path $distRoot "*_portable.zip"),
    (Join-Path $distRoot "switchai.exe"),
    (Join-Path $distRoot "vg-codex-account-manager.exe"),
    (Join-Path $distRoot "SwitchAI*.exe"),
    (Join-Path $distRoot "Codex Account Manager*.exe"),
    (Join-Path $distRoot "README_PORTABLE.txt"),
    (Join-Path $distRoot "WebView2Loader.dll")
  )

  foreach ($item in $cleanupItems) {
    try {
      Remove-Item -Path $item -Recurse -Force -ErrorAction SilentlyContinue
    }
    catch {
      Write-Warning "[build-release] Could not remove old artifact: $item"
    }
  }

  Write-Host "[build-release] Building app with MSVC static WebView2 loader..."
  if (!(Test-Path $tauriCli)) {
    throw "Tauri CLI not found at $tauriCli. Run: npm install"
  }

  & $nodeCommand $tauriCli build --no-bundle --config $tauriBuildConfig
  if ($LASTEXITCODE -ne 0) {
    throw "Tauri build failed"
  }

  if (!(Test-Path $exeSource)) {
    throw "Built executable not found at $exeSource"
  }

  Copy-Item -Force $exeSource $releaseExe

  if (Test-Path (Join-Path $distRoot "WebView2Loader.dll")) {
    throw "Release artifact must not include WebView2Loader.dll"
  }

  Assert-SafeReleaseArtifacts -ReleaseDirectory $distRoot

  $buildCompleted = $true
  Write-Host "[build-release] Release executable: $releaseExe"
}
finally {
  Pop-Location
  Remove-Item -LiteralPath $tauriBuildConfig -Force -ErrorAction SilentlyContinue
  if ($buildCompleted -and (Test-Path -LiteralPath $targetDir)) {
    Remove-Item -LiteralPath $targetDir -Recurse -Force
    Write-Host "[build-release] Removed temporary build files: $targetDir"
  }
  if ($buildCompleted -and (Test-Path -LiteralPath $uiDist)) {
    Remove-Item -LiteralPath $uiDist -Recurse -Force
  }
}
