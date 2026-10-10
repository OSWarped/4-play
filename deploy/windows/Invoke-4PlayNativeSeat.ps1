param(
  [Parameter(Mandatory = $true)]
  [ValidateRange(1, 4)]
  [int] $SeatNumber,

  [string] $Bind = ""
)

$ErrorActionPreference = "Stop"

function Get-4PlaySetting {
  param(
    [Parameter(Mandatory = $true)]
    [string] $Name,

    [AllowEmptyString()]
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
$ReleaseNativeSeat = Join-Path $RepoRoot "target\release\seat-client-native.exe"
$DebugNativeSeat = Join-Path $RepoRoot "target\debug\seat-client-native.exe"
$NativeSeat = $ReleaseNativeSeat
if (-not (Test-Path -LiteralPath $NativeSeat)) {
  $NativeSeat = $DebugNativeSeat
}

if (-not (Test-Path -LiteralPath $NativeSeat)) {
  throw "seat-client-native.exe was not found. Build it first with: cargo build --release -p seat-client-native"
}

$ControlPlane = Get-4PlaySetting "FOURPLAY_CONTROL_PLANE_URL" "http://192.168.20.68:8080"
$ApiToken = Get-4PlaySetting "FOURPLAY_SEAT_API_TOKEN" ""
$SeatAddress = Get-4PlaySetting "FOURPLAY_SEAT_ADDRESS" "192.168.20.10"
$FfplayPath = Get-4PlaySetting "FOURPLAY_FFPLAY_PATH" ""
$SeatId = Get-4PlaySetting "FOURPLAY_SEAT_ID" "windows-seat-$SeatNumber"
$ConfigDir = Join-Path $RepoRoot "tmp\native-seat"
$ConfigPath = Join-Path $ConfigDir "seat-$SeatNumber.json"

if ([string]::IsNullOrWhiteSpace($ApiToken)) {
  throw "FOURPLAY_SEAT_API_TOKEN is required. Set it in this PowerShell session before launching the native seat UI."
}

if ([string]::IsNullOrWhiteSpace($Bind)) {
  $BindPort = 40703 + $SeatNumber
  $Bind = "127.0.0.1:$BindPort"
}

$InitArguments = @(
  "init-config",
  "--config", $ConfigPath,
  "--control-plane", $ControlPlane,
  "--seat-id", $SeatId,
  "--destination-ip", $SeatAddress
)

if (-not [string]::IsNullOrWhiteSpace($FfplayPath)) {
  $InitArguments += @("--ffplay-path", $FfplayPath)
}

& $NativeSeat @InitArguments

$env:FOURPLAY_SEAT_API_TOKEN = $ApiToken

Write-Host "Starting 4-Play native seat UI $SeatNumber as $SeatId"
Write-Host "  native bridge: $NativeSeat"
Write-Host "  control plane: $ControlPlane"
Write-Host "  media destination: $SeatAddress"
Write-Host "  local UI: http://$Bind"
if (-not [string]::IsNullOrWhiteSpace($FfplayPath)) {
  Write-Host "  ffplay: $FfplayPath"
}

& $NativeSeat "serve-ui" "--config" $ConfigPath "--bind" $Bind
