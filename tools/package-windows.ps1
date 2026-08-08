param(
    [string]$Output = "dist",
    [Parameter(Mandatory = $true)][string]$Version
)
$ErrorActionPreference = "Stop"
if ($Version -notmatch '^[0-9]+\.[0-9]+\.[0-9]+([.+-][0-9A-Za-z.-]+)?$') { throw "invalid release version" }
if (Test-Path $Output) { throw "release output already exists: $Output" }
foreach ($Command in @("cargo-auditable", "syft")) {
    if (-not (Get-Command $Command -ErrorAction SilentlyContinue)) { throw "missing required tool: $Command" }
}
cargo auditable build --locked --release --package atrinik-editor
$Metadata = cargo metadata --locked --offline --format-version 1 --no-deps | ConvertFrom-Json
$StageRoot = Join-Path $env:RUNNER_TEMP ([System.IO.Path]::GetRandomFileName())
$Stage = Join-Path $StageRoot "atrinik-editor-$Version"
try {
    New-Item -ItemType Directory -Path "$Stage/bin", $Output | Out-Null
    Copy-Item (Join-Path $Metadata.target_directory "release/atrinik-editor.exe") "$Stage/bin/"
    Copy-Item LICENSE, PROVENANCE.md, THIRD_PARTY_NOTICES.md, policy/dependencies.json $Stage
    & "$Stage/bin/atrinik-editor.exe" version | Out-Null
    syft "$Stage/bin/atrinik-editor.exe" --source-name atrinik-editor --source-version $Version --output "cyclonedx-json=$Stage/sbom.cdx.json"
    $Sbom = Get-Content "$Stage/sbom.cdx.json" -Raw | ConvertFrom-Json
    if ($Sbom.components.Count -lt 10) { throw "binary SBOM is incomplete" }
    $Provenance = [ordered]@{
        schema_version = 1; version = $Version; revision = (git rev-parse HEAD); rust = (rustc --version)
        toolkit = [ordered]@{ release = "v1.0.0"; revision = "b2178d442af5d897a45619c200fec5ceb39fc3cf" }
        renderer = [ordered]@{ release = "v1.0.0"; revision = "3a6bbeabc2b7eac8d162d758732a0495fe8a9dd9" }
    }
    $Provenance | ConvertTo-Json -Depth 4 | Set-Content "$Stage/provenance.json" -Encoding utf8NoBOM
    $Archive = Join-Path $Output "atrinik-editor-$Version-windows-amd64.zip"
    Compress-Archive -Path $Stage -DestinationPath $Archive
    (Get-FileHash -Algorithm SHA256 $Archive).Hash.ToLowerInvariant() + "  " + (Split-Path $Archive -Leaf) | Set-Content (Join-Path $Output "SHA256SUMS.windows") -Encoding ascii
} finally {
    if (Test-Path $StageRoot) { Remove-Item -Recurse -Force $StageRoot }
}
