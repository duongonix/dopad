param(
    [switch] $CompileInstaller
)

$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$target = Join-Path $root "target/release"
$stage = Join-Path $root "dist/windows/Dopad"
$bin = Join-Path $stage "bin"

function Write-DopadIcon {
    param(
        [Parameter(Mandatory = $true)][string] $Source,
        [Parameter(Mandatory = $true)][string] $Destination
    )

    Add-Type -AssemblyName System.Drawing
    $sourceImage = [System.Drawing.Image]::FromFile($Source)
    $frames = [System.Collections.Generic.List[object]]::new()
    try {
        foreach ($size in @(16, 24, 32, 48, 64, 128, 256)) {
            $bitmap = [System.Drawing.Bitmap]::new(
                $size,
                $size,
                [System.Drawing.Imaging.PixelFormat]::Format32bppArgb
            )
            $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
            $stream = [System.IO.MemoryStream]::new()
            try {
                $graphics.Clear([System.Drawing.Color]::Transparent)
                $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
                $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
                $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
                $graphics.DrawImage($sourceImage, 0, 0, $size, $size)
                $bitmap.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
                $frames.Add([PSCustomObject]@{
                    Size = $size
                    Bytes = $stream.ToArray()
                })
            } finally {
                $stream.Dispose()
                $graphics.Dispose()
                $bitmap.Dispose()
            }
        }
    } finally {
        $sourceImage.Dispose()
    }

    $iconStream = [System.IO.MemoryStream]::new()
    $writer = [System.IO.BinaryWriter]::new($iconStream)
    try {
        $writer.Write([UInt16]0)
        $writer.Write([UInt16]1)
        $writer.Write([UInt16]$frames.Count)
        $imageOffset = 6 + (16 * $frames.Count)
        foreach ($frame in $frames) {
            $dimension = if ($frame.Size -eq 256) { [Byte]0 } else { [Byte]$frame.Size }
            $writer.Write($dimension)
            $writer.Write($dimension)
            $writer.Write([Byte]0)
            $writer.Write([Byte]0)
            $writer.Write([UInt16]1)
            $writer.Write([UInt16]32)
            $writer.Write([UInt32]$frame.Bytes.Length)
            $writer.Write([UInt32]$imageOffset)
            $imageOffset += $frame.Bytes.Length
        }
        foreach ($frame in $frames) {
            $writer.Write([Byte[]]$frame.Bytes)
        }
        $writer.Flush()
        [System.IO.File]::WriteAllBytes($Destination, $iconStream.ToArray())
    } finally {
        $writer.Dispose()
        $iconStream.Dispose()
    }

    $iconBytes = [System.IO.File]::ReadAllBytes($Destination)
    if ($iconBytes.Length -lt 6 + (16 * $frames.Count)) {
        throw "Generated Dopad icon has a truncated directory: $Destination"
    }
    $imageCount = [BitConverter]::ToUInt16($iconBytes, 4)
    $largestImage = 6 + (16 * ($imageCount - 1))
    if ($imageCount -ne $frames.Count -or $iconBytes[$largestImage] -ne 0 -or $iconBytes[$largestImage + 1] -ne 0) {
        throw "Generated Dopad icon is missing its 256x256 image: $Destination"
    }
    $icon = [System.Drawing.Icon]::new($Destination)
    try {
        if ($icon.Width -le 0 -or $icon.Height -le 0) {
            throw "Generated Dopad icon cannot be decoded: $Destination"
        }
    } finally {
        $icon.Dispose()
    }
}

function Assert-WindowsSubsystem {
    param(
        [Parameter(Mandatory = $true)][string] $Path,
        [Parameter(Mandatory = $true)][UInt16] $ExpectedSubsystem
    )

    $bytes = [System.IO.File]::ReadAllBytes($Path)
    if ($bytes.Length -lt 256) { throw "PE executable is too small: $Path" }
    $peOffset = [BitConverter]::ToInt32($bytes, 0x3c)
    if ($peOffset -lt 0x40 -or $peOffset + 94 -gt $bytes.Length) {
        throw "PE header is invalid: $Path"
    }
    $signature = [System.Text.Encoding]::ASCII.GetString($bytes, $peOffset, 4)
    if ($signature -ne "PE`0`0") { throw "PE signature is invalid: $Path" }
    $subsystem = [BitConverter]::ToUInt16($bytes, $peOffset + 24 + 68)
    if ($subsystem -ne $ExpectedSubsystem) {
        throw "Unexpected PE subsystem $subsystem in $Path (expected $ExpectedSubsystem)"
    }
}

Push-Location $root
try {
    $metadata = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw "Could not read Cargo workspace metadata" }
    $appVersion = ($metadata.packages | Where-Object { $_.name -eq "dopad-app" } | Select-Object -First 1).version
    if (-not $appVersion) { throw "Could not determine Dopad version from Cargo metadata" }

    New-Item -ItemType Directory -Force -Path $stage | Out-Null
    $iconSource = Join-Path $root "crates/dopad-app/assets/dopad-icon.png"
    $embeddedIcon = Join-Path $root "crates/dopad-app/assets/dopad.ico"
    $iconPath = Join-Path $stage "dopad.ico"
    Write-DopadIcon -Source $iconSource -Destination $embeddedIcon
    Write-DopadIcon -Source $iconSource -Destination $iconPath
    if (-not (Test-Path -LiteralPath $iconPath -PathType Leaf)) { throw "Staged Dopad icon is missing" }

    cargo build --release --locked -p dopad-app -p dopad-cli
    if ($LASTEXITCODE -ne 0) { throw "Cargo release build failed with exit code $LASTEXITCODE" }

    Assert-WindowsSubsystem -Path (Join-Path $target "dopad-app.exe") -ExpectedSubsystem 2
    Assert-WindowsSubsystem -Path (Join-Path $target "dopad.exe") -ExpectedSubsystem 3

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
        $iscc = Get-Command iscc.exe -ErrorAction SilentlyContinue | Select-Object -First 1
        $isccPath = if ($iscc) { $iscc.Source } else {
            @(
                (Join-Path ([Environment]::GetFolderPath("ProgramFilesX86")) "Inno Setup 6\ISCC.exe"),
                (Join-Path ([Environment]::GetFolderPath("ProgramFiles")) "Inno Setup 6\ISCC.exe"),
                (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 6\ISCC.exe")
            ) | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
        }
        if (-not $isccPath) { throw "Inno Setup ISCC is required to compile the installer" }
        & $isccPath "/DAppVersion=$appVersion" (Join-Path $root "packaging/windows/dopad.iss")
        if ($LASTEXITCODE -ne 0) { throw "Inno Setup compilation failed" }
    }
} finally {
    Pop-Location
}
