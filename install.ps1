# Install the Incodex CLI onto PATH. This script never enables app integration.
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$Repository = if ($env:INCODEX_REPO) { $env:INCODEX_REPO } else { 'daftAI2026/incodex' }
$DownloadBase = if ($env:INCODEX_DOWNLOAD_BASE) {
    $env:INCODEX_DOWNLOAD_BASE.TrimEnd('/')
} else {
    "https://github.com/$Repository/releases/latest/download"
}
$AssetName = 'incodex-windows-x64.exe'
$Utf8NoBom = New-Object System.Text.UTF8Encoding($false)

function Stop-Installer([string]$Message) {
    [Console]::Error.WriteLine("incodex installer: $Message")
    exit 1
}

function Resolve-UserRoot {
    if ($env:INCODEX_USER_ROOT) {
        return [IO.Path]::GetFullPath($env:INCODEX_USER_ROOT)
    }
    $Profile = [Environment]::GetFolderPath([Environment+SpecialFolder]::UserProfile)
    if ([string]::IsNullOrWhiteSpace($Profile)) {
        Stop-Installer 'could not resolve the current user profile'
    }
    return [IO.Path]::Combine($Profile, '.incodex')
}

function Confirm-Architecture {
    $Architecture = if ($env:INCODEX_ARCH) {
        $env:INCODEX_ARCH
    } else {
        $env:PROCESSOR_ARCHITECTURE
    }
    switch ($Architecture.ToLowerInvariant()) {
        'amd64' { return }
        'x64' { return }
        'x86_64' { return }
        default { Stop-Installer "unsupported Windows architecture: $Architecture" }
    }
}

function Copy-ReleaseFile([string]$Name, [string]$Destination) {
    if ($env:INCODEX_DOWNLOAD_DIR) {
        $Source = Join-Path $env:INCODEX_DOWNLOAD_DIR $Name
        if (-not (Test-Path -LiteralPath $Source -PathType Leaf)) {
            Stop-Installer "missing $Name in $($env:INCODEX_DOWNLOAD_DIR)"
        }
        Copy-Item -LiteralPath $Source -Destination $Destination
        return
    }

    $Uri = "$DownloadBase/$Name"
    for ($Attempt = 1; $Attempt -le 3; $Attempt++) {
        try {
            Invoke-WebRequest -UseBasicParsing -Uri $Uri -OutFile $Destination
            return
        } catch {
            Remove-Item -LiteralPath $Destination -Force -ErrorAction SilentlyContinue
            if ($Attempt -eq 3) {
                Stop-Installer "failed to download $Name after 3 attempts: $($_.Exception.Message)"
            }
            Start-Sleep -Milliseconds 200
        }
    }
}

function Read-ExpectedChecksum([string]$ManifestPath, [string]$Name) {
    $Entries = @()
    foreach ($Line in [IO.File]::ReadAllLines($ManifestPath)) {
        if ($Line -match '^([0-9A-Fa-f]{64})\s+(.+)$' -and $Matches[2] -eq $Name) {
            $Entries += $Matches[1].ToLowerInvariant()
        }
    }
    if ($Entries.Count -ne 1) {
        Stop-Installer "SHA256SUMS must contain exactly one entry for $Name"
    }
    return $Entries[0]
}

function Read-CliVersion([string]$Executable) {
    $Output = & $Executable --version 2>&1
    if ($LASTEXITCODE -ne 0) {
        Stop-Installer "downloaded $AssetName is not runnable"
    }
    foreach ($Line in $Output) {
        if ($Line -match '^Incodex version ([0-9]+\.[0-9]+\.[0-9]+)$') {
            return $Matches[1]
        }
    }
    Stop-Installer "downloaded $AssetName did not report a stable Incodex version"
}

function Ensure-PrivateDirectory([string]$Path) {
    $Created = -not (Test-Path -LiteralPath $Path)
    $Directory = New-Item -ItemType Directory -Path $Path -Force
    if (-not $Created) {
        return $Directory.FullName
    }

    $Identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $Security = New-Object Security.AccessControl.DirectorySecurity
    $Security.SetAccessRuleProtection($true, $false)
    $Inheritance = [Security.AccessControl.InheritanceFlags]'ContainerInherit, ObjectInherit'
    $Propagation = [Security.AccessControl.PropagationFlags]::None
    $Allow = [Security.AccessControl.AccessControlType]::Allow
    $FullControl = [Security.AccessControl.FileSystemRights]::FullControl
    $Security.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule(
        $Identity.User, $FullControl, $Inheritance, $Propagation, $Allow
    )))
    $System = New-Object Security.Principal.SecurityIdentifier('S-1-5-18')
    $Security.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule(
        $System, $FullControl, $Inheritance, $Propagation, $Allow
    )))
    [IO.Directory]::SetAccessControl($Directory.FullName, $Security)
    return $Directory.FullName
}

function Write-AtomicText([string]$Path, [string]$Body) {
    $Parent = Split-Path -Parent $Path
    $Temporary = Join-Path $Parent ('.' + [IO.Path]::GetFileName($Path) + '.' + [guid]::NewGuid().ToString('N') + '.tmp')
    try {
        [IO.File]::WriteAllText($Temporary, $Body, $Utf8NoBom)
        Move-Item -LiteralPath $Temporary -Destination $Path -Force
    } finally {
        Remove-Item -LiteralPath $Temporary -Force -ErrorAction SilentlyContinue
    }
}

function Add-UserPath([string]$BinDirectory) {
    if ($env:INCODEX_SKIP_PATH) {
        return $false
    }
    $Current = [Environment]::GetEnvironmentVariable('Path', 'User')
    $Entries = @($Current -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    if ($Entries | Where-Object { $_.TrimEnd('\') -ieq $BinDirectory.TrimEnd('\') }) {
        return $false
    }
    $Updated = (@($Entries) + $BinDirectory) -join ';'
    [Environment]::SetEnvironmentVariable('Path', $Updated, 'User')
    return $true
}

Confirm-Architecture
$WorkRoot = Join-Path ([IO.Path]::GetTempPath()) ('incodex-setup-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $WorkRoot | Out-Null

try {
    $SumsPath = Join-Path $WorkRoot 'SHA256SUMS'
    $AssetPath = Join-Path $WorkRoot $AssetName
    Copy-ReleaseFile 'SHA256SUMS' $SumsPath
    Copy-ReleaseFile $AssetName $AssetPath

    $ExpectedHash = Read-ExpectedChecksum $SumsPath $AssetName
    $ActualHash = (Get-FileHash -LiteralPath $AssetPath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($ActualHash -ne $ExpectedHash) {
        Stop-Installer "checksum mismatch for $AssetName"
    }

    Unblock-File -LiteralPath $AssetPath -ErrorAction SilentlyContinue
    $Version = Read-CliVersion $AssetPath
    if ($env:INCODEX_EXPECTED_VERSION -and $Version -ne $env:INCODEX_EXPECTED_VERSION) {
        Stop-Installer "downloaded $AssetName reports $Version, expected $($env:INCODEX_EXPECTED_VERSION)"
    }

    $UserRoot = Resolve-UserRoot
    $PackageRoot = Join-Path $UserRoot 'packages\standalone'
    $ReleasesRoot = Join-Path $PackageRoot 'releases'
    $ReleaseRoot = Join-Path $ReleasesRoot $Version
    $InstalledCli = Join-Path $ReleaseRoot 'incodex.exe'
    $BinRoot = Join-Path $UserRoot 'bin'
    Ensure-PrivateDirectory $UserRoot | Out-Null
    Ensure-PrivateDirectory (Join-Path $UserRoot 'packages') | Out-Null
    Ensure-PrivateDirectory $PackageRoot | Out-Null
    Ensure-PrivateDirectory $ReleasesRoot | Out-Null
    Ensure-PrivateDirectory $BinRoot | Out-Null

    if (Test-Path -LiteralPath $ReleaseRoot) {
        if (-not (Test-Path -LiteralPath $InstalledCli -PathType Leaf)) {
            Stop-Installer "existing release is incomplete: $ReleaseRoot"
        }
        $InstalledHash = (Get-FileHash -LiteralPath $InstalledCli -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($InstalledHash -ne $ExpectedHash) {
            Stop-Installer "existing release does not match $AssetName"
        }
        if ((Read-CliVersion $InstalledCli) -ne $Version) {
            Stop-Installer "existing release failed its version proof"
        }
    } else {
        $Staging = Join-Path $ReleasesRoot ('.staging-' + [guid]::NewGuid().ToString('N'))
        try {
            Ensure-PrivateDirectory $Staging | Out-Null
            $StagedCli = Join-Path $Staging 'incodex.exe'
            Copy-Item -LiteralPath $AssetPath -Destination $StagedCli
            Unblock-File -LiteralPath $StagedCli -ErrorAction SilentlyContinue
            if ((Get-FileHash -LiteralPath $StagedCli -Algorithm SHA256).Hash.ToLowerInvariant() -ne $ExpectedHash) {
                Stop-Installer "staged checksum mismatch for $AssetName"
            }
            if ((Read-CliVersion $StagedCli) -ne $Version) {
                Stop-Installer "staged release failed its version proof"
            }
            Move-Item -LiteralPath $Staging -Destination $ReleaseRoot
        } finally {
            Remove-Item -LiteralPath $Staging -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    $PrimaryBody = @"
@echo off
setlocal
set "INCODEX_MANAGED_BY_STANDALONE=1"
for %%I in ("%~dp0..\packages\standalone") do set "INCODEX_MANAGED_PACKAGE_ROOT=%%~fI"
"%~dp0..\packages\standalone\releases\$Version\incodex.exe" %*
exit /b %ERRORLEVEL%
"@
    $AliasBody = @"
@echo off
"%~dp0incodex.cmd" %*
"@
    Write-AtomicText (Join-Path $BinRoot 'incodex.cmd') $PrimaryBody
    Write-AtomicText (Join-Path $BinRoot 'inc.cmd') $AliasBody
    $PathAdded = Add-UserPath $BinRoot

    [Console]::OutputEncoding = [Text.Encoding]::UTF8
    [Console]::WriteLine("Installed Incodex $Version to $InstalledCli")
    if ($PathAdded) {
        [Console]::WriteLine('Open a new terminal, then run: incodex --help')
    } else {
        [Console]::WriteLine('Run: incodex --help')
    }
} finally {
    Remove-Item -LiteralPath $WorkRoot -Recurse -Force -ErrorAction SilentlyContinue
}
