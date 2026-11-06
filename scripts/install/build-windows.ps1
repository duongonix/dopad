param(
    [switch] $CompileInstaller
)

$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$target = Join-Path $root "target/release"
$stage = Join-Path $root "dist/windows/Dopad"
$bin = Join-Path $stage "bin"

Push-Location $root
try {
    $metadata = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw "Could not read Cargo workspace metadata" }
    $appVersion = ($metadata.packages | Where-Object { $_.name -eq "dopad-app" } | Select-Object -First 1).version
    if (-not $appVersion) { throw "Could not determine Dopad version from Cargo metadata" }

    cargo build --release --locked -p dopad-app -p dopad-cli
    if ($LASTEXITCODE -ne 0) { throw "Cargo release build failed with exit code $LASTEXITCODE" }

    New-Item -ItemType Directory -Force -Path $bin | Out-Null
    Copy-Item -LiteralPath (Join-Path $target "dopad-app.exe") -Destination (Join-Path $stage "dopad-gui.exe") -Force
    Copy-Item -LiteralPath (Join-Path $target "dopad.exe") -Destination (Join-Path $bin "dopad.exe") -Force

    $cli = Join-Path $bin "dopad.exe"
    if (-not (Test-Path -LiteralPath $cli -PathType Leaf)) { throw "Staged CLI binary is missing" }
    $cliVersion = & $cli --version
    if ($LASTEXITCODE -ne 0 -or $cliVersion -ne "dopad $appVersion") { throw "Staged CLI version smoke test failed" }
    & $cli --help | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Staged CLI help smoke test failed" }

    if ($CompileInstaller) {
        $iscc = Get-Command iscc.exe -ErrorAction SilentlyContinue
        if (-not $iscc) { throw "Inno Setup ISCC is required to compile the installer" }
        & $iscc.Source "/DAppVersion=$appVersion" (Join-Path $root "packaging/windows/dopad.iss")
        if ($LASTEXITCODE -ne 0) { throw "Inno Setup compilation failed" }
    }
} finally {
    Pop-Location
}
