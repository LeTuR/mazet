#Requires -Version 5.1
<#
.SYNOPSIS
    mazet installer for Windows.

.DESCRIPTION
    Resolves the latest mazet release (or one you name), downloads the Windows
    archive, verifies it against the release's SHA256 checksum file and extracts
    mazet.exe into an install directory, naming that directory and what to add
    to PATH when it is not already there.

    The mirror of install.sh, which covers Linux and macOS. The five target
    triples, the archive names and the checksum file are
    .github/workflows/cd.yml's output and are the contract between the two;
    see docs/RELEASING.md.

    This file is deliberately ASCII-only. Windows PowerShell 5.1 decodes an
    `irm | iex` payload as the system ANSI code page, so a non-ASCII byte here
    reaches the parser as something else on a machine whose code page differs.
    Write-Host, not Write-Output, for the same reason: Write-Output would put
    the installer's chatter into the iex pipeline.

.EXAMPLE
    irm https://raw.githubusercontent.com/LeTuR/mazet/main/install.ps1 | iex

.NOTES
    The pipe-to-iex form cannot pass parameters, so every option is an
    environment variable, and prefixed, exactly as in install.sh:
      $env:MAZET_VERSION      = 'v0.1.0'
      $env:MAZET_INSTALL_DIR  = 'C:\tools\mazet'
      $env:MAZET_REPO         = 'LeTuR/mazet'
#>

# Windows PowerShell 5.1 defaults to SSL3/TLS1.0, which github.com refuses.
if ([Net.ServicePointManager]::SecurityProtocol -notmatch 'Tls12') {
    [Net.ServicePointManager]::SecurityProtocol =
        [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
}

# Environment variable beats default.
$Repo = if ($env:MAZET_REPO) { $env:MAZET_REPO } else { 'LeTuR/mazet' }
$Version = if ($env:MAZET_VERSION) { $env:MAZET_VERSION } else { '' }
if ($env:MAZET_INSTALL_DIR) { $InstallDir = $env:MAZET_INSTALL_DIR }
elseif ($env:LOCALAPPDATA) { $InstallDir = Join-Path $env:LOCALAPPDATA 'Programs\mazet' }
else { $InstallDir = Join-Path $HOME '.mazet\bin' }

function Write-MazetInfo { param($Message) Write-Host "  $Message" }
function Write-MazetWarn { param($Message) Write-Host "  $Message" -ForegroundColor Yellow }
function Write-MazetOk { param($Message) Write-Host $Message -ForegroundColor Green }

# --- pure helpers -----------------------------------------------------------
# Value in, value out, so the Pester suite can exercise them without
# performing an install.

<#
.SYNOPSIS
    The release target triple for a Windows processor architecture.
#>
function Get-MazetTarget {
    param([string]$Architecture = $env:PROCESSOR_ARCHITECTURE)

    switch ($Architecture) {
        'AMD64' { return 'x86_64-pc-windows-msvc' }
        'ARM64' {
            # There is no aarch64-pc-windows-msvc build; the x64 one runs under
            # the emulation layer every ARM64 Windows ships with. Said out loud
            # rather than done quietly.
            Write-MazetWarn 'ARM64 Windows detected; installing the x86_64 build, which runs under emulation.'
            return 'x86_64-pc-windows-msvc'
        }
        'x86' { throw 'mazet publishes no 32-bit Windows build (detected x86).' }
        default { throw "mazet publishes no Windows build for the architecture '$Architecture'. The built Windows target is x86_64." }
    }
}

<#
.SYNOPSIS
    Normalises a version to the `v`-prefixed spelling the release assets use.
#>
function Get-MazetTag {
    param([Parameter(Mandatory = $true)][string]$Version)
    if ($Version.StartsWith('v')) { return $Version }
    return "v$Version"
}

<#
.SYNOPSIS
    The expected SHA256 for one archive, out of mazet-<tag>-checksums.txt.

.DESCRIPTION
    cd.yml produces that file with `sha256sum ./*.tar.gz ./*.zip` run from
    inside the assets directory, so every name in it carries a `./` prefix; a
    file written with `sha256sum -b` would mark binary mode with a leading `*`.
    Both are stripped and the names are compared as strings, which needs no
    regex escaping of a name that is full of dots.
#>
function Get-MazetExpectedChecksum {
    param(
        [Parameter(Mandatory = $true)][string]$ChecksumFile,
        [Parameter(Mandatory = $true)][string]$ArchiveName
    )

    foreach ($line in (Get-Content -LiteralPath $ChecksumFile)) {
        $fields = $line.Trim() -split '\s+', 2
        if ($fields.Count -lt 2) { continue }
        $name = $fields[1].Trim()
        if ($name.StartsWith('*')) { $name = $name.Substring(1) }
        if ($name.StartsWith('./')) { $name = $name.Substring(2) }
        if ($name -eq $ArchiveName) { return $fields[0].Trim().ToLowerInvariant() }
    }
    throw "$ArchiveName is not listed in the checksum file. The release may still be uploading; check https://github.com/$Repo/releases"
}

<#
.SYNOPSIS
    Whether a directory is already a component of a PATH string.
#>
function Test-MazetOnPath {
    param([AllowNull()][string]$ExistingPath, [Parameter(Mandatory = $true)][string]$Directory)

    $entries = @()
    if ($ExistingPath) {
        $entries = @($ExistingPath -split ';' | Where-Object { $_ -ne '' })
    }
    # Windows paths are case-insensitive, and a trailing separator names the
    # same directory, so neither may count as a different entry.
    $normalized = $Directory.TrimEnd('\', '/')
    foreach ($entry in $entries) {
        if ($entry.TrimEnd('\', '/') -ieq $normalized) { return $true }
    }
    return $false
}

# --- the world --------------------------------------------------------------

function Get-MazetLatestVersion {
    # The API answers with the newest published release. An unauthenticated
    # caller behind a shared address can be rate-limited, so fall back to the
    # page github.com/<repo>/releases/latest redirects to.
    try {
        $release = Invoke-RestMethod -UseBasicParsing `
            -Uri "https://api.github.com/repos/$Repo/releases/latest" `
            -Headers @{ 'User-Agent' = 'mazet-installer' }
        if ($release.tag_name) { return $release.tag_name }
    } catch {
        Write-Verbose "Releases API unavailable: $($_.Exception.Message)"
    }

    try {
        $page = Invoke-WebRequest -UseBasicParsing -Uri "https://github.com/$Repo/releases/latest"
        $match = [regex]::Match($page.Content, 'releases/tag/(v[0-9][0-9A-Za-z.+\-]*)')
        if ($match.Success) { return $match.Groups[1].Value }
    } catch {
        Write-Verbose "Releases page unavailable: $($_.Exception.Message)"
    }

    throw "Could not work out the latest release of $Repo. Name one instead: `$env:MAZET_VERSION = 'v0.1.0'"
}

<#
.SYNOPSIS
    Move mazet.exe out of an extracted archive into the install directory.

.DESCRIPTION
    Only the binary: the archive also carries LICENSE and README.md, which
    belong nowhere near a bin directory. Windows will not let a running
    executable be overwritten but will let it be renamed, so an existing
    mazet.exe is moved aside first and the leftover swept up on the next run.
#>
function Install-MazetBinary {
    param(
        [Parameter(Mandatory = $true)][string]$ExtractedDir,
        [Parameter(Mandatory = $true)][string]$Destination
    )

    $source = Join-Path $ExtractedDir 'mazet.exe'
    if (-not (Test-Path -LiteralPath $source)) {
        throw "The release archive does not contain mazet.exe."
    }

    New-Item -ItemType Directory -Path $Destination -Force | Out-Null
    $target = Join-Path $Destination 'mazet.exe'
    $displaced = Join-Path $Destination 'mazet.exe.old'

    Remove-Item -LiteralPath $displaced -Force -ErrorAction SilentlyContinue
    if (Test-Path -LiteralPath $target) {
        Move-Item -LiteralPath $target -Destination $displaced -Force
    }
    Move-Item -LiteralPath $source -Destination $target -Force
    Remove-Item -LiteralPath $displaced -Force -ErrorAction SilentlyContinue
    return $target
}

function Invoke-MazetInstall {
    # Set here, not at script scope: `irm | iex` runs the script in the console
    # the user keeps on using. The callees inherit all three dynamically.
    Set-StrictMode -Version Latest
    $ErrorActionPreference = 'Stop'
    # Invoke-WebRequest's progress bar costs more than the download in 5.1.
    $ProgressPreference = 'SilentlyContinue'

    Write-Host 'mazet installer'

    $target = Get-MazetTarget
    Write-MazetInfo "platform  Windows $($env:PROCESSOR_ARCHITECTURE) -> $target"

    $tag = if ($Version) { Get-MazetTag -Version $Version } else { Get-MazetLatestVersion }
    Write-MazetInfo "version   $tag"

    $archive = "mazet-$tag-$target.zip"
    $base = "https://github.com/$Repo/releases/download/$tag"
    $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("mazet-install-" + [System.Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tmp -Force | Out-Null

    try {
        Write-MazetInfo "fetching  $archive"
        $checksumPath = Join-Path $tmp 'checksums.txt'
        try {
            Invoke-WebRequest -UseBasicParsing -Uri "$base/mazet-$tag-checksums.txt" -OutFile $checksumPath
        } catch {
            throw "No checksum file for $tag. Check https://github.com/$Repo/releases/tag/$tag"
        }

        $zipPath = Join-Path $tmp $archive
        try {
            Invoke-WebRequest -UseBasicParsing -Uri "$base/$archive" -OutFile $zipPath
        } catch {
            throw "Could not download $archive. Check https://github.com/$Repo/releases/tag/$tag"
        }

        # The reason the checksum file is published at all. An installer that
        # downloads and runs without checking it is worse than no installer.
        $expected = Get-MazetExpectedChecksum -ChecksumFile $checksumPath -ArchiveName $archive
        $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $zipPath).Hash.ToLowerInvariant()
        if ($actual -ne $expected) {
            throw "Checksum mismatch for $archive`n  expected $expected`n  got      $actual`nNothing was installed."
        }
        Write-MazetInfo "verified  sha256 $expected"

        $extracted = Join-Path $tmp 'unpacked'
        Expand-Archive -LiteralPath $zipPath -DestinationPath $extracted -Force
        $installed = Install-MazetBinary -ExtractedDir $extracted -Destination $InstallDir
    } finally {
        Remove-Item -Recurse -Force -LiteralPath $tmp -ErrorAction SilentlyContinue
    }

    Write-Host ''
    Write-MazetOk "mazet $tag is installed at $installed"

    if (-not (Test-MazetOnPath -ExistingPath $env:Path -Directory $InstallDir)) {
        Write-Host ''
        Write-MazetWarn "$InstallDir is not on your PATH. Add it:"
        Write-Host "    `$env:Path = `"$InstallDir;`$env:Path`""
        Write-Host 'and put that line in your $PROFILE.'
    }

    Write-Host ''
    Write-Host 'Next: `mazet init` in a directory tree to bind it to an Azure identity'
    Write-Host 'of its own, and add this to your $PROFILE so bare `az` follows it:'
    Write-Host '    Invoke-Expression (& mazet hook powershell | Out-String)'
}

# Dot-source with MAZET_PS_TEST set to get the helpers without installing.
if (-not $env:MAZET_PS_TEST) {
    Invoke-MazetInstall
}
