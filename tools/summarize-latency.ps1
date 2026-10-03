param(
    [Parameter(Mandatory = $true)]
    [string]$Path,

    [string]$OutputPath
)

$ErrorActionPreference = "Stop"

function Get-Percentile {
    param(
        [double[]]$Values,
        [double]$Percentile
    )

    $sorted = @($Values | Sort-Object)
    if ($sorted.Count -eq 0) {
        return [double]::NaN
    }
    if ($sorted.Count -eq 1) {
        return $sorted[0]
    }

    $position = ($sorted.Count - 1) * $Percentile
    $lower = [math]::Floor($position)
    $upper = [math]::Ceiling($position)

    if ($lower -eq $upper) {
        return $sorted[$lower]
    }

    $weight = $position - $lower
    return $sorted[$lower] + (($sorted[$upper] - $sorted[$lower]) * $weight)
}

if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
    throw "Latency sample file not found: $Path"
}

$measurements = foreach ($row in Import-Csv -LiteralPath $Path) {
    $pressFrame = 0
    $responseFrame = 0
    $cameraFps = 0.0

    if (-not [int]::TryParse($row.press_frame, [ref]$pressFrame)) {
        throw "Invalid press_frame for mode '$($row.mode)', sample '$($row.sample)'"
    }
    if (-not [int]::TryParse($row.response_frame, [ref]$responseFrame)) {
        throw "Invalid response_frame for mode '$($row.mode)', sample '$($row.sample)'"
    }
    if (-not [double]::TryParse($row.camera_fps, [ref]$cameraFps) -or $cameraFps -le 0) {
        throw "Invalid camera_fps for mode '$($row.mode)', sample '$($row.sample)'"
    }
    if ($responseFrame -lt $pressFrame) {
        throw "response_frame precedes press_frame for mode '$($row.mode)', sample '$($row.sample)'"
    }

    [pscustomobject]@{
        mode = $row.mode
        sample = $row.sample
        press_frame = $pressFrame
        response_frame = $responseFrame
        camera_fps = $cameraFps
        latency_ms = (($responseFrame - $pressFrame) * 1000.0) / $cameraFps
    }
}

if ($measurements.Count -eq 0) {
    throw "Latency sample file contains no measurements: $Path"
}

$summary = foreach ($group in $measurements | Group-Object mode) {
    $values = [double[]]@($group.Group.latency_ms)

    [pscustomobject]@{
        mode = $group.Name
        samples = $values.Count
        minimum_ms = [math]::Round(($values | Measure-Object -Minimum).Minimum, 2)
        median_ms = [math]::Round((Get-Percentile $values 0.50), 2)
        p95_ms = [math]::Round((Get-Percentile $values 0.95), 2)
        p99_ms = [math]::Round((Get-Percentile $values 0.99), 2)
        maximum_ms = [math]::Round(($values | Measure-Object -Maximum).Maximum, 2)
    }
}

$summary | Sort-Object mode | Format-Table -AutoSize

$local = $summary | Where-Object mode -eq "local" | Select-Object -First 1
$remote = $summary | Where-Object mode -eq "remote" | Select-Object -First 1

if ($null -ne $local -and $null -ne $remote) {
    [pscustomobject]@{
        added_remote_median_ms = [math]::Round($remote.median_ms - $local.median_ms, 2)
        added_remote_p95_ms = [math]::Round($remote.p95_ms - $local.p95_ms, 2)
    } | Format-List
}

if ($OutputPath) {
    $measurements | Export-Csv -LiteralPath $OutputPath -NoTypeInformation
    Write-Host "Detailed measurements written to $OutputPath"
}
