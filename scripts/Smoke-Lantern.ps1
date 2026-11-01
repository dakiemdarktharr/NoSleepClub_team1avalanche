<#
.SYNOPSIS
    Offline smoke checks for an already-running local LANTERN server.
.DESCRIPTION
    Compatible with PowerShell 5.1 and 7. Sends only read requests and deliberately invalid
    requests. No model, speech, source, chain RPC or payment request is intentionally made.
    Run with no other client issuing paid work if you want the budget before/after check
    to isolate this script. It does not start, stop, or modify the server or delete data.
.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/Smoke-Lantern.ps1
#>
[CmdletBinding()]
param(
    [string]$BaseUrl = 'http://127.0.0.1:3210'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Assert-Condition {
    param([bool]$Condition, [string]$Description)
    if (-not $Condition) { throw "FAIL: $Description" }
    Write-Host "PASS: $Description"
}

function Send-Check {
    param(
        [string]$Path,
        [string]$Method = 'GET',
        [hashtable]$Headers = @{},
        [object]$Body = $null,
        [string]$ContentType = ''
    )
    $arguments = @{
        Uri = "$script:Base$Path"
        Method = $Method
        Headers = $Headers
        UseBasicParsing = $true
        TimeoutSec = 15
        ErrorAction = 'Stop'
    }
    if ($null -ne $Body) { $arguments.Body = $Body }
    if ($ContentType) { $arguments.ContentType = $ContentType }
    try {
        $response = Invoke-WebRequest @arguments
        return [pscustomobject]@{
            Status = [int]$response.StatusCode
            Headers = $response.Headers
            Content = [string]$response.Content
        }
    } catch {
        $response = $_.Exception.Response
        if ($null -eq $response) { throw "Request failed for $Method $Path (server unavailable or connection error)." }
        return [pscustomobject]@{ Status = [int]$response.StatusCode; Headers = $response.Headers; Content = '' }
    }
}

function Get-Header {
    param([object]$Response, [string]$Name)
    $value = $Response.Headers[$Name]
    if ($null -eq $value) { return '' }
    return ($value -join ',')
}

try {
    $uri = [Uri]$BaseUrl
    if ($uri.Scheme -ne 'http' -or -not $uri.IsLoopback -or $uri.UserInfo -or
        $uri.AbsolutePath -ne '/' -or $uri.Query -or $uri.Fragment) {
        throw 'BaseUrl must be an HTTP loopback origin without credentials, path, query, or fragment.'
    }
    $script:Base = $uri.GetLeftPart([UriPartial]::Authority).TrimEnd('/')

    $health = Send-Check '/api/health'
    Assert-Condition ($health.Status -eq 200) 'GET /api/health returns 200'
    $healthData = $health.Content | ConvertFrom-Json
    Assert-Condition ($healthData.ok -eq $true -and $healthData.app -eq 'lantern') 'Health identifies LANTERN and reports OK'
    $healthAlias = Send-Check '/health'
    Assert-Condition ($healthAlias.Status -eq 200) 'GET /health returns 200'

    $page = Send-Check '/'
    Assert-Condition ($page.Status -eq 200 -and $page.Content.Contains('id="workspace"')) 'GET / renders the application shell'
    Assert-Condition ((Get-Header $page 'X-Content-Type-Options') -match 'nosniff') 'X-Content-Type-Options is nosniff'
    Assert-Condition ((Get-Header $page 'Referrer-Policy') -match 'no-referrer') 'Referrer-Policy is no-referrer'
    Assert-Condition ((Get-Header $page 'Content-Security-Policy') -match "default-src 'self'" -and
        (Get-Header $page 'Content-Security-Policy') -match "frame-ancestors 'none'") 'Content Security Policy restricts sources and embedding'
    Assert-Condition ((Get-Header $page 'Permissions-Policy') -match 'camera=\(\)' -and
        (Get-Header $page 'Permissions-Policy') -match 'microphone=\(self\)') 'Permissions-Policy limits camera and microphone'
    Assert-Condition ((Get-Header $page 'Cache-Control') -match 'no-store') 'Application shell is not cached'

    foreach ($view in @('overview','companies','dossier','compare','watchlist','jobs','sources','settings')) {
        $fragment = Send-Check "/ui?view=$view"
        Assert-Condition ($fragment.Status -eq 200 -and $fragment.Content.Contains('id="main-content"')) "GET /ui for $view renders a workspace"
    }

    $stateBefore = Send-Check '/api/state'
    Assert-Condition ($stateBefore.Status -eq 200) 'GET /api/state returns 200'
    $before = $stateBefore.Content | ConvertFrom-Json
    Assert-Condition ($null -ne $before.config -and $null -ne $before.sources) 'State contains configuration metadata and source catalogue'
    Assert-Condition ((Get-Header $stateBefore 'Cache-Control') -match 'no-store') 'Private state is not cached'

    foreach ($asset in @(
        @{ Path='/manifest.webmanifest'; Type='application/manifest\+json' },
        @{ Path='/assets/app.css'; Type='text/css' },
        @{ Path='/assets/app.js'; Type='(text|application)/javascript' },
        @{ Path='/assets/icon.svg'; Type='image/svg\+xml' },
        @{ Path='/sw.js'; Type='(text|application)/javascript' }
    )) {
        $result = Send-Check $asset.Path
        Assert-Condition ($result.Status -eq 200 -and (Get-Header $result 'Content-Type') -match $asset.Type) "Static asset $($asset.Path) is served with the expected type"
    }

    # Even if the origin guard regresses, the nonexistent company cannot invoke an AI or paid endpoint.
    $dummyId = [guid]::NewGuid().ToString()
    $crossBody = '{"company_id":"' + $dummyId + '","watched":true}'
    $cross = Send-Check '/api/watchlist' 'POST' @{ Origin='https://invalid.example'; 'Sec-Fetch-Site'='cross-site' } $crossBody 'application/json'
    Assert-Condition ($cross.Status -eq 403) 'Cross-origin write is rejected'

    # Nonempty bytes are deliberately not a WAV: validation must reject before connector or budget.
    $invalidWav = [Text.Encoding]::ASCII.GetBytes('INVALID_WAV_OFFLINE_SMOKE')
    $audio = Send-Check '/api/transcribe' 'POST' @{} $invalidWav 'audio/wav'
    Assert-Condition ($audio.Status -eq 400) 'Invalid WAV is rejected before an audio request'

    # A fresh random UUID cannot name an existing report. These paths return before any RPC call.
    $commitment = Send-Check "/api/reports/$dummyId/commitment" 'POST'
    Assert-Condition ($commitment.Status -eq 404) 'Commitment for a missing report returns 404'
    $transaction = Send-Check "/api/reports/$dummyId/transaction" 'POST'
    Assert-Condition ($transaction.Status -eq 404 -or $transaction.Status -eq 428) 'Missing report or registry prevents chain transaction preparation'

    $stateAfter = Send-Check '/api/state'
    Assert-Condition ($stateAfter.Status -eq 200) 'State remains available after rejected requests'
    $after = $stateAfter.Content | ConvertFrom-Json
    Assert-Condition ($before.config.reserved_usd -eq $after.config.reserved_usd -and
        $before.config.spent_usd -eq $after.config.spent_usd) 'Rejected requests did not change the budget ledger (requires no concurrent paid work)'
    Write-Host 'Offline smoke checks passed.'
    exit 0
} catch {
    # No response bodies, credentials, prompts, or provider data are printed.
    Write-Error $_.Exception.Message -ErrorAction Continue
    exit 1
}
