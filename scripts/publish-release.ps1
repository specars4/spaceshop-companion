#requires -Version 5.1
<#
.SYNOPSIS
  Generate latest.json from the last `npm run tauri build` output, ready for
  upload to GitHub Releases.

.DESCRIPTION
  Reads version from src-tauri/tauri.conf.json, finds the matching MSI and
  .msi.sig in the bundle output, emits a latest.json next to them, and
  optionally invokes `gh release create` to publish.

  Prerequisite: you've already run a release build with the signing key set:
    $env:TAURI_SIGNING_PRIVATE_KEY = Get-Content "<path>" -Raw
    npm run tauri build

.PARAMETER GhOrg
  Your GitHub org or username (e.g. "spaceshop-studios"). Substituted into
  the manifest URL.

.PARAMETER Notes
  Plain-language release notes shown in the contractor's update banner.
  Keep to one or two sentences.

.PARAMETER Publish
  If passed, runs `gh release create` to upload the .msi + latest.json to a
  new GitHub Release. Requires the `gh` CLI to be installed and authed.

.EXAMPLE
  pwsh ./scripts/publish-release.ps1 -GhOrg spaceshop-studios -Notes "Fixes p4tickets read-only bug." -Publish
#>

param(
  [Parameter(Mandatory=$true)][string]$GhOrg,
  [Parameter(Mandatory=$true)][string]$Notes,
  [switch]$Publish
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$confPath = Join-Path $repoRoot "src-tauri/tauri.conf.json"

if (-not (Test-Path $confPath)) {
  throw "tauri.conf.json not found at $confPath"
}

$conf = Get-Content $confPath -Raw | ConvertFrom-Json
$version = $conf.version
Write-Host "Version: $version"

$bundleDir = Join-Path $repoRoot "src-tauri/target/release/bundle/msi"
$msiName = "Spaceshop Companion_${version}_x64_en-US.msi"
$msiPath = Join-Path $bundleDir $msiName
$sigPath = "$msiPath.sig"

if (-not (Test-Path $msiPath)) { throw "MSI not found at $msiPath. Run 'npm run tauri build' first." }
if (-not (Test-Path $sigPath)) { throw "Signature not found at $sigPath. Did you set TAURI_SIGNING_PRIVATE_KEY?" }

$signature = (Get-Content $sigPath -Raw).Trim()
$pubDate = (Get-Date -AsUTC).ToString("yyyy-MM-ddTHH:mm:ssZ")
$msiUrl = "https://github.com/$GhOrg/spaceshop-companion/releases/download/v$version/" + ($msiName -replace ' ', '_')

$manifest = [ordered]@{
  version  = $version
  notes    = $Notes
  pub_date = $pubDate
  platforms = [ordered]@{
    "windows-x86_64" = [ordered]@{
      signature = $signature
      url       = $msiUrl
    }
  }
}

$manifestPath = Join-Path $bundleDir "latest.json"
$manifest | ConvertTo-Json -Depth 10 | Out-File $manifestPath -Encoding utf8
Write-Host "Wrote $manifestPath"

if ($Publish) {
  $tag = "v$version"
  $title = "Companion v$version"
  $msiPublishName = $msiName -replace ' ', '_'
  $publishMsi = Join-Path $bundleDir $msiPublishName
  Copy-Item $msiPath $publishMsi -Force

  $args = @(
    "release", "create", $tag,
    "--title", $title,
    "--notes", $Notes,
    $publishMsi,
    $manifestPath
  )
  Write-Host "Running: gh $($args -join ' ')"
  & gh @args
} else {
  Write-Host "Skipped gh release create (run with -Publish to upload)."
  Write-Host "  MSI:      $msiPath"
  Write-Host "  Manifest: $manifestPath"
  Write-Host "Upload both to a new GitHub Release tagged v$version manually if you prefer."
}
