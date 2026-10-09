param(
  [switch] $CurrentUser
)

$ErrorActionPreference = "Stop"

if (-not $CurrentUser) {
  Write-Host "This helper enables local 4-Play seat launcher scripts for the current Windows user."
  Write-Host "It sets the PowerShell execution policy to RemoteSigned at CurrentUser scope."
  Write-Host ""
  Write-Host "Re-run with -CurrentUser to apply:"
  Write-Host "  .\deploy\windows\Enable-4PlaySeatLaunchers.ps1 -CurrentUser"
  exit 0
}

Set-ExecutionPolicy -Scope CurrentUser -ExecutionPolicy RemoteSigned -Force

Write-Host "PowerShell CurrentUser execution policy is now:"
Get-ExecutionPolicy -List | Format-Table -AutoSize
Write-Host ""
Write-Host "You can now start seats without the per-window process bypass:"
Write-Host "  .\deploy\windows\Start-4Play-Seat1.ps1"
