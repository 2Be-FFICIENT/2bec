# Installs the 2bec command line client of the 2Be-FFICIENT API on Windows.
#
#   irm https://api.2befficient.com/cli/install.ps1 | iex
#
# It reads the latest version from the release feed, downloads the archive of this machine
# (x64 or arm64) and its SHA256SUMS, checks the archive against them, puts 2bec.exe in a
# directory of the user's PATH and adds that directory to the PATH when it is not there yet.
# Windows PowerShell 5.1 and PowerShell 7.
#
# Environment:
#   TWOBEC_VERSION       a version to install (0.1.0) instead of the latest
#   TWOBEC_INSTALL_DIR   where the binary goes (default: %LOCALAPPDATA%\Programs\2bec)
#   TWOBEC_INSTALL_URL   the release feed (default: https://api.2befficient.com/cli)
#
# This file is served by the API from the copy vendored in the pegas repository; this
# repository is its source of truth.

function Install-TwoBec {
    [CmdletBinding()]
    param()

    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'
    $feed = if ($env:TWOBEC_INSTALL_URL) { $env:TWOBEC_INSTALL_URL.TrimEnd('/') } else { 'https://api.2befficient.com/cli' }
    $releasesPage = 'https://github.com/2Be-FFICIENT/2bec/releases'

    # TLS 1.2 for Windows PowerShell 5.1, whose default may be older.
    try { [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12 } catch { }

    # The architecture, as the release archives name it (a 32-bit shell on a 64-bit system reports the system's).
    $machine = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
    $arch = switch ($machine) {
        'AMD64' { 'x64' }
        'ARM64' { 'arm64' }
        default { throw "install.ps1: unsupported architecture '$machine'" }
    }
    $platform = "windows-$arch"

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("2bec-install-" + [IO.Path]::GetRandomFileName())
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        # The version: pinned, else the feed's latest.
        $version = $env:TWOBEC_VERSION
        if (-not $version) {
            try {
                $version = (Invoke-RestMethod -Uri "$feed/releases/latest" -UseBasicParsing).version
            } catch {
                throw "install.ps1: the release feed did not answer ($feed/releases/latest); the archives are at $releasesPage. $($_.Exception.Message)"
            }
        }
        $version = "$version".TrimStart('v')
        if ($version -notmatch '^\d+\.\d+\.\d+$') { throw "install.ps1: '$version' is not a version (expected 0.1.0)" }

        $name = "2bec-$version-$platform.zip"
        Write-Host "Installing 2bec $version for $platform"

        $archive = Join-Path $tmp $name
        $sums = Join-Path $tmp 'SHA256SUMS'
        try {
            Invoke-WebRequest -Uri "$feed/releases/$version/$name" -OutFile $archive -UseBasicParsing
            Invoke-WebRequest -Uri "$feed/releases/$version/SHA256SUMS" -OutFile $sums -UseBasicParsing
        } catch {
            throw "install.ps1: could not download $name from $feed/releases/$version/; the archives are at $releasesPage. $($_.Exception.Message)"
        }

        # The checksum: the line of our archive in SHA256SUMS (hash, two spaces, name).
        $line = Get-Content $sums | Where-Object { $_ -match ("\s" + [regex]::Escape($name) + '$') } | Select-Object -First 1
        if (-not $line) { throw "install.ps1: SHA256SUMS does not list $name" }
        $expected = ($line -split '\s+')[0].ToLowerInvariant()
        $actual = (Get-FileHash -Path $archive -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($expected -ne $actual) {
            throw "install.ps1: checksum mismatch for $name (expected $expected, got $actual): the download is corrupt or tampered"
        }

        Expand-Archive -Path $archive -DestinationPath $tmp -Force
        $binary = Join-Path $tmp "2bec-$version-$platform\2bec.exe"
        if (-not (Test-Path $binary)) { throw 'install.ps1: the archive holds no 2bec.exe' }

        $dir = if ($env:TWOBEC_INSTALL_DIR) { $env:TWOBEC_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\2bec' }
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
        $target = Join-Path $dir '2bec.exe'
        # A running 2bec.exe cannot be overwritten, but can be renamed away.
        if (Test-Path $target) { Move-Item -Path $target -Destination "$target.old" -Force }
        Copy-Item -Path $binary -Destination $target -Force
        Remove-Item -Path "$target.old" -Force -ErrorAction SilentlyContinue

        # The user's PATH, for the next shells; this one too.
        $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
        $onPath = ($userPath -split ';' | Where-Object { $_ } | ForEach-Object { $_.TrimEnd('\') }) -contains $dir.TrimEnd('\')
        if (-not $onPath) {
            $newPath = if ($userPath) { "$userPath;$dir" } else { $dir }
            [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
            Write-Host "Added $dir to your PATH (open a new terminal for it to apply)"
        }
        if (($env:Path -split ';') -notcontains $dir) { $env:Path = "$env:Path;$dir" }

        $installed = try { & $target --version 2>$null } catch { "2bec $version" }
        Write-Host "Installed $target ($installed)"
        Write-Host 'Shell completions: 2bec completion powershell. Next: 2bec auth login'
    } finally {
        Remove-Item -Path $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Install-TwoBec
