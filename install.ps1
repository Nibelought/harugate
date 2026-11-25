# harugate installer for Windows (PowerShell 5.1 or newer).
#
#   irm https://raw.githubusercontent.com/Nibelought/harugate/main/install.ps1 | iex
#
# What it does, in order:
#   1. works out your system (Windows on x64 or ARM64);
#   2. downloads the matching .zip from the GitHub release (the latest, or the version in
#      HARUGATE_VERSION) and its SHA256SUMS.txt;
#   3. checks the .zip against the checksum and stops if it does not match;
#   4. copies harugate.exe to %LOCALAPPDATA%\Programs\harugate (or HARUGATE_INSTALL_DIR);
#   5. adds that folder to your user PATH (this user only; no administrator rights needed).
# It changes nothing else: no installer, no registry entries other than your user PATH,
# no background services.
#
# Options (environment variables, set before running):
#   HARUGATE_VERSION        e.g. 0.1.0 (default: the latest release)
#   HARUGATE_INSTALL_DIR    where to put harugate.exe
#   HARUGATE_DOWNLOAD_BASE  URL or local folder holding the release files (for mirrors
#                           and tests; default: the GitHub release page)
#
# Everything is inside one function and uses throw/return, never exit, so that running this
# through "| iex" can not close your PowerShell window.

function Install-Harugate {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue' # makes downloads much faster on PowerShell 5.1
    # PowerShell 5.1 does not use TLS 1.2 by default, which GitHub requires.
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $repo = 'Nibelought/harugate'

    # 1. Which archive do we need? (A 32-bit PowerShell on 64-bit Windows reports itself via
    # PROCESSOR_ARCHITEW6432.)
    $cpu = $env:PROCESSOR_ARCHITEW6432
    if (-not $cpu) { $cpu = $env:PROCESSOR_ARCHITECTURE }
    switch ($cpu) {
        'AMD64' { $platform = 'windows-x64' }
        'ARM64' { $platform = 'windows-arm64' }
        default { throw "Unsupported CPU '$cpu' (harugate is built for 64-bit Windows, x64 and ARM64)." }
    }

    # 2. Which version, and where from?
    $version = $env:HARUGATE_VERSION
    if ($version) { $version = $version.TrimStart('v') }
    if (-not $version) {
        Write-Host 'Looking up the latest release...'
        # /releases/latest redirects to /releases/tag/vX.Y.Z; read the address it ends up at.
        $request = [Net.HttpWebRequest]::Create("https://github.com/$repo/releases/latest")
        $request.Method = 'HEAD'
        $response = $request.GetResponse()
        $final = $response.ResponseUri.AbsoluteUri
        $response.Close()
        if ($final -notlike '*/releases/tag/*') {
            throw "No release has been published yet: see https://github.com/$repo/releases"
        }
        $version = $final.Substring($final.LastIndexOf('/') + 1).TrimStart('v')
    }

    $base = $env:HARUGATE_DOWNLOAD_BASE
    if (-not $base) { $base = "https://github.com/$repo/releases/download/v$version" }
    $installDir = $env:HARUGATE_INSTALL_DIR
    if (-not $installDir) { $installDir = Join-Path $env:LOCALAPPDATA 'Programs\harugate' }

    $name = "harugate-$version-$platform"
    $archive = "$name.zip"
    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("harugate-install-" + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tmp | Out-Null

    try {
        # Download from a URL, or copy from a local folder (HARUGATE_DOWNLOAD_BASE).
        function Get-ReleaseFile($file) {
            $target = Join-Path $tmp $file
            if (Test-Path -LiteralPath $base -PathType Container) {
                Copy-Item -LiteralPath (Join-Path $base $file) -Destination $target
            } else {
                try {
                    Invoke-WebRequest -UseBasicParsing -Uri "$base/$file" -OutFile $target
                } catch {
                    throw "Download failed: $base/$file ($($_.Exception.Message))"
                }
            }
            return $target
        }

        Write-Host "Downloading harugate $version for $platform..."
        $zip = Get-ReleaseFile $archive
        $sums = Get-ReleaseFile 'SHA256SUMS.txt'

        # 3. Verify.
        $line = Get-Content -LiteralPath $sums | Where-Object { $_ -match "^[0-9a-fA-F]{64}\s+\*?$([regex]::Escape($archive))\s*$" } | Select-Object -First 1
        if (-not $line) { throw "$archive is not listed in SHA256SUMS.txt." }
        $expected = ($line -split '\s+')[0].ToLowerInvariant()
        $actual = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($expected -ne $actual) {
            throw "Checksum mismatch for $archive (expected $expected, got $actual). Nothing was installed."
        }
        Write-Host 'Checksum OK.'

        # 4. Install. If harugate is running, Windows will not let us replace it.
        Expand-Archive -LiteralPath $zip -DestinationPath $tmp -Force
        $exe = Join-Path $tmp "$name\harugate.exe"
        if (-not (Test-Path -LiteralPath $exe)) { throw 'The archive does not contain harugate.exe.' }
        New-Item -ItemType Directory -Path $installDir -Force | Out-Null
        try {
            Copy-Item -LiteralPath $exe -Destination (Join-Path $installDir 'harugate.exe') -Force
        } catch {
            throw "Could not write harugate.exe to $installDir. If harugate is running, close it and run this again."
        }
        Unblock-File -LiteralPath (Join-Path $installDir 'harugate.exe')
    } finally {
        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }

    # 5. Add the folder to this user's PATH, if it is not there already.
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $entries = @()
    if ($userPath) { $entries = $userPath.Split(';') | Where-Object { $_ } }
    $trimmed = $installDir.TrimEnd('\')
    $alreadyThere = $entries | Where-Object { $_.TrimEnd('\') -ieq $trimmed }
    if (-not $alreadyThere) {
        $newPath = (@($entries) + $installDir) -join ';'
        [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
        $pathChanged = $true
    }
    # Make it work in this window too.
    if (-not (($env:Path.Split(';') | Where-Object { $_.TrimEnd('\') -ieq $trimmed }))) {
        $env:Path = "$env:Path;$installDir"
    }

    Write-Host ''
    Write-Host "Installed: $(Join-Path $installDir 'harugate.exe')"
    Write-Host ''
    if ($pathChanged) { Write-Host "Added $installDir to your PATH (new windows will see it)." }
    Write-Host 'Next: run  harugate   (or double-click harugate.exe in that folder).'
    Write-Host 'It opens the web UI in your browser; leave its window open while you use Koharu.'
    Write-Host 'If Windows SmartScreen warns about an unknown app: More info, then Run anyway.'
}

Install-Harugate
