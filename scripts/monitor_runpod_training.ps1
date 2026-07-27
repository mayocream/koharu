param(
    [string]$PodId = "uvtau6zu2ly8r1",
    [string]$RemoteHostName = "157.66.254.11",
    [int]$SshPort = 13272,
    [string]$RemoteOutput = "/workspace/koharu/runs/rfdetr-seg-2xl-768-stage1-v3",
    [string]$LocalOutput = "C:\Users\Mayo\Workspaces\koharu\runs\rfdetr-seg-2xl-768-stage1-v3",
    [int]$ExpectedCheckpointEpoch = 35,
    [int]$PollSeconds = 60
)

$ErrorActionPreference = "Stop"
$sshKey = Join-Path $env:USERPROFILE ".ssh\id_ed25519"
$localParent = Split-Path -Parent $LocalOutput
$runName = ($RemoteOutput -split '/')[-1]
$monitorLog = Join-Path $localParent "$runName-monitor.log"
$remote = "root@$RemoteHostName"
$sshOptions = @(
    "-o", "BatchMode=yes",
    "-o", "ConnectTimeout=10",
    "-o", "ConnectionAttempts=1",
    "-o", "ServerAliveInterval=5",
    "-o", "ServerAliveCountMax=2",
    "-o", "StrictHostKeyChecking=yes",
    "-i", $sshKey,
    "-p", $SshPort
)
$scpOptions = @(
    "-o", "BatchMode=yes",
    "-o", "ConnectTimeout=10",
    "-o", "ConnectionAttempts=1",
    "-o", "ServerAliveInterval=5",
    "-o", "ServerAliveCountMax=2",
    "-o", "StrictHostKeyChecking=yes",
    "-i", $sshKey,
    "-P", $SshPort
)

New-Item -ItemType Directory -Force -Path $localParent | Out-Null

function Write-MonitorLog([string]$Message) {
    $line = "{0} {1}" -f (Get-Date).ToUniversalTime().ToString("o"), $Message
    Add-Content -LiteralPath $monitorLog -Value $line
}

function Invoke-Ssh([string]$Command) {
    $result = & ssh @sshOptions $remote $Command 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "SSH command failed with exit code ${LASTEXITCODE}: $($result -join ' ')"
    }
    return $result
}

try {
    Write-MonitorLog "START pod=$PodId remote=$RemoteOutput local=$LocalOutput"
    $consecutiveSshFailures = 0

    while ($true) {
        try {
            $status = @(Invoke-Ssh (
                "if test -f '$RemoteOutput/wrapper.exit'; then echo STOPPED; " +
                "elif test -s '$RemoteOutput/wrapper.pid' && kill -0 `$(cat '$RemoteOutput/wrapper.pid') 2>/dev/null; " +
                "then echo RUNNING; else echo STOPPED; fi; " +
                "if test -s '$RemoteOutput/checkpoint_latest.pth'; then " +
                "echo checkpoint=available; " +
                "else echo checkpoint_epoch=none; fi"
            ))
            $consecutiveSshFailures = 0
        }
        catch {
            $consecutiveSshFailures += 1
            Write-MonitorLog "SSH_RETRY attempt=$consecutiveSshFailures error=$($_.Exception.Message)"
            if ($consecutiveSshFailures -ge 10) {
                throw "Ten consecutive SSH checks failed"
            }
            Start-Sleep -Seconds $PollSeconds
            continue
        }

        $state = $status[0].Trim()
        $progress = if ($status.Count -gt 1) { $status[-1].Trim() } else { "unknown" }
        Write-MonitorLog "STATE state=$state progress=$progress"
        if ($state -eq "STOPPED") {
            break
        }
        if ($state -ne "RUNNING") {
            throw "Unexpected remote process state: $state"
        }
        Start-Sleep -Seconds $PollSeconds
    }

    Invoke-Ssh (
        "test `"`$(cat '$RemoteOutput/wrapper.exit')`" = '0' && " +
        "test -s '$RemoteOutput/checkpoint_best_total.pth' && " +
        "test -s '$RemoteOutput/final_validation.json' && " +
        "test -s '$RemoteOutput/TRAINING_COMPLETE.json' && " +
        "grep -Eq '`"status`"[[:space:]]*:[[:space:]]*`"complete`"' '$RemoteOutput/TRAINING_COMPLETE.json' && " +
        "grep -Eq '`"checkpoint_epoch`"[[:space:]]*:[[:space:]]*$ExpectedCheckpointEpoch([,[:space:]]|`$)' '$RemoteOutput/TRAINING_COMPLETE.json' && " +
        "! grep -Eq 'Traceback|CUDA out of memory|RuntimeError|Killed|DistBackendError|Watchdog caught|NCCL.*timed out' '$RemoteOutput/train.log'"
    ) | Out-Null
    Write-MonitorLog "REMOTE_SUCCESS_CHECK passed"

    Invoke-Ssh (
        "cd '$RemoteOutput' && " +
        "find . -type f ! -name SHA256SUMS -print0 | sort -z | " +
        "xargs -0 sha256sum > SHA256SUMS"
    ) | Out-Null
    Write-MonitorLog "REMOTE_MANIFEST created"

    New-Item -ItemType Directory -Force -Path $localParent | Out-Null
    & scp @scpOptions -r -X nrequests=256 -X buffer=262144 "${remote}:$RemoteOutput" $localParent
    if ($LASTEXITCODE -ne 0) {
        throw "SCP download failed with exit code $LASTEXITCODE"
    }
    Write-MonitorLog "DOWNLOAD complete"

    $manifestPath = Join-Path $LocalOutput "SHA256SUMS"
    if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
        throw "Downloaded SHA256SUMS is missing"
    }
    $root = [IO.Path]::GetFullPath($LocalOutput).TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
    $verified = 0
    foreach ($line in Get-Content -LiteralPath $manifestPath) {
        if ($line -notmatch '^([0-9a-f]{64})  (.+)$') {
            throw "Malformed SHA256SUMS line: $line"
        }
        $relative = $Matches[2] -replace '^\./', ''
        $localFile = [IO.Path]::GetFullPath((Join-Path $LocalOutput $relative))
        if (-not $localFile.StartsWith($root, [StringComparison]::OrdinalIgnoreCase)) {
            throw "Manifest path escaped the output directory: $relative"
        }
        if (-not (Test-Path -LiteralPath $localFile -PathType Leaf)) {
            throw "Downloaded file is missing: $relative"
        }
        $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $localFile).Hash.ToLowerInvariant()
        if ($actual -ne $Matches[1]) {
            throw "SHA-256 mismatch: $relative"
        }
        $verified += 1
    }
    if ($verified -eq 0) {
        throw "No files were present in SHA256SUMS"
    }
    Write-MonitorLog "VERIFY passed files=$verified"

    $apiKey = [Environment]::GetEnvironmentVariable("RUNPOD_API_KEY", "Process")
    if (-not $apiKey) {
        $apiKey = [Environment]::GetEnvironmentVariable("RUNPOD_API_KEY", "User")
    }
    if (-not $apiKey) {
        throw "RUNPOD_API_KEY is not set in the user environment"
    }
    try {
        $headers = @{ Authorization = "Bearer $apiKey" }
        Invoke-RestMethod -Uri "https://rest.runpod.io/v1/pods/$PodId/stop" -Headers $headers -Method Post -TimeoutSec 30 | Out-Null
        $stopped = $false
        for ($attempt = 0; $attempt -lt 30; $attempt += 1) {
            Start-Sleep -Seconds 10
            $pod = Invoke-RestMethod -Uri "https://rest.runpod.io/v1/pods/$PodId" -Headers $headers -Method Get -TimeoutSec 30
            if ($pod.desiredStatus -eq "EXITED") {
                $stopped = $true
                break
            }
        }
        if (-not $stopped) {
            throw "RunPod did not reach EXITED within five minutes"
        }
    }
    finally {
        Remove-Variable apiKey -ErrorAction SilentlyContinue
    }

    Write-MonitorLog "COMPLETE files=$verified pod_status=EXITED"
    exit 0
}
catch {
    Write-MonitorLog "FAILED $($_.Exception.Message)"
    exit 1
}
