param(
    [string]$Output = "dist",
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$FontPack
)
$ErrorActionPreference = "Stop"
if ($Version -notmatch '^[0-9]+\.[0-9]+\.[0-9]+([.+-][0-9A-Za-z.-]+)?$') { throw "invalid release version" }
if (Test-Path $Output) { throw "release output already exists: $Output" }
foreach ($Command in @("cargo-auditable", "syft", "python")) {
    if (-not (Get-Command $Command -ErrorAction SilentlyContinue)) { throw "missing required tool: $Command" }
}
cargo auditable build --locked --release --package tilewright-editor
$Metadata = cargo metadata --locked --offline --format-version 1 --no-deps | ConvertFrom-Json
$StageRoot = Join-Path $env:RUNNER_TEMP ([System.IO.Path]::GetRandomFileName())
$Stage = Join-Path $StageRoot "tilewright-editor-$Version"
try {
    New-Item -ItemType Directory -Path "$Stage/bin", $Output | Out-Null
    Copy-Item (Join-Path $Metadata.target_directory "release/tilewright-editor.exe") "$Stage/bin/"
    Copy-Item LICENSE, PROVENANCE.md, THIRD_PARTY_NOTICES.md $Stage
    python tools/check-prototype-assets.py
    if ($LASTEXITCODE -ne 0) { throw "sample asset verification failed" }
    python tools/package-assets.py --root . --stage $Stage
    if ($LASTEXITCODE -ne 0) { throw "sample package allowlist failed" }
    python tools/package-font-notice.py --root . --stage $Stage | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "font package notice verification failed" }
    python tools/prepare-font-pack.py stage --root . --font-pack $FontPack --stage $Stage | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Unicode font pack staging failed" }
    & "$Stage/bin/tilewright-editor.exe" version | Out-Null
    syft "$Stage/bin/tilewright-editor.exe" --source-name tilewright-editor --source-version $Version --output "cyclonedx-json=$Stage/sbom.cdx.json"
    $Sbom = Get-Content "$Stage/sbom.cdx.json" -Raw | ConvertFrom-Json
    if ($Sbom.components.Count -lt 10) { throw "binary SBOM is incomplete" }
    $Digest = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes("tilewright-editor:windows-amd64:$Version"))).ToLowerInvariant()
    $Sbom.serialNumber = "urn:uuid:$($Digest.Substring(0, 8))-$($Digest.Substring(8, 4))-8$($Digest.Substring(13, 3))-8$($Digest.Substring(17, 3))-$($Digest.Substring(20, 12))"
    $Sbom.metadata.timestamp = "1970-01-01T00:00:00Z"
    $Sbom.metadata.component.'bom-ref' = "tilewright-editor-windows-amd64@$Version"
    foreach ($Component in $Sbom.components) {
        if ($Component.type -eq "file") { $Component.name = "/tilewright-editor.exe" }
    }
    $Sbom | ConvertTo-Json -Depth 20 -Compress | Set-Content "$Stage/sbom.cdx.json" -Encoding utf8NoBOM
    python tools/prepare-font-pack.py sbom --root . --stage $Stage --sbom "$Stage/sbom.cdx.json" | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Unicode font pack SBOM verification failed" }
    python tools/package-provenance.py --root . --version $Version --output "$Stage/provenance.json"
    if ($LASTEXITCODE -ne 0) { throw "package dependency provenance failed" }
    $Archive = Join-Path $Output "tilewright-editor-$Version-windows-amd64.zip"
    Compress-Archive -Path $Stage -DestinationPath $Archive
    (Get-FileHash -Algorithm SHA256 $Archive).Hash.ToLowerInvariant() + "  " + (Split-Path $Archive -Leaf) | Set-Content (Join-Path $Output "SHA256SUMS.windows") -Encoding ascii
} finally {
    if (Test-Path $StageRoot) { Remove-Item -Recurse -Force $StageRoot }
}
