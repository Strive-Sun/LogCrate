param(
  [ValidateRange(16, 2048)]
  [int]$SizeMiB = 256,
  [ValidateRange(32, 2048)]
  [int]$MaxMemoryMiB = 192,
  [string]$ReportPath = ""
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($ReportPath)) {
  $ReportPath = Join-Path $root "src-tauri\target\performance\windows-archive-baseline.md"
}

$env:LOGCRATE_PERF_MIB = $SizeMiB.ToString()
$env:LOGCRATE_PERF_MAX_MEMORY_MIB = $MaxMemoryMiB.ToString()
$env:LOGCRATE_PERF_REPORT = [System.IO.Path]::GetFullPath($ReportPath)
$env:LOGCRATE_PERF_DATE = (Get-Date -Format "yyyy-MM-dd HH:mm:ss zzz")

Push-Location $root
try {
  cargo test --release --manifest-path src-tauri/Cargo.toml windows_archive_performance_baseline -- --ignored --nocapture --test-threads=1
  if ($LASTEXITCODE -ne 0) {
    throw "Windows archive baseline failed with exit code $LASTEXITCODE"
  }
  Write-Output "Performance report: $env:LOGCRATE_PERF_REPORT"
} finally {
  Pop-Location
}
