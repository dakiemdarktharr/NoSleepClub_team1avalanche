[CmdletBinding()]
param([switch]$Build)
$ErrorActionPreference = 'Stop'
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Push-Location $projectRoot
try {
    if (-not (Test-Path -LiteralPath (Join-Path $projectRoot '.env.local'))) {
        Copy-Item -LiteralPath (Join-Path $projectRoot '.env.example') -Destination (Join-Path $projectRoot '.env.local')
        Write-Host 'Đã tạo .env.local từ mẫu. Chỉnh file đó để chọn cấu hình API trước khi dùng MIRA.'
    }
    $releaseExe = Join-Path $projectRoot 'target/release/lantern-server.exe'
    $debugExe = Join-Path $projectRoot 'target/debug/lantern-server.exe'
    if ($Build -or ((-not (Test-Path -LiteralPath $releaseExe)) -and (-not (Test-Path -LiteralPath $debugExe)))) {
        $cargo = Get-Command cargo -ErrorAction SilentlyContinue
        if (-not $cargo) { throw 'Không tìm thấy Cargo. Cài Rust 1.89 trở lên và C compiler, sau đó chạy lại script.' }
        & $cargo.Source build --release -p lantern-server --locked
        if ($LASTEXITCODE -ne 0) { throw 'Rust build không thành công; xem lỗi ở trên.' }
    }
    $exe = if (Test-Path -LiteralPath $releaseExe) { $releaseExe } else { $debugExe }
    $port = 3210
    Get-Content -LiteralPath (Join-Path $projectRoot '.env.local') | ForEach-Object {
        if ($_ -match '^\s*LANTERN_PORT\s*=\s*(\d{1,5})\s*$') { $port = [int]$Matches[1] }
    }
    if ($port -lt 1 -or $port -gt 65535) { throw 'LANTERN_PORT không hợp lệ.' }
    $url = "http://127.0.0.1:$port"
    try {
        $health = Invoke-RestMethod -Uri "$url/api/health" -TimeoutSec 2
        if ($health.app -ne 'lantern') { throw 'Port này đang được một ứng dụng khác sử dụng.' }
        Write-Host "LANTERN đã chạy tại $url"
        Start-Process $url
        return
    } catch { if ($_.Exception.Message -like '*ứng dụng khác*') { throw } }
    $msysBin = 'C:/msys64/mingw64/bin'
    if (Test-Path -LiteralPath (Join-Path $msysBin 'libsqlite3-0.dll')) { $env:PATH = "$msysBin;$env:PATH" }
    $runDir = Join-Path $projectRoot 'data'
    New-Item -ItemType Directory -Path $runDir -Force | Out-Null
    $process = Start-Process -FilePath $exe -WorkingDirectory $projectRoot -WindowStyle Hidden -PassThru `
        -RedirectStandardOutput (Join-Path $runDir 'server.out.log') -RedirectStandardError (Join-Path $runDir 'server.err.log')
    $ready = $false
    for ($attempt = 0; $attempt -lt 50; $attempt++) {
        if ($process.HasExited) { throw 'LANTERN không khởi động được. Xem data/server.err.log.' }
        try {
            $health = Invoke-RestMethod -Uri "$url/api/health" -TimeoutSec 1
            if ($health.app -eq 'lantern') { $ready = $true; break }
        } catch { }
        Start-Sleep -Milliseconds 200
    }
    if (-not $ready) { throw "Chưa kết nối được LANTERN tại $url; xem data/server.err.log." }
    Write-Host "LANTERN đang chạy tại $url"
    Start-Process $url
} finally { Pop-Location }
