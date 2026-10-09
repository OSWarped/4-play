$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$SeatInput = Join-Path $RepoRoot "target\release\seat-input.exe"

& $SeatInput `
  --control-plane "http://192.168.20.68:8080" `
  --api-token "phase-1c-seat-token-2026" `
  --seat-id "windows-seat-3" `
  --destination-ip "192.168.20.10"

