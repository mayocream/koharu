param(
    [int]$PollSeconds = 30,
    [int]$MaxPolls = 0,
    [string]$PodName = "manga109-rfdetr-v2-h100x8",
    [string]$StateDirectory = "",
    [string]$ExistingPodId = ""
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if ($PollSeconds -lt 15) {
    throw "PollSeconds must be at least 15"
}

$repositoryRoot = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($StateDirectory)) {
    $StateDirectory = Join-Path $repositoryRoot "runs\runpod-h100x8-textseg-v2-orchestrator"
}
$stateDirectoryPath = [IO.Path]::GetFullPath($StateDirectory)
$runsRoot = [IO.Path]::GetFullPath((Join-Path $repositoryRoot "runs"))
if (-not $stateDirectoryPath.StartsWith($runsRoot, [StringComparison]::OrdinalIgnoreCase)) {
    throw "StateDirectory must be inside the repository runs directory"
}
[IO.Directory]::CreateDirectory($stateDirectoryPath) | Out-Null

$apiKey = [Environment]::GetEnvironmentVariable("RUNPOD_API_KEY", "Process")
if (-not $apiKey) {
    $apiKey = [Environment]::GetEnvironmentVariable("RUNPOD_API_KEY", "User")
}
if (-not $apiKey) {
    throw "RUNPOD_API_KEY is not set"
}

$sshKey = Join-Path $env:USERPROFILE ".ssh\id_ed25519"
if (-not (Test-Path -LiteralPath $sshKey -PathType Leaf)) {
    throw "SSH private key is missing: $sshKey"
}

$archiveRoot = Join-Path $repositoryRoot "runs\runpod-b200-stage2\upload_archives"
$overlay = Join-Path $repositoryRoot "runs\runpod-h100x8-textseg-v2-r3\overlay.tar.zst"
$uploadFiles = @(
    @(Get-ChildItem -LiteralPath $archiveRoot -Filter "train.part.*" -File | Sort-Object Name | ForEach-Object FullName)
    (Join-Path $archiveRoot "valid.tar.zst")
    (Join-Path $archiveRoot "test.tar.zst")
    $overlay
)
foreach ($path in $uploadFiles) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Required upload artifact is missing: $path"
    }
}

$statePath = Join-Path $stateDirectoryPath "state.json"
$logPath = Join-Path $stateDirectoryPath "orchestrator.log"
$localOutput = Join-Path $repositoryRoot "runs\rfdetr-seg-2xl-1152-textseg-v2-h100x8"
$remoteOutput = "/workspace/koharu/runs/rfdetr-seg-2xl-1152-textseg-v2-h100x8"
$headers = @{ Authorization = "Bearer $apiKey"; "Content-Type" = "application/json" }
$podId = $null
$podStopped = $false

function Write-Log([string]$Message) {
    $line = "{0} {1}" -f ([DateTimeOffset]::Now.ToString("o")), $Message
    Add-Content -LiteralPath $logPath -Value $line -Encoding utf8
}

function Write-State([hashtable]$State) {
    $State["updatedAt"] = [DateTimeOffset]::Now.ToString("o")
    $temporary = Join-Path $stateDirectoryPath ".state.$PID.tmp"
    $State | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $temporary -Encoding utf8
    Move-Item -LiteralPath $temporary -Destination $statePath -Force
}

function Invoke-Rest([string]$Method, [string]$Path, [object]$Body = $null) {
    $arguments = @{
        Method = $Method
        Uri = "https://rest.runpod.io/v1$Path"
        Headers = $headers
        TimeoutSec = 60
    }
    if ($null -ne $Body) {
        $arguments["Body"] = $Body | ConvertTo-Json -Depth 10
    }
    return Invoke-RestMethod @arguments
}

function New-ReliablePod {
    foreach ($cloudType in @("SECURE", "COMMUNITY")) {
        $payload = @{
            name = $PodName
            imageName = "runpod/pytorch:1.0.7-rc.138-cu1281-torch280-ubuntu2404"
            cloudType = $cloudType
            computeType = "GPU"
            # All three variants are genuine H100s with at least 80 GB per GPU.
            gpuTypeIds = @(
                "NVIDIA H100 80GB HBM3",
                "NVIDIA H100 NVL",
                "NVIDIA H100 PCIe"
            )
            gpuTypePriority = "availability"
            gpuCount = 8
            # AP-IN-2 is deliberately excluded after repeated dead SSH/Jupyter endpoints.
            dataCenterIds = @(
                "AP-JP-1", "CA-MTL-1", "EUR-IS-3", "US-GA-2", "AP-IN-1",
                "EU-FR-1", "EU-NL-1", "US-CA-2", "US-KS-2", "US-TX-3"
            )
            dataCenterPriority = "availability"
            containerDiskInGb = 100
            volumeInGb = 200
            volumeMountPath = "/workspace"
            ports = @("22/tcp", "8888/http")
            supportPublicIp = $true
            interruptible = $false
            allowedCudaVersions = @("12.8")
            minVCPUPerGPU = 8
            minRAMPerGPU = 32
        }
        try {
            $pod = Invoke-Rest "Post" "/pods" $payload
            if ($null -ne $pod) {
                Write-Log "capacity acquired; cloudType=$cloudType; id=$($pod.id)"
                return $pod
            }
        }
        catch {
            $message = $_.Exception.Message
            if ($_.ErrorDetails -and $_.ErrorDetails.Message) {
                $message = $_.ErrorDetails.Message
            }
            if ($message -match "no instances currently available") {
                continue
            }
            throw
        }
    }
    return $null
}

function Get-SshEndpoint([string]$Id) {
    $query = @"
query PodRuntime {
  pod(input: { podId: "$Id" }) {
    desiredStatus
    costPerHr
    machine { dataCenterId location gpuDisplayName }
    runtime { uptimeInSeconds ports { ip isIpPublic privatePort publicPort type } }
  }
}
"@
    $uri = "https://api.runpod.io/graphql?api_key=" + [uri]::EscapeDataString($apiKey)
    $response = Invoke-RestMethod -Method Post -Uri $uri -ContentType "application/json" -Body (@{ query = $query } | ConvertTo-Json -Compress) -TimeoutSec 60
    if (
        $response.PSObject.Properties.Name -contains "errors" -and
        $null -ne $response.errors
    ) {
        throw "RunPod GraphQL error: $($response.errors.message -join '; ')"
    }
    $pod = $response.data.pod
    if ($null -eq $pod -or $null -eq $pod.runtime) {
        return $null
    }
    $port = @($pod.runtime.ports | Where-Object {
        $_.privatePort -eq 22 -and $_.type -eq "tcp" -and $_.isIpPublic
    } | Select-Object -First 1)
    if ($port.Count -eq 0) {
        return $null
    }
    return [pscustomobject]@{
        hostName = [string]$port[0].ip
        port = [int]$port[0].publicPort
        costPerHr = [double]$pod.costPerHr
        dataCenterId = [string]$pod.machine.dataCenterId
        location = [string]$pod.machine.location
        gpu = [string]$pod.machine.gpuDisplayName
        uptimeInSeconds = [int]$pod.runtime.uptimeInSeconds
    }
}

function Get-SshOptions([int]$Port) {
    return @(
        "-o", "BatchMode=yes",
        "-o", "ConnectTimeout=15",
        "-o", "ConnectionAttempts=1",
        "-o", "ServerAliveInterval=15",
        "-o", "ServerAliveCountMax=4",
        "-o", "StrictHostKeyChecking=accept-new",
        "-i", $sshKey,
        "-p", $Port
    )
}

function Invoke-Ssh([string]$Remote, [int]$Port, [string]$Command) {
    $options = Get-SshOptions $Port
    # Windows OpenSSH reconstructs the remote command and can strip nested
    # shell quotes. Encode scripts so regex alternation and other shell syntax
    # arrive at bash byte-for-byte instead of becoming an unintended pipeline.
    $encodedCommand = [Convert]::ToBase64String(
        [Text.Encoding]::UTF8.GetBytes($Command)
    )
    $remoteCommand = "printf %s $encodedCommand | base64 -d | bash"
    $result = & ssh @options $Remote $remoteCommand 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "SSH failed ($LASTEXITCODE): $($result -join ' ')"
    }
    return $result
}

function Stop-Pod([string]$Id) {
    if ($podStopped) {
        return
    }
    try {
        Invoke-Rest "Post" "/pods/$Id/stop" | Out-Null
        for ($attempt = 0; $attempt -lt 30; $attempt += 1) {
            Start-Sleep -Seconds 10
            $pod = Invoke-Rest "Get" "/pods/$Id"
            if ($pod.desiredStatus -eq "EXITED") {
                $script:podStopped = $true
                Write-Log "pod stopped; id=$Id"
                return
            }
        }
        throw "pod did not reach EXITED within five minutes"
    }
    catch {
        Write-Log "pod stop failed; id=$Id; error=$($_.Exception.Message -replace '[\r\n]+',' ')"
        throw
    }
}

function Verify-DownloadedOutput([string]$OutputPath) {
    $manifest = Join-Path $OutputPath "SHA256SUMS"
    if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) {
        throw "Downloaded SHA256SUMS is missing"
    }
    $root = [IO.Path]::GetFullPath($OutputPath).TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
    $verified = 0
    foreach ($line in Get-Content -LiteralPath $manifest) {
        if ($line -notmatch '^([0-9a-f]{64})  (.+)$') {
            throw "Malformed SHA256SUMS line: $line"
        }
        $relative = $Matches[2] -replace '^\./', ''
        $localFile = [IO.Path]::GetFullPath((Join-Path $OutputPath $relative))
        if (-not $localFile.StartsWith($root, [StringComparison]::OrdinalIgnoreCase)) {
            throw "Manifest path escaped output: $relative"
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
        throw "No files were verified"
    }
    return $verified
}

try {
    Write-Log "orchestrator started; podName=$PodName; pollSeconds=$PollSeconds; existingPodId=$ExistingPodId"
    if ([string]::IsNullOrWhiteSpace($ExistingPodId)) {
        Write-State @{
            status = "polling_capacity"
            pid = $PID
            podName = $PodName
            excludedDataCenterIds = @("AP-IN-2")
        }

        $poll = 0
        $pod = $null
        while ($null -eq $pod) {
            $poll += 1
            $pod = New-ReliablePod
            if ($null -eq $pod) {
                Write-Log "capacity unavailable; poll=$poll"
                Write-State @{ status = "polling_capacity"; pid = $PID; podName = $PodName; poll = $poll }
                if ($MaxPolls -gt 0 -and $poll -ge $MaxPolls) {
                    throw "maximum capacity polls reached"
                }
                Start-Sleep -Seconds $PollSeconds
            }
        }

        $podId = [string]$pod.id
        Write-Log "pod created; id=$podId; apiCostPerHr=$($pod.costPerHr)"
    }
    else {
        $podId = $ExistingPodId
        $existing = Invoke-Rest "Get" "/pods/$podId"
        if ($existing.desiredStatus -ne "RUNNING") {
            throw "existing pod is not running: id=$podId; status=$($existing.desiredStatus)"
        }
        Write-Log "adopting existing pod; id=$podId; apiCostPerHr=$($existing.costPerHr)"
    }
    Write-State @{ status = "waiting_for_ssh"; pid = $PID; podId = $podId; podName = $PodName }

    $endpoint = $null
    for ($attempt = 1; $attempt -le 60; $attempt += 1) {
        $endpoint = Get-SshEndpoint $podId
        if ($null -ne $endpoint) {
            break
        }
        Start-Sleep -Seconds 10
    }
    if ($null -eq $endpoint) {
        throw "RunPod did not expose a public SSH endpoint within ten minutes"
    }
    $remote = "root@$($endpoint.hostName)"
    Write-Log "endpoint assigned; id=$podId; host=$($endpoint.hostName); port=$($endpoint.port); dc=$($endpoint.dataCenterId); costPerHr=$($endpoint.costPerHr)"

    $sshReady = $false
    for ($attempt = 1; $attempt -le 40; $attempt += 1) {
        try {
            $probe = Invoke-Ssh $remote $endpoint.port "test `$(nvidia-smi -L | wc -l) -eq 8 && echo ssh_ready"
            if (($probe -join "`n") -match "ssh_ready") {
                $sshReady = $true
                break
            }
        }
        catch {
            Write-Log "ssh not ready; attempt=$attempt"
        }
        Start-Sleep -Seconds 15
    }
    if (-not $sshReady) {
        throw "SSH did not become healthy within ten minutes"
    }

    Write-Log "ssh healthy; beginning upload"
    Write-State @{
        status = "uploading"
        pid = $PID
        podId = $podId
        hostName = $endpoint.hostName
        sshPort = $endpoint.port
        dataCenterId = $endpoint.dataCenterId
        costPerHr = $endpoint.costPerHr
    }
    Invoke-Ssh $remote $endpoint.port "mkdir -p /workspace/incoming /workspace/koharu" | Out-Null
    $scpOptions = @(
        "-o", "BatchMode=yes",
        "-o", "ConnectTimeout=30",
        "-o", "ServerAliveInterval=15",
        "-o", "ServerAliveCountMax=20",
        "-o", "StrictHostKeyChecking=accept-new",
        "-i", $sshKey,
        "-P", $endpoint.port,
        "-X", "nrequests=256",
        "-X", "buffer=262144"
    )
    $scpExecutable = (Get-Command scp.exe -ErrorAction Stop).Source
    $transferProcesses = @()
    foreach ($uploadFile in $uploadFiles) {
        $process = Start-Process `
            -FilePath $scpExecutable `
            -ArgumentList ($scpOptions + @($uploadFile, "${remote}:/workspace/incoming/")) `
            -WindowStyle Hidden `
            -PassThru
        $transferProcesses += [pscustomobject]@{
            file = $uploadFile
            process = $process
        }
    }
    $uploadFailures = @()
    foreach ($transfer in $transferProcesses) {
        $transfer.process.WaitForExit()
        if ($transfer.process.ExitCode -ne 0) {
            $uploadFailures += "$(Split-Path -Leaf $transfer.file):$($transfer.process.ExitCode)"
        }
    }
    if ($uploadFailures.Count -gt 0) {
        throw "parallel SCP upload failed: $($uploadFailures -join ', ')"
    }
    Write-Log "upload complete"

    Write-State @{ status = "extracting_and_installing"; pid = $PID; podId = $podId }
    $setup = @'
set -euo pipefail
mkdir -p /workspace/koharu/data/manga109-segmentation-rfdetr
cat /workspace/incoming/train.part.* > /workspace/incoming/train.tar.zst
tar --zstd -xf /workspace/incoming/train.tar.zst -C /workspace/koharu/data/manga109-segmentation-rfdetr
tar --zstd -xf /workspace/incoming/valid.tar.zst -C /workspace/koharu/data/manga109-segmentation-rfdetr
tar --zstd -xf /workspace/incoming/test.tar.zst -C /workspace/koharu/data/manga109-segmentation-rfdetr
tar --zstd -xf /workspace/incoming/overlay.tar.zst -C /workspace/koharu
cd /workspace/koharu
echo 'b8b101a2bbd1f139b4283a6093e3c8dd36cf218fc3c9daf2ca84a241f2dcbec1  data/manga109-segmentation-rfdetr/train/_annotations.coco.json' | sha256sum -c -
echo 'dd892565a348a7830ff2cb985a8f37333df31d50fcab6c2989d29dd9f35a0cdc  data/manga109-segmentation-rfdetr/valid/_annotations.coco.json' | sha256sum -c -
echo 'ae1cc28f784a0c302bafa75d64678c4378360a513fdab3b68b2b5279b4e46826  data/manga109-segmentation-rfdetr/test/_annotations.coco.json' | sha256sum -c -
echo '92aaf92a92b5d64b1d7ef55dbd42fd1920f993c8e34cd645e3fd998e1be518a6  runs/rfdetr-seg-2xl-1152-stage2-h100x8/checkpoint_best_total.pth' | sha256sum -c -
python - <<'PY'
import json
from pathlib import Path
expected = {'train': (8128, 358499), 'valid': (1001, 48213), 'test': (969, 47894)}
root = Path('/workspace/koharu/data/manga109-segmentation-rfdetr')
for split, counts in expected.items():
    with (root / split / '_annotations.coco.json').open(encoding='utf-8') as f:
        coco = json.load(f)
    assert (len(coco['images']), len(coco['annotations'])) == counts
    missing = [x['file_name'] for x in coco['images'] if not (root / split / x['file_name']).is_file()]
    assert not missing, (split, missing[:10])
print('dataset_validation=ok')
PY
python -m venv --system-site-packages /workspace/rfdetr-venv
/workspace/rfdetr-venv/bin/python -m pip install --upgrade pip
/workspace/rfdetr-venv/bin/python -m pip install -e '/workspace/koharu/data/rfdetr-1.7.0[train]' tensorboard
/workspace/rfdetr-venv/bin/python - <<'PY'
import torch, rfdetr
assert torch.cuda.is_available() and torch.cuda.device_count() == 8
print('torch', torch.__version__, 'cuda_devices', torch.cuda.device_count(), 'rfdetr', rfdetr.__file__)
PY
'@
    Invoke-Ssh $remote $endpoint.port $setup | ForEach-Object { Write-Log "setup: $_" }
    Write-Log "remote dataset and environment verified"

    Write-State @{ status = "launching_training"; pid = $PID; podId = $podId }
    $launch = @'
set -euo pipefail
OUT=/workspace/koharu/runs/rfdetr-seg-2xl-1152-textseg-v2-h100x8
mkdir -p "$OUT"
chmod +x /workspace/koharu/runs/runpod-h100x8-textseg-v2-r3/launch_training.sh
nohup bash /workspace/koharu/runs/runpod-h100x8-textseg-v2-r3/launch_training.sh > "$OUT/wrapper.log" 2>&1 < /dev/null &
echo $! > "$OUT/wrapper.pid"
echo "training_pid=$(cat "$OUT/wrapper.pid")"
'@
    Invoke-Ssh $remote $endpoint.port $launch | ForEach-Object { Write-Log "launch: $_" }
    Start-Sleep -Seconds 90
    $startup = Invoke-Ssh $remote $endpoint.port "OUT='$remoteOutput'; test -s `"`$OUT/wrapper.pid`"; kill -0 `$(cat `"`$OUT/wrapper.pid`"); test `$(nvidia-smi --query-compute-apps=pid --format=csv,noheader | sort -u | wc -l) -ge 8; echo training_started"
    if (($startup -join "`n") -notmatch "training_started") {
        throw "training did not start on all eight GPUs"
    }
    Write-Log "training started on eight GPUs"
    Write-State @{
        status = "training"
        pid = $PID
        podId = $podId
        remoteOutput = $remoteOutput
        localOutput = $localOutput
        hostName = $endpoint.hostName
        sshPort = $endpoint.port
        dataCenterId = $endpoint.dataCenterId
        costPerHr = $endpoint.costPerHr
    }

    while ($true) {
        $status = @(Invoke-Ssh $remote $endpoint.port (
            "OUT='$remoteOutput'; " +
            "if test -s `"`$OUT/wrapper.exit`"; then echo EXIT=`$(cat `"`$OUT/wrapper.exit`"); " +
            "elif test -s `"`$OUT/wrapper.pid`" && kill -0 `$(cat `"`$OUT/wrapper.pid`") 2>/dev/null; then echo RUNNING; " +
            "else echo LOST; fi; " +
            "if test -s `"`$OUT/checkpoint_latest.pth`"; then stat -c checkpoint_bytes=%s `"`$OUT/checkpoint_latest.pth`"; fi; " +
            "tail -n 3 `"`$OUT/train.log`" 2>/dev/null || true"
        ))
        $headline = [string]$status[0]
        Write-Log "training status: $($status -join ' | ')"
        if ($headline -eq "RUNNING") {
            Start-Sleep -Seconds 60
            continue
        }
        if ($headline -eq "EXIT=0") {
            break
        }
        throw "training stopped unsuccessfully: $headline"
    }

    $successCheck = @'
set -euo pipefail
OUT=/workspace/koharu/runs/rfdetr-seg-2xl-1152-textseg-v2-h100x8
test "$(cat "$OUT/wrapper.exit")" = 0
test -s "$OUT/checkpoint_best_total.pth"
test -s "$OUT/final_validation.json"
test -s "$OUT/TRAINING_COMPLETE.json"
grep -Eq '"status"[[:space:]]*:[[:space:]]*"complete"' "$OUT/TRAINING_COMPLETE.json"
! grep -Eq 'Traceback|CUDA out of memory|RuntimeError|Killed|DistBackendError|Watchdog caught|NCCL.*timed out' "$OUT/train.log"
cd "$OUT"
find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS
echo training_complete
'@
    Invoke-Ssh $remote $endpoint.port $successCheck | ForEach-Object { Write-Log "complete: $_" }

    Write-State @{ status = "downloading"; pid = $PID; podId = $podId; remoteOutput = $remoteOutput }
    $localParent = Split-Path -Parent $localOutput
    [IO.Directory]::CreateDirectory($localParent) | Out-Null
    $downloadOptions = @(
        "-o", "BatchMode=yes",
        "-o", "ConnectTimeout=30",
        "-o", "ServerAliveInterval=15",
        "-o", "ServerAliveCountMax=20",
        "-o", "StrictHostKeyChecking=accept-new",
        "-i", $sshKey,
        "-P", $endpoint.port,
        "-r", "-X", "nrequests=256", "-X", "buffer=262144"
    )
    & scp @downloadOptions "${remote}:$remoteOutput" $localParent
    if ($LASTEXITCODE -ne 0) {
        throw "SCP download failed with exit code $LASTEXITCODE"
    }
    $verified = Verify-DownloadedOutput $localOutput
    Write-Log "download verified; files=$verified"

    Stop-Pod $podId
    Write-State @{
        status = "complete"
        pid = $PID
        podId = $podId
        podStatus = "EXITED"
        localOutput = $localOutput
        verifiedFiles = $verified
    }
    Write-Log "orchestration complete; weights downloaded and pod stopped"
    exit 0
}
catch {
    $message = $_.Exception.Message -replace '[\r\n]+', ' '
    Write-Log "orchestration failed; error=$message"
    if ($podId) {
        try {
            Stop-Pod $podId
        }
        catch {
            Write-Log "emergency stop also failed; error=$($_.Exception.Message -replace '[\r\n]+',' ')"
        }
    }
    Write-State @{
        status = "failed"
        pid = $PID
        podId = $podId
        podStopped = $podStopped
        error = $message
    }
    exit 1
}
finally {
    Remove-Variable apiKey -ErrorAction SilentlyContinue
}
