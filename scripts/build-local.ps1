param([switch]$Release, [switch]$Test, [switch]$Clippy, [switch]$RealTest)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'env-local.ps1')
Push-Location $ovmsProjectRoot
try {
    $ovmsCargoArguments = @()
    if ($Clippy) { $ovmsCargoArguments += @('clippy', '--all-targets') }
    elseif ($Test) { $ovmsCargoArguments += 'test' }
    else { $ovmsCargoArguments += 'build' }
    $ovmsCargoArguments += @('--locked', '--offline', '--target', 'x86_64-pc-windows-gnu')
    if ($Release) { $ovmsCargoArguments += '--release' }
    if ($RealTest) {
        $env:OVMS_TEST_BUNDLE = Join-Path $ovmsProjectRoot 'work\runtime-python-on'
        $ovmsCargoArguments = @('test', '--locked', '--offline', '--target', 'x86_64-pc-windows-gnu', '--test', 'real_ovms', '--', '--ignored', '--test-threads=1')
    }
    if ($Clippy) { $ovmsCargoArguments += @('--', '-D', 'warnings') }
    & (Join-Path $ovmsRustRoot 'bin\cargo.exe') @ovmsCargoArguments
    if ($LASTEXITCODE -ne 0) { throw "Cargo failed with exit code $LASTEXITCODE" }
}
finally { Pop-Location }
