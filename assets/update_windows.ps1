param(
    [Parameter(Mandatory = $true)][int]$LauncherPid,
    [Parameter(Mandatory = $true)][string]$TargetPath,
    [Parameter(Mandatory = $true)][string]$ReplacementPath,
    [Parameter(Mandatory = $true)][string]$HandoffDataPath
)

$ErrorActionPreference = 'Stop'

function Show-UpdateFailure {
    param([string]$Message)

    try {
        Add-Type -AssemblyName System.Windows.Forms
        [System.Windows.Forms.MessageBox]::Show(
            $Message,
            $handoffData.failureTitle,
            [System.Windows.Forms.MessageBoxButtons]::OK,
            [System.Windows.Forms.MessageBoxIcon]::Error
        ) | Out-Null
    } catch {
        # Keep the existing launcher available even if Windows cannot show the dialog.
    }
}

function ConvertTo-WindowsArgument {
    param([string]$Value)

    if ($Value.Length -gt 0 -and $Value -notmatch '[\s"]') {
        return $Value
    }

    $builder = New-Object System.Text.StringBuilder
    [void]$builder.Append('"')
    $slashes = 0
    foreach ($character in $Value.ToCharArray()) {
        if ($character -eq '\') {
            $slashes++
            continue
        }
        if ($character -eq '"') {
            [void]$builder.Append(('\' * (2 * $slashes + 1)))
            [void]$builder.Append('"')
            $slashes = 0
            continue
        }
        if ($slashes -gt 0) {
            [void]$builder.Append(('\' * $slashes))
            $slashes = 0
        }
        [void]$builder.Append($character)
    }
    if ($slashes -gt 0) {
        [void]$builder.Append(('\' * (2 * $slashes)))
    }
    [void]$builder.Append('"')
    return $builder.ToString()
}

$handoffData = Get-Content -LiteralPath $HandoffDataPath -Raw -Encoding UTF8 | ConvertFrom-Json

while ($true) {
    try {
        Get-Process -Id $LauncherPid -ErrorAction Stop | Out-Null
        Start-Sleep -Milliseconds 250
    } catch {
        break
    }
}

$installed = $false
$lastError = ''
for ($attempt = 0; $attempt -lt 60; $attempt++) {
    try {
        Move-Item -LiteralPath $ReplacementPath -Destination $TargetPath -Force -ErrorAction Stop
        $installed = $true
        break
    } catch {
        $lastError = $_.Exception.Message
        Start-Sleep -Seconds 1
    }
}

if (-not $installed) {
    Show-UpdateFailure ($handoffData.installFailure.Replace('%0', $lastError))
    Remove-Item -LiteralPath $ReplacementPath -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $HandoffDataPath -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue
    exit 1
}

try {
    $argumentList = @($handoffData.arguments | ForEach-Object {
        ConvertTo-WindowsArgument ([string]$_)
    })
    $processInfo = New-Object System.Diagnostics.ProcessStartInfo
    $processInfo.FileName = $TargetPath
    $processInfo.WorkingDirectory = Split-Path -LiteralPath $TargetPath -Parent
    $processInfo.Arguments = $argumentList -join ' '
    $processInfo.UseShellExecute = $false
    [System.Diagnostics.Process]::Start($processInfo) | Out-Null
} catch {
    Show-UpdateFailure ($handoffData.restartFailure.Replace('%0', $_.Exception.Message))
    Remove-Item -LiteralPath $HandoffDataPath -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue
    exit 1
}

Remove-Item -LiteralPath $HandoffDataPath -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue
