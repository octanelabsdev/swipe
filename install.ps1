# Installs swipe on Windows:
#   irm https://raw.githubusercontent.com/octanelabsdev/swipe/main/install.ps1 | iex
# SWIPE_VERSION picks a release tag (default: the latest). SWIPE_INSTALL_DIR picks the folder
# (default: %LOCALAPPDATA%\Programs\swipe).

# `iex` runs this in the caller's session, so keep every setting and variable inside this block.
& {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    # Windows 11 on ARM runs the x64 build through emulation; Windows 10 on ARM can't.
    if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64' -and [Environment]::OSVersion.Version.Build -lt 22000) {
        throw 'swipe: Windows 10 on ARM is not supported; Windows 11 on ARM is'
    }

    $repo = 'octanelabsdev/swipe'
    $dir = if ($env:SWIPE_INSTALL_DIR) { $env:SWIPE_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\swipe' }
    $version = if ($env:SWIPE_VERSION) { $env:SWIPE_VERSION } else { 'latest' }
    $asset = 'swipe-x86_64-pc-windows-msvc.zip'
    $url = if ($version -eq 'latest') { "https://github.com/$repo/releases/latest/download" } else { "https://github.com/$repo/releases/download/$version" }
    $exe = Join-Path $dir 'swipe.exe'

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid())
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        Write-Host "swipe: downloading $asset ($version)"
        Invoke-WebRequest -UseBasicParsing -Uri "$url/$asset" -OutFile (Join-Path $tmp $asset)
        Invoke-WebRequest -UseBasicParsing -Uri "$url/$asset.sha256" -OutFile (Join-Path $tmp "$asset.sha256")

        $expected = ((Get-Content (Join-Path $tmp "$asset.sha256") -Raw).Trim() -split '\s+')[0]
        $actual = (Get-FileHash (Join-Path $tmp $asset) -Algorithm SHA256).Hash
        if ($expected -ne $actual) { throw 'swipe: the checksum does not match, so nothing was installed' }

        Expand-Archive -Path (Join-Path $tmp $asset) -DestinationPath (Join-Path $tmp 'out')
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
        # A running exe can't be overwritten, but it can be renamed; old ones go once they stop.
        Get-ChildItem $dir -Filter 'swipe.exe.*.old' | Remove-Item -Force -ErrorAction SilentlyContinue
        $old = "$exe.$([Guid]::NewGuid().ToString('N')).old"
        if (Test-Path $exe) { Move-Item $exe $old }
        try {
            Copy-Item (Join-Path $tmp 'out\swipe.exe') $exe
        }
        catch {
            if (Test-Path $old) { Move-Item $old $exe -Force }
            throw
        }
    }
    finally {
        Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    }

    $installed = & $exe --version
    Write-Host "swipe: installed $installed to $exe"

    # Read and write the raw registry value, so entries like %USERPROFILE%\... stay unexpanded.
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
    try {
        $userPath = $key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        if (-not (($userPath -split ';') -contains $dir)) {
            $newPath = if ($userPath) { "$userPath;$dir" } else { $dir }
            $key.SetValue('Path', $newPath, [Microsoft.Win32.RegistryValueKind]::ExpandString)
            # Any .NET user-variable write broadcasts the change, so new terminals see the PATH.
            [Environment]::SetEnvironmentVariable('SWIPE_INSTALL_REFRESH', $null, 'User')
            $env:Path = "$env:Path;$dir"
            Write-Host "swipe: added $dir to your PATH; open a new terminal to use it everywhere"
        }
    }
    finally {
        $key.Close()
    }
    Write-Host 'swipe: next, add the shell hook: https://swipe.octanelabs.dev/install/#2-add-the-shell-hook'
}
