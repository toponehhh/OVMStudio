# Dot-source this file to use the workspace's portable toolchain.
$ErrorActionPreference = 'Stop'
$ovmsProjectRoot = Split-Path -Parent $PSScriptRoot
$ovmsToolchain = '1.99.1-x86_64-pc-windows-gnu'
$ovmsCompilerBin = Join-Path $ovmsProjectRoot 'work\toolchain\llvm-mingw-20261020-ucrt-x86_64\bin'
if (-not (Test-Path -LiteralPath (Join-Path $ovmsCompilerBin 'x86_64-w64-mingw32-clang.exe'))) {
    throw 'Portable compiler is missing. Use a Visual Studio developer shell with cargo, or prepare the LLVM-MinGW toolchain described in README.md.'
}
$ovmsRustRoot = (& rustup run $ovmsToolchain rustc --print sysroot).Trim()
if ($LASTEXITCODE -ne 0) { throw "Rust toolchain $ovmsToolchain is missing." }
$env:PATH = (Join-Path $ovmsRustRoot 'bin') + ';' + $ovmsCompilerBin + ';' + $env:PATH
$env:LIBRARY_PATH = Join-Path $ovmsRustRoot 'lib\rustlib\x86_64-pc-windows-gnu\lib\self-contained'
$env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = Join-Path $ovmsCompilerBin 'x86_64-w64-mingw32-clang.exe'
$env:CARGO_HOME = Join-Path $ovmsProjectRoot 'work\cargo-home'
