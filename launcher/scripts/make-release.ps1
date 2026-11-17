# Builds the folder of one launcher release, ready to be uploaded to GitHub Releases by hand: the portable Kalamar.exe,
# the NSIS installer, the signed manifest (kalamar-release.json + .minisig), the release notes and SHA256SUMS.
# Nothing is uploaded, pushed, tagged or published. Step by step, keys included: docs\releasing.md.
#
#   powershell -ExecutionPolicy Bypass -File scripts\make-release.ps1 -Repo owner/kalamar -Version 0.2.0 `
#       -Notes D:\notes\0.2.0.txt -Key E:\kalamar-keys\kalamar.key -Minisign E:\tools\minisign.exe `
#       [-Channel beta] [-Out <folder>] [-SkipBuild] [-AllowDirty]
#
# -Repo      the GitHub repository whose releases the launcher will check (compiled in: KALAMAR_UPDATE_REPO)
# -Version   must equal the version of src-tauri\Cargo.toml, src-tauri\tauri.conf.json and package.json
# -Notes     a plain-text file (shown as text by the launcher, never as HTML or Markdown)
# -Key       the minisign secret key (encrypted; its password is asked for while signing); refused inside a git tree
# -Minisign  the official minisign command-line tool (0.10 or later), outside any git tree: it alone reads the secret
#            key. No program built from this repository (and its hundreds of dependencies) ever decrypts it.
# -Channel   stable (default) or beta (a pre-release version such as 0.3.0-beta.1 must be beta)
# -Out       the release folder (default: release\<version> beside src-tauri, ignored by git); must not exist yet
# -SkipBuild uses the build already in the target folder (it must have been made with the same -Repo)
# -DiscordClientId  the application id of the Discord application (compiled in: KALAMAR_DISCORD_CLIENT_ID; the opt-in
#            Discord presence, docs\discord-presence.md); left out, the id of src-tauri\discord-app-id.txt; `none` for a
#            release without the Discord feature
param(
    [Parameter(Mandatory = $true)][string]$Repo,
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$Notes,
    [Parameter(Mandatory = $true)][string]$Key,
    [Parameter(Mandatory = $true)][string]$Minisign,
    [ValidateSet('stable', 'beta')][string]$Channel = 'stable',
    [string]$Out = '',
    [string]$DiscordClientId = '',
    [switch]$SkipBuild,
    [switch]$AllowDirty
)
$ErrorActionPreference = 'Stop'
$Launcher = Split-Path $PSScriptRoot -Parent
$Tauri = Join-Path $Launcher 'src-tauri'

function Fail([string]$m) {
    Write-Host "error: $m" -ForegroundColor Red
    exit 1
}

function Step([string]$m) {
    Write-Host "== $m" -ForegroundColor Cyan
}

# ---- checks, before anything is built -------------------------------------------------------------------------------
Step 'checks'
if ($Repo -notmatch '^[A-Za-z0-9-]{1,39}/[A-Za-z0-9._-]{1,100}$') { Fail '-Repo must be owner/repo' }
if ($Version -notmatch '^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$') { Fail '-Version must be like 0.2.0 or 0.3.0-beta.1 (no v, no build metadata)' }
if ($Channel -eq 'stable' -and $Version.Contains('-')) { Fail "$Version is a pre-release version: use -Channel beta" }
if ($DiscordClientId -eq 'none') {
    $DiscordClientId = ''
} elseif (-not $DiscordClientId) {
    $idFile = Join-Path $Tauri 'discord-app-id.txt'
    if (Test-Path -LiteralPath $idFile) { $DiscordClientId = [string](Get-Content -LiteralPath $idFile | Where-Object { $_ -match '^\d{17,20}$' } | Select-Object -First 1) }
} elseif ($DiscordClientId -notmatch '^\d{17,20}$') { Fail '-DiscordClientId must be the application id (17 to 20 digits) or none' }

$cargoVersion = (Select-String -Path (Join-Path $Tauri 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$tauriVersion = (Get-Content (Join-Path $Tauri 'tauri.conf.json') -Raw | ConvertFrom-Json).version
$npmVersion = (Get-Content (Join-Path $Launcher 'package.json') -Raw | ConvertFrom-Json).version
foreach ($pair in @(@('src-tauri\Cargo.toml', $cargoVersion), @('src-tauri\tauri.conf.json', $tauriVersion), @('package.json', $npmVersion))) {
    if ($pair[1] -ne $Version) { Fail "$($pair[0]) says $($pair[1]), not ${Version}: set the same version in the three files and commit" }
}

if (-not (Test-Path -LiteralPath $Notes -PathType Leaf)) { Fail "notes file not found: $Notes" }
$NotesFull = (Resolve-Path -LiteralPath $Notes).Path
if (-not (Test-Path -LiteralPath $Key -PathType Leaf)) { Fail "secret key not found: $Key" }
$KeyFull = (Resolve-Path -LiteralPath $Key).Path
$d = Split-Path $KeyFull -Parent
while ($d) {
    if (Test-Path (Join-Path $d '.git')) { Fail "the secret key is inside the git working tree ${d}: keep it out of every repository (docs\releasing.md)" }
    $d = Split-Path $d -Parent
}
if (-not (Test-Path -LiteralPath $Minisign -PathType Leaf)) { Fail "minisign not found: $Minisign (the official tool, docs\releasing.md)" }
$MinisignFull = (Resolve-Path -LiteralPath $Minisign).Path
$d = Split-Path $MinisignFull -Parent
while ($d) {
    if (Test-Path (Join-Path $d '.git')) { Fail "minisign is inside the git working tree ${d}: use the official tool from its own folder" }
    $d = Split-Path $d -Parent
}

$Keys = @(Get-Content (Join-Path $Tauri 'update-keys.txt') | ForEach-Object { $_.Trim() } | Where-Object { $_ -and -not $_.StartsWith('#') })
if ($Keys.Count -eq 0) { Fail 'src-tauri\update-keys.txt holds no public key: add yours first (docs\releasing.md)' }

$dirty = git -C $Launcher status --porcelain
if ($LASTEXITCODE -ne 0) { Fail 'git status failed' }
if ($dirty -and -not $AllowDirty) { Fail 'the working tree has changes: commit them first (a release is one commit), or pass -AllowDirty for a trial run' }
$commit = (git -C $Launcher rev-parse HEAD).Trim()

if (-not $Out) { $Out = Join-Path $Launcher "release\$Version" }
if ((Test-Path -LiteralPath $Out) -and (Get-ChildItem -LiteralPath $Out -Force | Select-Object -First 1)) { Fail "$Out already exists and is not empty" }
$Target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Tauri 'target' }

# ---- the build, with the update repository compiled in ------------------------------------------------------------------
$env:KALAMAR_UPDATE_REPO = $Repo
# Always set: a value left in the shell never slips into a release (`none`: scripts\release-build.ps1 builds without).
$env:KALAMAR_DISCORD_CLIENT_ID = if ($DiscordClientId) { $DiscordClientId } else { 'none' }
if (-not $SkipBuild) {
    Step "npm run tauri:release (KALAMAR_UPDATE_REPO=$Repo; Discord application: $(if ($DiscordClientId) { $DiscordClientId } else { 'none' }); the paths of this machine remapped)"
    Push-Location $Launcher
    try {
        npm run tauri:release
        if ($LASTEXITCODE -ne 0) { Fail 'the build failed, or the program carries a trace of this machine' }
    } finally { Pop-Location }
}
$Exe = Join-Path $Target 'release\kalamar.exe'
$SetupName = "Kalamar_${Version}_x64-setup.exe"
$Setup = Join-Path $Target "release\bundle\nsis\$SetupName"
if (-not (Test-Path -LiteralPath $Exe)) { Fail "no build at $Exe" }
if (-not (Test-Path -LiteralPath $Setup)) { Fail "no installer at $Setup" }

# A build made without KALAMAR_UPDATE_REPO has no updater at all: such a release would strand its players.
Step 'the program carries the repository and the public keys'
$text = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($Exe))
if (-not $text.Contains($Repo)) { Fail "$Exe was not built with KALAMAR_UPDATE_REPO=$Repo (run without -SkipBuild)" }
foreach ($k in $Keys) { if (-not $text.Contains($k)) { Fail "$Exe does not carry the public key $k (rebuild)" } }
if ($DiscordClientId -and -not $text.Contains($DiscordClientId)) { Fail "$Exe was not built with KALAMAR_DISCORD_CLIENT_ID=$DiscordClientId (run without -SkipBuild, or with -DiscordClientId none for a release without the Discord presence)" }
Write-Host "Discord application: $(if ($DiscordClientId) { $DiscordClientId } else { 'none' })"
$text = $null

# Nothing of the machine that built it (a build made without scripts\release-build.ps1 carries the user's profile path in
# every panic location of its dependencies).
Step 'the program carries no trace of this machine'
powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'release-build.ps1') -CheckOnly $Exe
if ($LASTEXITCODE -ne 0) { Fail "$Exe carries a trace of this machine (build it with npm run tauri:release)" }

# ---- the release folder --------------------------------------------------------------------------------------------------
Step "release folder $Out"
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$OutFull = (Resolve-Path -LiteralPath $Out).Path
Copy-Item -LiteralPath $Exe -Destination (Join-Path $OutFull 'Kalamar.exe')
Copy-Item -LiteralPath $Setup -Destination (Join-Path $OutFull $SetupName)
Copy-Item -LiteralPath $NotesFull -Destination (Join-Path $OutFull 'RELEASE-NOTES.txt')

Push-Location $Tauri
try {
    Step 'manifest'
    cargo run --release --quiet --example release_sign -- manifest --repo $Repo --version $Version --channel $Channel --notes $NotesFull `
        --portable (Join-Path $OutFull 'Kalamar.exe') --installer (Join-Path $OutFull $SetupName) --out $OutFull
    if ($LASTEXITCODE -ne 0) { Fail 'the manifest could not be written' }
    # Signed by the official minisign tool: the secret key is decrypted by it alone, never by a program built from this
    # tree (a compromised build dependency could otherwise read it). Prehashed (minisign's default), as the launcher
    # requires.
    Step 'signature by minisign (the secret key password is asked for by minisign; nothing is echoed or stored)'
    $manifest = Join-Path $OutFull 'kalamar-release.json'
    $trusted = "timestamp:$([DateTimeOffset]::UtcNow.ToUnixTimeSeconds())`tfile:kalamar-release.json`thashed"
    & $MinisignFull -S -s $KeyFull -m $manifest -t $trusted
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath "$manifest.minisig")) { Fail 'signing failed' }
    Step 'the check the launcher makes (signature against update-keys.txt, sizes, SHA-256)'
    cargo run --release --quiet --example release_sign -- verify --dir $OutFull --repo $Repo
    if ($LASTEXITCODE -ne 0) { Fail 'the release does not pass the launcher''s own check' }
} finally { Pop-Location }

Step 'SHA256SUMS'
$lines = Get-ChildItem -LiteralPath $OutFull -File | Where-Object { $_.Name -ne 'SHA256SUMS' } | Sort-Object Name |
    ForEach-Object { '{0}  {1}' -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant(), $_.Name }
# LF line ends and no byte-order mark: `sha256sum -c SHA256SUMS` reads it as is
[IO.File]::WriteAllText((Join-Path $OutFull 'SHA256SUMS'), (($lines -join "`n") + "`n"))

Write-Host ''
Write-Host "Release $Version ($Channel) of $Repo, from commit $commit, ready in:" -ForegroundColor Green
Write-Host "  $OutFull"
Get-ChildItem -LiteralPath $OutFull -File | Sort-Object Name | ForEach-Object { Write-Host ("  {0,12:N0}  {1}" -f $_.Length, $_.Name) }
Write-Host ''
Write-Host 'Nothing was uploaded. To publish (docs\releasing.md, "Publishing a release"):'
Write-Host "  1. tag the commit:   git tag v$Version $commit ; git push origin v$Version"
Write-Host "  2. on GitHub, draft a release for the tag v$Version, title 'Kalamar $Version', the notes as its text"
Write-Host '  3. attach ALL the files of the folder above (the launcher needs kalamar-release.json and its .minisig)'
if ($Channel -eq 'beta') { Write-Host '  4. tick "Set as a pre-release", then publish' } else { Write-Host '  4. leave "Set as a pre-release" unticked, tick "Set as the latest release", then publish' }
