param(
    [int]$PollSeconds = 30,
    [string]$PodName = "manga109-rfdetr-1152-stage2",
    [string]$StateDirectory = "",
    [int]$MaxPolls = 0,
    [string[]]$GpuPriority = @(
        "NVIDIA B200",
        "NVIDIA H200",
        "NVIDIA H100 80GB HBM3"
    ),
    [string]$ImageName = "runpod/pytorch:1.0.7-rc.138-cu1281-torch280-ubuntu2404"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if ($PollSeconds -lt 15) {
    throw "PollSeconds must be at least 15"
}

$repositoryRoot = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($StateDirectory)) {
    $StateDirectory = Join-Path $repositoryRoot "runs\runpod-8gpu-poller"
}
$stateDirectoryPath = [System.IO.Path]::GetFullPath($StateDirectory)
$runsRoot = [System.IO.Path]::GetFullPath((Join-Path $repositoryRoot "runs"))
if (-not $stateDirectoryPath.StartsWith($runsRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "StateDirectory must be inside the repository runs directory"
}
[System.IO.Directory]::CreateDirectory($stateDirectoryPath) | Out-Null

$apiKey = $env:RUNPOD_API_KEY
if ([string]::IsNullOrWhiteSpace($apiKey)) {
    throw "RUNPOD_API_KEY is not set"
}

$statePath = Join-Path $stateDirectoryPath "state.json"
$logPath = Join-Path $stateDirectoryPath "poller.log"
$priority = $GpuPriority
$imageName = $ImageName

function Write-Log([string]$Message) {
    $timestamp = [DateTimeOffset]::Now.ToString("o")
    Add-Content -LiteralPath $logPath -Value "$timestamp $Message" -Encoding utf8
}

function Write-State([hashtable]$State) {
    $State["updatedAt"] = [DateTimeOffset]::Now.ToString("o")
    $temporary = Join-Path $stateDirectoryPath ".state.$PID.tmp"
    $State | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $temporary -Encoding utf8
    Move-Item -LiteralPath $temporary -Destination $statePath -Force
}

function Invoke-Graphql([string]$Query) {
    $body = @{ query = $Query } | ConvertTo-Json -Compress
    $uri = "https://api.runpod.io/graphql?api_key=" + [uri]::EscapeDataString($apiKey)
    $response = Invoke-RestMethod -Method Post -Uri $uri -ContentType "application/json" -Body $body
    if ($response.PSObject.Properties.Name -contains "errors" -and $null -ne $response.errors) {
        $messages = @($response.errors | ForEach-Object { $_.message }) -join "; "
        throw "Runpod GraphQL error: $messages"
    }
    return $response.data
}

function Find-ExistingPod {
    $query = @"
query ExistingPod {
  myself {
    pods {
      id
      name
      desiredStatus
      gpuCount
      machine { gpuDisplayName dataCenterId location secureCloud }
    }
  }
}
"@
    $data = Invoke-Graphql $query
    return @($data.myself.pods | Where-Object { $_.name -eq $PodName } | Select-Object -First 1)
}

function Get-Availability([string]$GpuTypeId) {
    $escaped = $GpuTypeId.Replace('"', '\"')
    $query = @"
query Availability {
  gpuTypes(input: { id: "$escaped" }) {
    id
    displayName
    memoryInGb
    lowestPrice(input: { gpuCount: 8 }) {
      stockStatus
      uninterruptablePrice
      availableGpuCounts
    }
  }
}
"@
    $data = Invoke-Graphql $query
    return @($data.gpuTypes | Select-Object -First 1)
}

function New-TrainingPod([string]$GpuTypeId) {
    $escapedGpu = $GpuTypeId.Replace('"', '\"')
    $escapedName = $PodName.Replace('"', '\"')
    $query = @"
mutation CreateTrainingPod {
  podFindAndDeployOnDemand(
    input: {
      cloudType: ALL
      gpuCount: 8
      volumeInGb: 200
      containerDiskInGb: 100
      minVcpuCount: 64
      minMemoryInGb: 256
      gpuTypeId: "$escapedGpu"
      name: "$escapedName"
      imageName: "$imageName"
      dockerArgs: ""
      ports: "22/tcp,8888/http"
      volumeMountPath: "/workspace"
      allowedCudaVersions: ["12.8", "12.9", "13.0"]
    }
  ) {
    id
    imageName
    machineId
    machine { podHostId }
  }
}
"@
    $data = Invoke-Graphql $query
    return $data.podFindAndDeployOnDemand
}

Write-Log "poller started; priority=$($priority -join ' > '); interval=${PollSeconds}s"
Write-State @{
    status = "polling"
    pid = $PID
    podName = $PodName
    priority = $priority
    pollSeconds = $PollSeconds
    imageName = $imageName
}

$pollCount = 0
while ($true) {
    try {
        $pollCount += 1
        $existing = @(Find-ExistingPod)
        if ($existing.Count -gt 0) {
            $pod = $existing[0]
            Write-Log "existing matching pod found; id=$($pod.id); status=$($pod.desiredStatus)"
            Write-State @{
                status = "created"
                source = "existing"
                podName = $PodName
                pod = $pod
            }
            exit 0
        }

        $snapshot = @()
        $candidate = $null
        foreach ($gpuTypeId in $priority) {
            $result = @(Get-Availability $gpuTypeId)
            if ($result.Count -eq 0) {
                $snapshot += [ordered]@{ id = $gpuTypeId; stockStatus = $null; price = $null }
                continue
            }
            $gpu = $result[0]
            $price = $gpu.lowestPrice
            $snapshot += [ordered]@{
                id = $gpu.id
                displayName = $gpu.displayName
                memoryInGb = $gpu.memoryInGb
                stockStatus = $price.stockStatus
                price = $price.uninterruptablePrice
            }
            if ($null -eq $candidate -and $null -ne $price.stockStatus -and $null -ne $price.uninterruptablePrice) {
                $candidate = $gpu
            }
        }

        Write-State @{
            status = "polling"
            pid = $PID
            podName = $PodName
            pollCount = $pollCount
            availability = $snapshot
        }

        if ($null -ne $candidate) {
            $gpuTypeId = [string]$candidate.id
            $price = $candidate.lowestPrice.uninterruptablePrice
            Write-Log "offer found; gpu=$gpuTypeId; totalPricePerHour=$price; attempting creation"
            try {
                $created = New-TrainingPod $gpuTypeId
                if ($null -ne $created -and -not [string]::IsNullOrWhiteSpace([string]$created.id)) {
                    Write-Log "pod created; id=$($created.id); gpu=$gpuTypeId; totalPricePerHour=$price"
                    Write-State @{
                        status = "created"
                        source = "new"
                        podName = $PodName
                        gpuTypeId = $gpuTypeId
                        totalPricePerHour = $price
                        pod = $created
                    }
                    exit 0
                }
                Write-Log "creation returned no pod; continuing to poll"
            }
            catch {
                $message = $_.Exception.Message -replace '[\r\n]+', ' '
                Write-Log "creation attempt failed due to a race or API rejection; message=$message; continuing to poll"
            }
        }
    }
    catch {
        $typeName = $_.Exception.GetType().FullName
        $diagnostic = ""
        if ($_.Exception -is [System.Management.Automation.PropertyNotFoundException]) {
            $diagnostic = "; message=$($_.Exception.Message); stack=$($_.ScriptStackTrace -replace '[\r\n]+', ' ')"
        }
        Write-Log "poll iteration failed; exceptionType=$typeName$diagnostic; retrying"
        Write-State @{
            status = "polling_after_error"
            pid = $PID
            podName = $PodName
            exceptionType = $typeName
        }
    }
    if ($MaxPolls -gt 0 -and $pollCount -ge $MaxPolls) {
        Write-Log "maximum poll count reached without creating a pod"
        exit 0
    }
    Start-Sleep -Seconds $PollSeconds
}
