#Requires -Version 5.1
<#
.SYNOPSIS
    Pester tests for install.ps1, the mirror of tests/install.bats.

.DESCRIPTION
    Behaviour of the pure helpers, plus the source properties an `irm | iex`
    install depends on: the file parses, and it is ASCII with no BOM.

    install.ps1 guards Invoke-MazetInstall behind $env:MAZET_PS_TEST, so
    dot-sourcing it here defines every function without performing an install.
    The helpers read no machine state that is not passed to them, so this runs
    wherever PowerShell does and does not need a Windows runner.

.EXAMPLE
    Invoke-Pester -Path tests/install.Tests.ps1
#>

BeforeAll {
    $script:ScriptPath = Join-Path (Split-Path -Parent $PSScriptRoot) 'install.ps1'
    $env:MAZET_PS_TEST = '1'
    . $script:ScriptPath
}

AfterAll {
    Remove-Item Env:\MAZET_PS_TEST -ErrorAction SilentlyContinue
}

Describe 'install.ps1 source' {
    It 'parses' {
        $tokens = $null
        $errors = $null
        [System.Management.Automation.Language.Parser]::ParseFile(
            $script:ScriptPath, [ref]$tokens, [ref]$errors) | Out-Null
        $errors | Should -BeNullOrEmpty
    }

    It 'is ASCII only, which is what survives irm | iex on Windows PowerShell 5.1' {
        $bytes = [System.IO.File]::ReadAllBytes($script:ScriptPath)
        ($bytes | Where-Object { $_ -gt 127 } | Measure-Object).Count | Should -Be 0
    }

    It 'carries no UTF-8 BOM, which iex would parse as content' {
        $bytes = [System.IO.File]::ReadAllBytes($script:ScriptPath)
        ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) |
            Should -BeFalse
    }

    It 'says nothing on stdout when dot-sourced, so it cannot pollute an iex pipeline' {
        $out = & {
            $env:MAZET_PS_TEST = '1'
            . $script:ScriptPath
        }
        $out | Should -BeNullOrEmpty
    }
}

Describe 'Get-MazetTarget' {
    It 'maps AMD64 to the Windows target cd.yml builds' {
        Get-MazetTarget -Architecture 'AMD64' | Should -Be 'x86_64-pc-windows-msvc'
    }

    It 'maps ARM64 to the x86_64 build, which runs under emulation' {
        Get-MazetTarget -Architecture 'ARM64' | Should -Be 'x86_64-pc-windows-msvc'
    }

    It 'refuses 32-bit Windows and names what it detected' {
        { Get-MazetTarget -Architecture 'x86' } | Should -Throw '*x86*'
    }

    It 'refuses an unknown architecture and names what it detected' {
        { Get-MazetTarget -Architecture 'SPARC' } | Should -Throw '*SPARC*'
    }

    It 'reads PROCESSOR_ARCHITECTURE when no argument is given' {
        $saved = $env:PROCESSOR_ARCHITECTURE
        try {
            $env:PROCESSOR_ARCHITECTURE = 'AMD64'
            Get-MazetTarget | Should -Be 'x86_64-pc-windows-msvc'
        } finally {
            $env:PROCESSOR_ARCHITECTURE = $saved
        }
    }
}

Describe 'Get-MazetTag' {
    It 'leaves a v-prefixed tag alone' {
        Get-MazetTag -Version 'v0.1.0' | Should -Be 'v0.1.0'
    }

    It 'adds the v the release assets are named with' {
        Get-MazetTag -Version '0.1.0' | Should -Be 'v0.1.0'
    }
}

Describe 'Get-MazetExpectedChecksum' {
    BeforeAll {
        $script:Archive = 'mazet-v1.2.3-x86_64-pc-windows-msvc.zip'
        $script:Hash = '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'
    }

    BeforeEach {
        $script:Checksums = Join-Path $TestDrive 'checksums.txt'
    }

    It 'reads the hash for the matching archive' {
        Set-Content -LiteralPath $script:Checksums -Value "$script:Hash  $script:Archive"
        Get-MazetExpectedChecksum -ChecksumFile $script:Checksums -ArchiveName $script:Archive |
            Should -Be $script:Hash
    }

    It 'tolerates the ./ prefix cd.yml produces' {
        Set-Content -LiteralPath $script:Checksums -Value "$script:Hash  ./$script:Archive"
        Get-MazetExpectedChecksum -ChecksumFile $script:Checksums -ArchiveName $script:Archive |
            Should -Be $script:Hash
    }

    It "tolerates sha256sum's binary-mode asterisk" {
        Set-Content -LiteralPath $script:Checksums -Value "$script:Hash *$script:Archive"
        Get-MazetExpectedChecksum -ChecksumFile $script:Checksums -ArchiveName $script:Archive |
            Should -Be $script:Hash
    }

    It 'lowercases an uppercase hash, so the comparison can be a plain equality' {
        Set-Content -LiteralPath $script:Checksums -Value "$($script:Hash.ToUpperInvariant())  ./$script:Archive"
        Get-MazetExpectedChecksum -ChecksumFile $script:Checksums -ArchiveName $script:Archive |
            Should -Be $script:Hash
    }

    It 'picks the right line out of a whole release' {
        $other = 'a' * 64
        Set-Content -LiteralPath $script:Checksums -Value @(
            "$other  ./mazet-v1.2.3-x86_64-unknown-linux-gnu.tar.gz"
            "$other  ./mazet-v1.2.3-aarch64-unknown-linux-gnu.tar.gz"
            "$other  ./mazet-v1.2.3-x86_64-apple-darwin.tar.gz"
            "$other  ./mazet-v1.2.3-aarch64-apple-darwin.tar.gz"
            "$script:Hash  ./$script:Archive"
        )
        Get-MazetExpectedChecksum -ChecksumFile $script:Checksums -ArchiveName $script:Archive |
            Should -Be $script:Hash
    }

    It 'refuses an archive the file does not list' {
        Set-Content -LiteralPath $script:Checksums -Value "$script:Hash  ./some-other-file.zip"
        { Get-MazetExpectedChecksum -ChecksumFile $script:Checksums -ArchiveName $script:Archive } |
            Should -Throw "*$script:Archive*"
    }

    It 'does not match a name by prefix' {
        Set-Content -LiteralPath $script:Checksums -Value "$script:Hash  ./$script:Archive"
        { Get-MazetExpectedChecksum -ChecksumFile $script:Checksums -ArchiveName "$script:Archive.sig" } |
            Should -Throw
    }
}

Describe 'Add-MazetPathEntry' {
    It 'appends a directory that is absent' {
        Add-MazetPathEntry -ExistingPath 'C:\Windows;C:\Windows\System32' -Directory 'C:\tools\mazet' |
            Should -Be 'C:\Windows;C:\Windows\System32;C:\tools\mazet'
    }

    It 'returns nothing when the directory is already there' {
        Add-MazetPathEntry -ExistingPath 'C:\Windows;C:\tools\mazet' -Directory 'C:\tools\mazet' |
            Should -BeNullOrEmpty
    }

    It 'treats a differently cased entry as the same directory' {
        Add-MazetPathEntry -ExistingPath 'C:\Tools\Mazet' -Directory 'C:\tools\mazet' |
            Should -BeNullOrEmpty
    }

    It 'treats a trailing separator as the same directory' {
        Add-MazetPathEntry -ExistingPath 'C:\tools\mazet\' -Directory 'C:\tools\mazet' |
            Should -BeNullOrEmpty
    }

    It 'handles an empty PATH' {
        Add-MazetPathEntry -ExistingPath '' -Directory 'C:\tools\mazet' | Should -Be 'C:\tools\mazet'
    }

    It 'handles an unset PATH' {
        Add-MazetPathEntry -ExistingPath $null -Directory 'C:\tools\mazet' | Should -Be 'C:\tools\mazet'
    }

    It 'drops the empty entries a trailing semicolon leaves' {
        Add-MazetPathEntry -ExistingPath 'C:\Windows;;' -Directory 'C:\tools\mazet' |
            Should -Be 'C:\Windows;C:\tools\mazet'
    }
}

Describe 'Install-MazetBinary' {
    BeforeEach {
        $script:Extracted = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString('N'))
        $script:Dest = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Path $script:Extracted -Force | Out-Null
        # What cd.yml's Compress-Archive step puts in the zip.
        Set-Content -LiteralPath (Join-Path $script:Extracted 'mazet.exe') -Value 'first'
        Set-Content -LiteralPath (Join-Path $script:Extracted 'LICENSE') -Value 'MIT'
        Set-Content -LiteralPath (Join-Path $script:Extracted 'README.md') -Value '# mazet'
    }

    It 'puts mazet.exe in the install directory and returns its path' {
        $installed = Install-MazetBinary -ExtractedDir $script:Extracted -Destination $script:Dest
        $installed | Should -Be (Join-Path $script:Dest 'mazet.exe')
        Test-Path -LiteralPath $installed | Should -BeTrue
    }

    It 'leaves LICENSE and README.md out of the install directory' {
        Install-MazetBinary -ExtractedDir $script:Extracted -Destination $script:Dest | Out-Null
        (Get-ChildItem -LiteralPath $script:Dest).Name | Should -Be 'mazet.exe'
    }

    It 'creates the install directory when it is absent' {
        $deep = Join-Path $script:Dest 'nested\bin'
        Install-MazetBinary -ExtractedDir $script:Extracted -Destination $deep | Out-Null
        Test-Path -LiteralPath (Join-Path $deep 'mazet.exe') | Should -BeTrue
    }

    It 'replaces an older binary and sweeps up the displaced copy' {
        Install-MazetBinary -ExtractedDir $script:Extracted -Destination $script:Dest | Out-Null
        Set-Content -LiteralPath (Join-Path $script:Extracted 'mazet.exe') -Value 'second'
        Install-MazetBinary -ExtractedDir $script:Extracted -Destination $script:Dest | Out-Null
        Get-Content -LiteralPath (Join-Path $script:Dest 'mazet.exe') | Should -Be 'second'
        (Get-ChildItem -LiteralPath $script:Dest).Name | Should -Be 'mazet.exe'
    }

    It 'refuses an archive with no mazet.exe in it' {
        Remove-Item -LiteralPath (Join-Path $script:Extracted 'mazet.exe')
        { Install-MazetBinary -ExtractedDir $script:Extracted -Destination $script:Dest } |
            Should -Throw '*mazet.exe*'
        Test-Path -LiteralPath (Join-Path $script:Dest 'mazet.exe') | Should -BeFalse
    }
}
