$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
	Write-Error "cargo not found, install Rust from https://rustup.rs"
}

$triple = (rustc -vV | Select-String "^host: (.+)$").Matches[0].Groups[1].Value
if ($triple -like "*-msvc") {
	$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
	$vc = $null
	if (Test-Path $vswhere) {
		$vc = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
	}
	if (-not $vc -and -not (Get-Command cl -ErrorAction SilentlyContinue)) {
		Write-Error "MSVC not found, install the Visual Studio Build Tools with the ""Desktop development with C++"" workload"
	}
} elseif (-not (Get-Command g++ -ErrorAction SilentlyContinue)) {
	Write-Error "g++ not found, install MinGW-w64 or switch to the MSVC toolchain (rustup default stable-msvc)"
}

cargo build --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Write-Host "built target\release\gm_shadercompat.exe"
