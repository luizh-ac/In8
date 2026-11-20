# Opens a release build of In8 inside Windows Sandbox, to try tweaks and debloat without touching this PC.
# Windows Sandbox must be turned on (Windows features → "Windows Sandbox").
#
#   .\scripts\sandbox.ps1            build, then open the sandbox
#   .\scripts\sandbox.ps1 -NoBuild   reuse the last release build
#   .\scripts\sandbox.ps1 -NoLaunch  only prepare the files
#
# The sandbox is a minimal Windows: no Microsoft Store and almost none of the preinstalled apps,
# so Debloat has little to show there. Everything inside is thrown away when the window closes.
param(
    [switch]$NoBuild,
    [switch]$NoLaunch
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

if (-not $NoBuild) {
    Push-Location $root
    try {
        npm run tauri build -- --no-bundle
        if ($LASTEXITCODE -ne 0) { throw "Build failed." }
    } finally {
        Pop-Location
    }
}

$exe = Join-Path $root "src-tauri\target\release\in8.exe"
if (-not (Test-Path $exe)) { throw "No release build at $exe. Run without -NoBuild first." }

# Only the executable is shared with the sandbox, read-only.
$shared = Join-Path $root "src-tauri\target\sandbox"
New-Item -ItemType Directory -Force -Path $shared | Out-Null
Copy-Item $exe (Join-Path $shared "in8.exe") -Force

$config = Join-Path $root "src-tauri\target\in8-sandbox.wsb"
$xml = @"
<Configuration>
  <MappedFolders>
    <MappedFolder>
      <HostFolder>$([System.Security.SecurityElement]::Escape($shared))</HostFolder>
      <SandboxFolder>C:\In8</SandboxFolder>
      <ReadOnly>true</ReadOnly>
    </MappedFolder>
  </MappedFolders>
  <LogonCommand>
    <Command>C:\In8\in8.exe</Command>
  </LogonCommand>
</Configuration>
"@
Set-Content -Path $config -Value $xml -Encoding utf8
Write-Output "Sandbox config: $config"

if (-not $NoLaunch) {
    Invoke-Item $config
}
