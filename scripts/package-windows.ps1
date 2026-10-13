param(
    [string]$Version = "",
    [ValidateSet("x86_64-pc-windows-msvc")][string]$Target = "x86_64-pc-windows-msvc",
    [string]$OutputDir = "dist",
    [switch]$SkipBuild
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $root
try {
    $manifest = Get-Content -LiteralPath (Join-Path $root "Cargo.toml") -Raw
    $match = [regex]::Match($manifest, '(?m)^version\s*=\s*"([^"]+)"')
    if (-not $match.Success) { throw "Missing Cargo package version" }
    $packageVersion = $match.Groups[1].Value
    if ([string]::IsNullOrWhiteSpace($Version)) { $Version = "v$packageVersion" }
    if ($Version -ne "v$packageVersion") { throw "Package version and requested release tag do not match" }
    python scripts/release_notes.py --check-current
    if ($LASTEXITCODE -ne 0) { throw "Release metadata validation failed" }
    if (-not $SkipBuild) {
        cargo build --locked --release --target $Target --bin tiny-proxy
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    }
    $sourceExe = Join-Path $root "target\$Target\release\tiny-proxy.exe"
    if (-not (Test-Path -LiteralPath $sourceExe -PathType Leaf)) { throw "Compiled executable not found: $sourceExe" }
    if ((Get-Item -LiteralPath $sourceExe).VersionInfo.ProductVersion -ne $packageVersion) { throw "Executable ProductVersion does not match Cargo.toml" }
    if (-not [IO.Path]::IsPathRooted($OutputDir)) { $OutputDir = Join-Path $root $OutputDir }
    $OutputDir = [IO.Path]::GetFullPath($OutputDir)
    New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
    $portableBase = "tiny-proxy-$Version-windows-x86_64-portable"
    $portableArchive = Join-Path $OutputDir "$portableBase.zip"
    $installerBase = "tiny-proxy-$Version-windows-x86_64-setup"
    $installerPath = Join-Path $OutputDir "$installerBase.exe"
    foreach ($path in @($portableArchive, $installerPath)) {
        if (Test-Path -LiteralPath $path) { throw "Refusing to overwrite an existing release asset: $path" }
    }
    $stageRoot = Join-Path $OutputDir (".stage-" + [guid]::NewGuid().ToString("N"))
    $portableDir = Join-Path $stageRoot $portableBase
    New-Item -ItemType Directory -Path $portableDir -Force | Out-Null
    try {
        Copy-Item -LiteralPath $sourceExe -Destination (Join-Path $portableDir "tiny-proxy.exe")
        Compress-Archive -LiteralPath $portableDir -DestinationPath $portableArchive -CompressionLevel Optimal
        $iscc = @("${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:ProgramFiles\Inno Setup 6\ISCC.exe") |
            Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
        if (-not $iscc) { throw "Inno Setup 6 is required" }
        & $iscc "/DMyAppVersion=$packageVersion" "/DSourceExe=$sourceExe" "/DSetupIcon=$root\assets\icons\tiny-proxy.ico" "/DOutputDir=$OutputDir" "/DOutputBaseFilename=$installerBase" (Join-Path $PSScriptRoot "windows\tiny-proxy.iss")
        if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed" }
    } finally {
        # Resolve and constrain the recursive deletion to this unique stage.
        $checkedStage = [IO.Path]::GetFullPath($stageRoot)
        $outputPrefix = $OutputDir.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
        if (-not $checkedStage.StartsWith($outputPrefix, [StringComparison]::OrdinalIgnoreCase)) { throw "Staging path escaped output directory" }
        Remove-Item -LiteralPath $checkedStage -Recurse -Force
    }
    Write-Host "Created $portableArchive"
    Write-Host "Created $installerPath"
} finally { Pop-Location }
