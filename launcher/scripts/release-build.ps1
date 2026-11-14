# Release build of the launcher with no trace of the build machine in the executable.
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\release-build.ps1 [arguments of `tauri build`...]
#   npm run tauri:release -- [arguments of `tauri build`...]
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\release-build.ps1 -CheckOnly <exe>   (the search alone)
#
# Rust writes the source path of every panic location into the program. The dependencies come from Cargo's registry
# under the user's profile, so a plain release build carries "C:\Users\<name>\.cargo\registry\src\..." a few hundred
# times. --remap-path-prefix rewrites the user's profile, this repository, the rustup home and Cargo's home (the most
# specific last: rustc applies the last matching prefix) to neutral names; Cargo's `trim-paths` would do the same but is
# not stable (Cargo 1.97). The flags reach tauri build, cargo and every crate through CARGO_ENCODED_RUSTFLAGS (paths with
# spaces survive). They change the compiler's arguments, so the first build with them recompiles every crate once.
#
# Afterwards the executable is searched for the user's name, "Users\<name>" and this repository's path, in ASCII and in
# UTF-16LE: any hit fails the script (exit code 3), so a release that would leak them is not used by mistake.
param([string]$CheckOnly = '', [Parameter(ValueFromRemainingArguments = $true)] [string[]]$TauriArgs)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
$rustupHome = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $env:USERPROFILE '.rustup' }
$maps = @(
    @($env:USERPROFILE, '/home'),
    @($repo, '/kalamar'),
    @($rustupHome, '/rustup'),
    @($cargoHome, '/cargo')
)
if ($CheckOnly) {
    $exe = $CheckOnly
} else {
    $flags = @()
    foreach ($m in $maps) {
        if ($m[0]) { $flags += "--remap-path-prefix=$($m[0])=$($m[1])" }
    }
    # Keep flags someone already gives (they come first; ours win on the same prefix).
    $given = if ($env:CARGO_ENCODED_RUSTFLAGS) { $env:CARGO_ENCODED_RUSTFLAGS -split [char]0x1f } elseif ($env:RUSTFLAGS) { $env:RUSTFLAGS -split '\s+' | Where-Object { $_ } } else { @() }
    $env:CARGO_ENCODED_RUSTFLAGS = (@($given) + $flags) -join [char]0x1f
    Write-Host "release-build: remapping $($maps.Count) path prefixes (user profile, repository, rustup, Cargo)"

    Push-Location $repo
    try {
        & npx tauri build @TauriArgs
        if ($LASTEXITCODE -ne 0) { throw "release-build: tauri build failed (exit $LASTEXITCODE)" }
    } finally { Pop-Location }

    $targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $repo 'src-tauri\target' }
    $exe = Join-Path $targetDir 'release\kalamar.exe'
}
if (-not (Test-Path $exe)) { throw "release-build: $exe not found" }

# The search: bytes read as Latin-1 (one char per byte), UTF-16LE strings as their bytes interleaved with NULs.
$text = [Text.Encoding]::GetEncoding(28591).GetString([IO.File]::ReadAllBytes($exe))
function Wide([string]$s) { ($s.ToCharArray() | ForEach-Object { [regex]::Escape([string]$_) + '\x00' }) -join '' }
# The update repository is compiled in on purpose (KALAMAR_UPDATE_REPO, owner/name). Its owner may be named like the
# Windows user: those occurrences are the public repository's address, not a trace of this machine.
if ($env:KALAMAR_UPDATE_REPO -match '^[A-Za-z0-9-]{1,39}/[A-Za-z0-9._-]{1,100}$') {
    $text = $text.Replace($env:KALAMAR_UPDATE_REPO, '<update repository>')
}
$patterns = @()
if ($env:USERNAME -and $env:USERNAME.Length -ge 3) { $patterns += [regex]::Escape($env:USERNAME); $patterns += (Wide $env:USERNAME) }
$patterns += '(?i)users[\\/][a-z0-9]'
$patterns += '(?i)u\x00s\x00e\x00r\x00s\x00[\\/]\x00[a-z0-9]\x00'
foreach ($p in @($repo, $repo.Replace('\', '/'))) { $patterns += [regex]::Escape($p); $patterns += (Wide $p) }
$names = @('Users\<name>', 'Users\<name> (UTF-16)', 'repository path', 'repository path (UTF-16)', 'repository path (/)', 'repository path (/, UTF-16)')
if ($env:USERNAME -and $env:USERNAME.Length -ge 3) { $names = @('user name', 'user name (UTF-16)') + $names }
$hits = 0
for ($i = 0; $i -lt $patterns.Count; $i++) {
    # The pattern itself is not printed: it may hold the user's name.
    $n = ([regex]::Matches($text, $patterns[$i])).Count
    if ($n -gt 0) { Write-Host "release-build: $n x $($names[$i])"; $hits += $n }
}
if ($hits -gt 0) {
    Write-Host "release-build: $exe carries $hits trace(s) of this machine: do not ship it"
    exit 3
}
Write-Host "release-build: $exe has no trace of this machine (user name, profile path, repository path)"
