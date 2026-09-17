param(
    [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+$')][string]$Version,
    [string]$Iscc = 'C:\Program Files (x86)\Inno Setup 6\ISCC.exe'
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    $cargo = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
    $installer = (Select-String -Path installer/ailimits.iss -Pattern '#define AppVersion "([^"]+)"').Matches[0].Groups[1].Value
    if ($Version -ne $cargo -or $Version -ne $installer) { throw 'Package versions must match' }
    if (-not (Test-Path -LiteralPath $Iscc)) { throw "Inno Setup compiler not found: $Iscc" }
    $files = @('target/release-min/ailimits.exe', 'target/release-min/ailimits-auth.exe', 'README.md', 'README.zh-CN.md', 'LICENSE', 'TRADEMARKS.md')
    foreach ($file in $files) {
        if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "Missing package input: $file" }
    }
    foreach ($binary in $files[0..1]) {
        $info = (Get-Item -LiteralPath $binary).VersionInfo
        $actual = "$($info.FileMajorPart).$($info.FileMinorPart).$($info.FileBuildPart)"
        if ($actual -ne $Version) { throw "Rebuild $binary for $Version (found $actual)" }
        $reader = [IO.BinaryReader]::new([IO.File]::OpenRead((Resolve-Path $binary)))
        try {
            $reader.BaseStream.Position = 0x3c
            $peOffset = $reader.ReadInt32()
            $reader.BaseStream.Position = $peOffset
            if ($reader.ReadUInt32() -ne 0x4550 -or $reader.ReadUInt16() -ne 0x8664) {
                throw "Not a Windows x64 PE binary: $binary"
            }
        } finally { $reader.Dispose() }
    }
    New-Item -ItemType Directory -Force target/installer | Out-Null
    & $Iscc installer/ailimits.iss
    if ($LASTEXITCODE -ne 0) { throw 'Inno Setup compilation failed' }
    $setup = "target/installer/AiLimits-Setup-$Version.exe"
    $zip = "target/installer/AiLimits-Portable-$Version.zip"
    Compress-Archive -LiteralPath $files -DestinationPath $zip -Force
    $archive = [IO.Compression.ZipFile]::OpenRead((Resolve-Path $zip))
    try {
        $expected = @($files | ForEach-Object { Split-Path $_ -Leaf })
        $actual = @($archive.Entries | ForEach-Object FullName)
        if (Compare-Object $expected $actual) { throw 'Portable archive contents do not match package inputs' }
    } finally { $archive.Dispose() }
    $checksums = foreach ($file in @($setup, $zip)) {
        $hash = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()
        "$hash  $(Split-Path $file -Leaf)"
    }
    $checksums | Set-Content target/installer/SHA256SUMS.txt -Encoding ascii
    $checksums
} finally { Pop-Location }
