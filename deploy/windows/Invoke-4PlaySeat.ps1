param(
  [Parameter(Mandatory = $true)]
  [ValidateRange(1, 4)]
  [int] $SeatNumber
)

$ErrorActionPreference = "Stop"

function Get-4PlaySetting {
  param(
    [Parameter(Mandatory = $true)]
    [string] $Name,

    [Parameter(Mandatory = $true)]
    [string] $Default
  )

  $Value = [Environment]::GetEnvironmentVariable($Name, "Process")
  if ([string]::IsNullOrWhiteSpace($Value)) {
    $Value = [Environment]::GetEnvironmentVariable($Name, "User")
  }
  if ([string]::IsNullOrWhiteSpace($Value)) {
    $Value = [Environment]::GetEnvironmentVariable($Name, "Machine")
  }
  if ([string]::IsNullOrWhiteSpace($Value)) {
    return $Default
  }
  return $Value.Trim()
}

$RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$SeatInput = Join-Path $RepoRoot "target\release\seat-input.exe"

if (-not (Test-Path -LiteralPath $SeatInput)) {
  throw "seat-input.exe was not found at $SeatInput. Build it first with: cargo build --release -p seat-input"
}

$ControlPlane = Get-4PlaySetting "FOURPLAY_CONTROL_PLANE_URL" "http://192.168.20.68:8080"
$ApiToken = Get-4PlaySetting "FOURPLAY_SEAT_API_TOKEN" "phase-1c-seat-token-2026"
$SeatAddress = Get-4PlaySetting "FOURPLAY_SEAT_ADDRESS" "192.168.20.10"
$FfplayPath = Get-4PlaySetting "FOURPLAY_FFPLAY_PATH" ""
$SeatId = Get-4PlaySetting "FOURPLAY_SEAT_ID" "windows-seat-$SeatNumber"

$Arguments = @(
  "--control-plane", $ControlPlane,
  "--api-token", $ApiToken,
  "--seat-id", $SeatId,
  "--destination-ip", $SeatAddress
)

if (-not [string]::IsNullOrWhiteSpace($FfplayPath)) {
  $Arguments += @("--ffplay-path", $FfplayPath)
}

Write-Host "Starting 4-Play seat $SeatNumber as $SeatId"
Write-Host "  control plane: $ControlPlane"
Write-Host "  media destination: $SeatAddress"
if (-not [string]::IsNullOrWhiteSpace($FfplayPath)) {
  Write-Host "  ffplay: $FfplayPath"
}

& $SeatInput @Arguments
