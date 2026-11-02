param([switch]$StageOnly, [string]$ReleaseBinary)
$ErrorActionPreference = 'Stop'
$ovmsProjectRoot = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$ovmsVersion = [regex]::Match((Get-Content -LiteralPath (Join-Path $ovmsProjectRoot 'Cargo.toml') -Raw), '(?m)^version = "([^"]+)"').Groups[1].Value
$ovmsRelease = if ($ReleaseBinary) { [IO.Path]::GetFullPath($ReleaseBinary) } else { Join-Path $ovmsProjectRoot 'target\x86_64-pc-windows-gnu\release\ovms-studio.exe' }
if (-not $ReleaseBinary -and -not (Test-Path -LiteralPath $ovmsRelease)) { $ovmsRelease = Join-Path $ovmsProjectRoot 'target\x86_64-pc-windows-msvc\release\ovms-studio.exe' }
if (-not $ReleaseBinary -and -not (Test-Path -LiteralPath $ovmsRelease)) { $ovmsRelease = Join-Path $ovmsProjectRoot 'target\release\ovms-studio.exe' }
if (-not (Test-Path -LiteralPath $ovmsRelease)) { throw 'Build the release executable first.' }
$ovmsRuntime = Join-Path $ovmsProjectRoot 'work\runtime-python-on\ovms'
$ovmsRuntimeSource = Join-Path $ovmsProjectRoot 'work\runtime-python-on\runtime-source.json'
foreach ($ovmsRequired in @('ovms.exe', 'openvino.dll', 'python\python.exe', 'LICENSE', 'thirdparty-licenses')) {
    if (-not (Test-Path -LiteralPath (Join-Path $ovmsRuntime $ovmsRequired))) { throw "The complete bundled OVMS runtime is missing: $ovmsRequired" }
}
if (-not (Test-Path -LiteralPath $ovmsRuntimeSource)) { throw 'Verified runtime source metadata is missing.' }
$ovmsBuildId = [guid]::NewGuid().ToString('N')
$ovmsStageParent = Join-Path $ovmsProjectRoot "work\package stages\$ovmsBuildId"
$ovmsStage = Join-Path $ovmsStageParent 'OVMS-Studio'
New-Item -ItemType Directory -Force -Path $ovmsStage | Out-Null
Copy-Item -LiteralPath $ovmsRelease -Destination (Join-Path $ovmsStage 'OVMS-Studio.exe')
foreach ($ovmsFile in @('LICENSE', 'README.md', 'QUICKSTART.txt')) {
    Copy-Item -LiteralPath (Join-Path $ovmsProjectRoot $ovmsFile) -Destination $ovmsStage
}
foreach ($ovmsFolder in @('examples', 'docs')) {
    Copy-Item -LiteralPath (Join-Path $ovmsProjectRoot $ovmsFolder) -Destination $ovmsStage -Recurse
}
Write-Output 'Copying the complete official OVMS runtime and its licenses...'
& (Join-Path $env:SystemRoot 'System32\robocopy.exe') $ovmsRuntime (Join-Path $ovmsStage 'ovms') /E /COPY:DAT /DCOPY:DAT /MT:8 /R:1 /W:1 /NFL /NDL /NJH /NJS /NP | Out-Null
if ($LASTEXITCODE -ge 8) { throw "Runtime copy failed with robocopy exit code $LASTEXITCODE" }
$global:LASTEXITCODE = 0
$ovmsSource = Get-Content -LiteralPath $ovmsRuntimeSource -Raw | ConvertFrom-Json
$ovmsManifest = [ordered]@{
    application_version = $ovmsVersion
    runtime_version = $ovmsSource.version
    runtime_flavor = $ovmsSource.flavor
    runtime_source = $ovmsSource.source_url
    runtime_archive_sha256 = $ovmsSource.archive_sha256
    application_sha256 = (Get-FileHash -LiteralPath (Join-Path $ovmsStage 'OVMS-Studio.exe') -Algorithm SHA256).Hash.ToLowerInvariant()
    runtime_executable_sha256 = (Get-FileHash -LiteralPath (Join-Path $ovmsStage 'ovms\ovms.exe') -Algorithm SHA256).Hash.ToLowerInvariant()
}
$ovmsManifest | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $ovmsStage 'bundle-manifest.json') -Encoding utf8
@{ stage = $ovmsStage; version = $ovmsVersion } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $ovmsProjectRoot 'work\bundle-stage.json') -Encoding utf8
if ($StageOnly) { Write-Output "Staged complete application: $ovmsStage"; return }

function Assert-OvmsWorkspacePath([string]$Candidate) {
    $ovmsAbsolute = [IO.Path]::GetFullPath($Candidate)
    if (-not $ovmsAbsolute.StartsWith($ovmsProjectRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw "Path escapes the workspace: $ovmsAbsolute" }
    if (Test-Path -LiteralPath $ovmsAbsolute) {
        $ovmsResolved = (Resolve-Path -LiteralPath $ovmsAbsolute).ProviderPath
        if (-not $ovmsResolved.StartsWith($ovmsProjectRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw "Resolved path escapes the workspace: $ovmsResolved" }
        if ((Get-Item -LiteralPath $ovmsAbsolute -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Refusing to move a reparse point: $ovmsAbsolute" }
    }
    return $ovmsAbsolute
}

$ovmsDist = Join-Path $ovmsProjectRoot 'dist'
New-Item -ItemType Directory -Force -Path $ovmsDist | Out-Null
$ovmsArchive = Join-Path $ovmsDist "OVMS-Studio-$ovmsVersion-windows-x64.zip"
python (Join-Path $PSScriptRoot 'create_bundle_archive.py') $ovmsStage $ovmsArchive
if ($LASTEXITCODE -ne 0) { throw 'Archive integrity verification failed.' }
$ovmsDestination = Assert-OvmsWorkspacePath (Join-Path $ovmsDist 'OVMS-Studio')
$ovmsStageChecked = Assert-OvmsWorkspacePath $ovmsStage
if (Test-Path -LiteralPath $ovmsDestination) {
    $ovmsBackup = Assert-OvmsWorkspacePath (Join-Path $ovmsProjectRoot "work\package-backups\OVMS-Studio-$ovmsBuildId")
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $ovmsBackup) | Out-Null
    Move-Item -LiteralPath $ovmsDestination -Destination $ovmsBackup
}
Move-Item -LiteralPath $ovmsStageChecked -Destination $ovmsDestination
@{ stage = $ovmsDestination; version = $ovmsVersion } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $ovmsProjectRoot 'work\bundle-stage.json') -Encoding utf8
Get-Item -LiteralPath $ovmsArchive | Select-Object FullName,Length
