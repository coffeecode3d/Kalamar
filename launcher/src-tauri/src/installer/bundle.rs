//! The runtime source the install wizard compiles, shipped with the launcher as one archive
//! (`resources/runtime-src.tar.gz` + `runtime-src.json`): a `git archive` of the engine's sources at one state, plus
//! the helper scripts under `kit-tools/`. It holds code only. [`scan_entry`] is the "nothing derived from the
//! game" check: the bundling tool (`examples/bundle_runtime.rs`) refuses to write an archive that fails it, and a test
//! scans the archive that is actually shipped.

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const ARCHIVE_NAME: &str = "runtime-src.tar.gz";
pub const MANIFEST_NAME: &str = "runtime-src.json";
/// Written into the extracted source folder: which bundle it came from.
pub const MARKER_NAME: &str = "KALAMAR-SOURCE.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Manifest {
    /// Identifier of the engine snapshot the archive was made from: the git tree id of the engine's sources, the same
    /// in every repository that holds these exact sources (so it names no commit of any history). Compared as an
    /// opaque text; the field keeps the name install records and markers already use.
    pub commit: String,
    pub files: usize,
    pub bytes: u64,
    /// SHA-256 of the archive file.
    pub sha256: String,
    /// Helper scripts added under `kit-tools/` (path, SHA-256).
    pub kit_tools: Vec<(String, String)>,
    /// Image and font files the scan allowed (icons reviewed by hand, open fonts by family), with their SHA-256.
    pub reviewed_binaries: Vec<(String, String)>,
}

/// Switch containers, executables, game data formats, keys, saves and caches (a superset of the runtime repository's
/// own `tools/check_content.py`).
const BLOCKED_SUFFIXES: &[&str] = &[
    ".xci", ".xcz", ".nsp", ".nsz", ".nca", ".ncz", ".tik", ".cert", ".nso", ".nro", ".nrr", ".npdm", ".keys", ".bfres",
    ".bntx", ".bnsh", ".sarc", ".szs", ".zs", ".pack", ".byml", ".bgyml", ".msbt", ".bars", ".bwav", ".bfwav", ".bfstm",
    ".bfttf", ".bfotf", ".sharcb", ".webm", ".bin", ".dat", ".sav", ".cnmt", ".istorage", ".romfs", ".exefs", ".proto",
    ".pb", ".spv", ".dxbc", ".pcache", ".mp4",
];
/// ExeFS module names and files the pipeline derives from the player's game.
const BLOCKED_NAMES: &[&str] = &[
    "main", "sdk", "rtld", "subsdk0", "subsdk1", "subsdk2", "main.npdm", "prod.keys", "title.keys", "dev.keys",
    "s3save.keys.json", "romfs.bin", "extraction-manifest.json", "merge_romfs.log", "layout.json", "target.json",
    "pipeline-cache.bin", "save.dat", "romfs-index.bin",
];
/// Folders that only ever hold extracted or generated game data.
const BLOCKED_DIRS: &[&str] =
    &["exefs", "romfs", "lift", "generated", "userdata", "shader-cache", "save", "game-cache", "screens", "build"];
/// Images and fonts are refused unless listed here by path; each was looked at (no game imagery).
const REVIEWED_BINARIES: &[&str] = &["runtime/totk.ico", "runtime/kalamar.ico", "runtime/ui/fonts/Roboto-Medium.ttf"];
const IMAGE_SUFFIXES: &[&str] =
    &[".png", ".jpg", ".jpeg", ".bmp", ".gif", ".webp", ".tga", ".dds", ".ico", ".astc", ".ktx", ".ttf", ".otf"];
/// Open-license font families the runtime's interface ships (SIL OFL / Apache 2.0).
const FREE_FONT_FAMILIES: &[&str] = &["Roboto", "JetBrainsMono", "JetBrains Mono", "NotoSans", "Noto Sans", "Inter"];

/// A `.ttf`/`.otf` under `runtime/ui/fonts/` whose file name starts with a known free family and whose name table
/// carries that family's name (ASCII or UTF-16BE): an open font, not one taken from a game.
pub fn free_font(path: &str, data: &[u8]) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let suffix = lower_suffix(name);
    if !path.starts_with("runtime/ui/fonts/") || !(suffix == ".ttf" || suffix == ".otf") {
        return false;
    }
    FREE_FONT_FAMILIES.iter().any(|fam| {
        let compact: String = fam.chars().filter(|c| *c != ' ').collect();
        if !name.starts_with(&compact) {
            return false;
        }
        let ascii = fam.as_bytes();
        let utf16: Vec<u8> = fam.encode_utf16().flat_map(|u| u.to_be_bytes()).collect();
        data.windows(ascii.len()).any(|w| w == ascii) || data.windows(utf16.len()).any(|w| w == utf16.as_slice())
    })
}

pub const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// Written by the runtime's code generator into every file it emits (`tools/check_content.py`).
pub const GENERATED_MARKER: &str = concat!("TOTK_RECOMP_", "GENERATED_FROM_GAME_CODE");

fn lower_suffix(name: &str) -> String {
    name.rfind('.').map(|i| name[i..].to_ascii_lowercase()).unwrap_or_default()
}

/// A `prod.keys` / `title.keys` line: a name containing key, kek or seed, `=`, then 32 or more hex digits.
pub fn has_key_line(text: &str) -> bool {
    text.lines().any(|l| {
        let Some((name, value)) = l.split_once('=') else { return false };
        let (name, value) = (name.trim().to_ascii_lowercase(), value.trim());
        !name.is_empty()
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && (name.contains("key") || name.contains("kek") || name.contains("seed"))
            && value.len() >= 32
            && value.chars().all(|c| c.is_ascii_hexdigit())
    })
}

/// Problems of one archive entry (path relative to the archive root, `/`-separated), empty when it is clean.
pub fn scan_entry(path: &str, data: &[u8]) -> Vec<String> {
    let mut found = Vec::new();
    let name = path.rsplit('/').next().unwrap_or(path);
    let lname = name.to_ascii_lowercase();
    let suffix = lower_suffix(&lname);
    if BLOCKED_SUFFIXES.contains(&suffix.as_str()) {
        found.push(format!("blocked file type {suffix}"));
    }
    if BLOCKED_NAMES.contains(&lname.as_str()) || lname.starts_with("subsdk") {
        found.push("name of a game module or of a file derived from the game".into());
    }
    let dirs: Vec<&str> = path.split('/').collect();
    if let Some(d) = dirs[..dirs.len().saturating_sub(1)].iter().find(|d| BLOCKED_DIRS.contains(&d.to_ascii_lowercase().as_str())) {
        found.push(format!("inside a '{d}' folder"));
    }
    if IMAGE_SUFFIXES.contains(&suffix.as_str()) && !REVIEWED_BINARIES.contains(&path) && !free_font(path, data) {
        found.push("image or font that was not reviewed".into());
    }
    // Data files named like a catalogue or a cache (source code about them, e.g. tools/pack_shader_cache.py, is fine).
    let code = [".py", ".c", ".h", ".cpp", ".hpp", ".cs", ".rs", ".sh", ".ps1", ".md"].contains(&suffix.as_str());
    if !code && (lname.contains("catalogue") || lname.contains("shader_cache") || lname.contains("shadercache") || lname.contains("pipeline-cache")) {
        found.push("catalogue or cache file".into());
    }
    if data.len() as u64 > MAX_FILE_BYTES {
        found.push(format!("{} KiB, over the {} KiB limit", data.len() / 1024, MAX_FILE_BYTES / 1024));
    }
    if data.windows(GENERATED_MARKER.len()).any(|w| w == GENERATED_MARKER.as_bytes()) {
        found.push("generated from game code".into());
    }
    if has_key_line(&String::from_utf8_lossy(data)) {
        found.push("contains a key in prod.keys format".into());
    }
    found
}

/// Whether an archive entry path is a safe relative path (no `..`, no root, no drive). Both separators are read as
/// separators whatever the host: an archive is data, not this machine's path syntax.
pub fn safe_relative(path: &str) -> Option<PathBuf> {
    if path.contains('\0') {
        return None;
    }
    let normalized = path.replace('\\', "/");
    let bytes = normalized.as_bytes();
    let drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if drive || normalized.starts_with('/') || normalized.starts_with("//") {
        return None;
    }
    let mut out = PathBuf::new();
    for part in normalized.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            s => out.push(s),
        }
    }
    if out.as_os_str().is_empty() { None } else { Some(out) }
}

/// Every regular file of a `.tar.gz`, as (path, bytes).
pub fn read_archive(gz: &[u8]) -> std::io::Result<Vec<(String, Vec<u8>)>> {
    let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(gz));
    let mut out = Vec::new();
    for e in ar.entries()? {
        let mut e = e?;
        if !e.header().entry_type().is_file() {
            continue;
        }
        let path = e.path()?.to_string_lossy().replace('\\', "/");
        let mut data = Vec::with_capacity(e.size() as usize);
        e.read_to_end(&mut data)?;
        out.push((path, data));
    }
    Ok(out)
}

/// Scans a whole archive; returns (files scanned, problems as "path: reason").
pub fn scan_archive(gz: &[u8]) -> std::io::Result<(usize, Vec<String>)> {
    let files = read_archive(gz)?;
    let mut problems = Vec::new();
    for (p, d) in &files {
        for why in scan_entry(p, d) {
            problems.push(format!("{p}: {why}"));
        }
    }
    Ok((files.len(), problems))
}

/// Extracts the archive into `dest` (created). Only regular files and folders; refuses unsafe paths.
pub fn extract(gz: &[u8], dest: &Path) -> std::io::Result<usize> {
    std::fs::create_dir_all(dest)?;
    let mut n = 0;
    for (path, data) in read_archive(gz)? {
        let rel = safe_relative(&path).ok_or_else(|| std::io::Error::other(format!("unsafe path in archive: {path}")))?;
        let target = dest.join(rel);
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&target, data)?;
        n += 1;
    }
    Ok(n)
}

pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    let d = sha2::Sha256::digest(data);
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// The runtime source carried inside the executable itself, so a lone `Kalamar.exe` (on the desktop, or run from inside
/// an archive viewer) works without a `resources\` folder beside it. These are the very files of `src-tauri/resources`
/// (a test fails if the copy under test and this one ever differ).
static EMBEDDED_ARCHIVE: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/resources/runtime-src.tar.gz"));
static EMBEDDED_MANIFEST: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/resources/runtime-src.json"));

/// The embedded archive and manifest bytes (tests and the scan of the shipped bytes use them).
pub fn embedded() -> (&'static [u8], &'static [u8]) {
    (EMBEDDED_ARCHIVE, EMBEDDED_MANIFEST)
}

/// Why no usable runtime source could be provided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundleError {
    /// The embedded copy does not match its own manifest (a broken build).
    Corrupt,
    /// The embedded copy could not be written to the data folder.
    Io(String),
}

impl std::fmt::Display for BundleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BundleError::Corrupt => write!(f, "the runtime source embedded in the launcher is damaged"),
            BundleError::Io(m) => write!(f, "the runtime source could not be written to the data folder: {m}"),
        }
    }
}

/// Folders that may hold the shipped files, in priority order: the Tauri resource folder (installer), the `resources\`
/// folder beside the executable, and (development builds only) the source tree's `src-tauri/resources`.
fn candidates(resource_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut cands = Vec::new();
    if let Some(r) = resource_dir {
        cands.push(r.join("resources"));
        cands.push(r.to_path_buf());
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(d) = exe.parent()
    {
        cands.push(d.join("resources"));
    }
    #[cfg(all(debug_assertions, not(test)))]
    cands.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources"));
    cands
}

/// The manifest of `dir` when it holds an archive and a manifest and the archive's SHA-256 is the manifest's.
pub fn verified(dir: &Path) -> Option<Manifest> {
    let man = load_manifest(dir)?;
    let gz = std::fs::read(dir.join(ARCHIVE_NAME)).ok()?;
    (sha256_hex(&gz) == man.sha256).then_some(man)
}

/// A folder next to the program or in the resource folder that holds an intact archive, and the very archive compiled
/// into this executable (first such folder, in priority order). A folder whose archive does not match its manifest is
/// passed over, and so is an intact one of another version: the `resources` of an installed copy whose program was
/// replaced by a newer one (the launcher's own update, or a copy put there by hand) still hold the older runtime source,
/// which would hide the game update the new launcher brings.
pub fn locate(resource_dir: Option<&Path>) -> Option<PathBuf> {
    let want = serde_json::from_slice::<Manifest>(embedded().1).ok()?.sha256;
    candidates(resource_dir).into_iter().find(|d| verified(d).is_some_and(|m| m.sha256 == want))
}

/// Folder name of the embedded copy under `<data>/runtime-src/`: the SHA-256 of its manifest.
fn embedded_dir(data_dir: &Path, manifest: &[u8]) -> PathBuf {
    data_dir.join("runtime-src").join(sha256_hex(manifest))
}

/// Makes sure `<data>/runtime-src/<sha of the manifest>/` holds the given archive and manifest, intact: reuses it when
/// its archive still matches the manifest, else writes a fresh copy beside it and renames it into place.
pub fn ensure_extracted(data_dir: &Path, archive: &[u8], manifest: &[u8]) -> Result<PathBuf, BundleError> {
    let want: Manifest = serde_json::from_slice(manifest).map_err(|_| BundleError::Corrupt)?;
    if sha256_hex(archive) != want.sha256 {
        return Err(BundleError::Corrupt);
    }
    let dir = embedded_dir(data_dir, manifest);
    let intact = |d: &Path| verified(d).is_some_and(|m| m.sha256 == want.sha256);
    if intact(&dir) {
        return Ok(dir);
    }
    let io = |e: std::io::Error| BundleError::Io(e.to_string());
    let parent = dir.parent().unwrap_or(data_dir);
    std::fs::create_dir_all(parent).map_err(io)?;
    let tmp = parent.join(format!("{}.tmp-{}", dir.file_name().unwrap_or_default().to_string_lossy(), std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(io)?;
    let written = std::fs::write(tmp.join(ARCHIVE_NAME), archive).and_then(|()| std::fs::write(tmp.join(MANIFEST_NAME), manifest));
    if let Err(e) = written {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(io(e));
    }
    // What was written is read back and checked before it takes the place of anything.
    if !intact(&tmp) {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(BundleError::Io("the copy written to disk does not match its manifest".into()));
    }
    if dir.exists() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    if let Err(e) = std::fs::rename(&tmp, &dir) {
        let _ = std::fs::remove_dir_all(&tmp);
        // Another launcher instance may have just put the same copy there.
        return if intact(&dir) { Ok(dir) } else { Err(io(e)) };
    }
    if intact(&dir) { Ok(dir) } else { Err(BundleError::Io("the copy in the data folder does not match its manifest".into())) }
}

/// The folder the install wizard and the freshness check read the runtime source from: a valid `resources` folder when
/// there is one, else the copy embedded in the executable, written under `data_dir` (reused while intact).
pub fn resolve(resource_dir: Option<&Path>, data_dir: &Path) -> Result<PathBuf, BundleError> {
    if let Some(d) = locate(resource_dir) {
        crate::trace::mark(&format!("runtime:resources {}", d.display()));
        return Ok(d);
    }
    let (gz, man) = embedded();
    let r = ensure_extracted(data_dir, gz, man);
    match &r {
        Ok(d) => crate::trace::mark(&format!("runtime:embedded {}", d.display())),
        Err(e) => crate::trace::mark(&format!("runtime:error {e}")),
    }
    r
}

pub fn load_manifest(dir: &Path) -> Option<Manifest> {
    let text = std::fs::read_to_string(dir.join(MANIFEST_NAME)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Source files the lifted C depends on (the code generator, its targets and patch tables, and the marker it writes
/// into every file, from `tools/check_content.py`).
pub const LIFT_INPUTS: &[&str] = &["recomp/", "tools/check_content.py"];
/// Source files the shader translator is built from (what `tools/build_xlate.py` puts in its identity).
pub const XLATE_INPUTS: &[&str] = &["tools/build_xlate.py", "tools/xlate/", "tools/vendor/ryujinx/"];
/// Source files the save keys are read with (the keys step's script).
pub const KEYS_INPUTS: &[&str] = &["kit-tools/profile/s3save.py"];

fn inputs_digest(mut files: Vec<(String, String)>) -> String {
    files.sort();
    let mut text = String::new();
    for (p, h) in files {
        text.push_str(&format!("{p}\0{h}\n"));
    }
    sha256_hex(text.as_bytes())
}

fn wanted(path: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|p| if p.ends_with('/') { path.starts_with(p) } else { path == *p })
}

/// Digest of the files under `prefixes` in an archive (path + content hash, sorted).
pub fn inputs_hash_archive(files: &[(String, Vec<u8>)], prefixes: &[&str]) -> String {
    inputs_digest(files.iter().filter(|(p, _)| wanted(p, prefixes)).map(|(p, d)| (p.clone(), sha256_hex(d))).collect())
}

/// The same digest from the files' hashes (`incremental::archive_hashes`).
pub fn inputs_hash_map(hashes: &std::collections::BTreeMap<String, String>, prefixes: &[&str]) -> String {
    inputs_digest(hashes.iter().filter(|(p, _)| wanted(p, prefixes)).map(|(p, h)| (p.clone(), h.clone())).collect())
}

/// The same digest over an extracted source folder (`""` when none of the files are there).
pub fn inputs_hash_dir(src: &Path, prefixes: &[&str]) -> String {
    let mut files = Vec::new();
    let mut stack = vec![src.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(rel) = p.strip_prefix(src) else { continue };
            let rel = rel.to_string_lossy().replace('\\', "/");
            if p.is_dir() {
                // Descend only where a wanted file can be (never into Python's caches, which no archive holds).
                if !rel.ends_with("__pycache__") && prefixes.iter().any(|w| w.starts_with(&format!("{rel}/")) || rel.starts_with(w.trim_end_matches('/'))) {
                    stack.push(p);
                }
            } else if wanted(&rel, prefixes)
                && let Ok(data) = std::fs::read(&p)
            {
                files.push((rel, sha256_hex(&data)));
            }
        }
    }
    if files.is_empty() { String::new() } else { inputs_digest(files) }
}

/// Snapshot identifier recorded in an extracted source folder by [`write_marker`].
pub fn load_marker(src: &Path) -> Option<String> {
    let text = std::fs::read_to_string(src.join(MARKER_NAME)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("commit")?.as_str().map(str::to_string)
}

/// SHA-256 of the archive an extracted source folder came from (`None` for markers written before it was recorded):
/// the same commit can ship with other kit-tools.
pub fn load_marker_archive(src: &Path) -> Option<String> {
    let text = std::fs::read_to_string(src.join(MARKER_NAME)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("archive_sha256")?.as_str().map(str::to_string)
}

pub fn write_marker(src: &Path, m: &Manifest) -> std::io::Result<()> {
    let v = serde_json::json!({ "commit": m.commit, "archive_sha256": m.sha256, "extracted": crate::util::now_secs() });
    std::fs::write(src.join(MARKER_NAME), serde_json::to_string_pretty(&v).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_catch_game_files_and_keys() {
        assert!(scan_entry("runtime/a.c", b"int x;").is_empty());
        assert!(!scan_entry("x/Splatoon.nsp", b"").is_empty());
        assert!(!scan_entry("exefs/main", b"").is_empty());
        assert!(!scan_entry("tools/romfs.bin", b"").is_empty());
        assert!(!scan_entry("kit-tools/profile/s3save.keys.json", b"{}").is_empty());
        assert!(!scan_entry("lift/main/main_0001.c", b"int x;").is_empty());
        assert!(!scan_entry("docs/shot.png", b"").is_empty());
        assert!(scan_entry("runtime/kalamar.ico", b"x").is_empty());
        let marker = format!("// {GENERATED_MARKER}\n");
        assert!(!scan_entry("recomp/x.c", marker.as_bytes()).is_empty());
        let key = format!("header_key = {}\n", "ab".repeat(32));
        assert!(!scan_entry("notes.txt", key.as_bytes()).is_empty());
        assert!(scan_entry("notes.txt", b"key = value\nheader_key = 12\n").is_empty());
        assert!(!scan_entry("big.txt", &vec![b'a'; 2 * 1024 * 1024]).is_empty());
    }

    #[test]
    fn unsafe_paths_are_refused() {
        assert!(safe_relative("../x").is_none());
        assert!(safe_relative("C:/x").is_none());
        assert!(safe_relative("/x").is_none());
        assert_eq!(safe_relative("a/./b.py").unwrap(), Path::new("a").join("b.py"));
    }

    fn shipped_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources")
    }

    /// The archive that ships with the launcher holds nothing derived from the game, and matches its manifest. The bytes
    /// compiled into the executable are scanned too, and must be the very files of `resources\`.
    #[test]
    fn shipped_bundle_is_clean() {
        let dir = shipped_dir();
        let Ok(gz) = std::fs::read(dir.join(ARCHIVE_NAME)) else { panic!("{} missing: run examples/bundle_runtime.rs", ARCHIVE_NAME) };
        let man = load_manifest(&dir).expect("manifest");
        assert_eq!(sha256_hex(&gz), man.sha256, "manifest does not match the archive");
        let (n, problems) = scan_archive(&gz).unwrap();
        assert_eq!(n, man.files);
        assert!(problems.is_empty(), "{problems:#?}");
        assert!(n > 500);

        let (egz, eman) = embedded();
        assert!(egz == gz.as_slice(), "the archive embedded in the executable differs from resources/{ARCHIVE_NAME}: rebuild");
        assert!(eman == std::fs::read(dir.join(MANIFEST_NAME)).unwrap().as_slice(), "the embedded manifest differs from resources/{MANIFEST_NAME}");
        let (en, eproblems) = scan_archive(egz).unwrap();
        assert_eq!(en, man.files);
        assert!(eproblems.is_empty(), "{eproblems:#?}");
        assert_eq!(serde_json::from_slice::<Manifest>(eman).unwrap().sha256, sha256_hex(egz));
    }

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("s3l-bundle-{tag}-{}", crate::util::random_hex(6)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A tiny valid bundle (archive bytes, manifest bytes).
    fn tiny(text: &str) -> (Vec<u8>, Vec<u8>) {
        let mut tar = tar::Builder::new(Vec::new());
        let mut h = tar::Header::new_gnu();
        h.set_size(text.len() as u64);
        h.set_mode(0o644);
        h.set_cksum();
        tar.append_data(&mut h, "runtime/a.py", text.as_bytes()).unwrap();
        let raw = tar.into_inner().unwrap();
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut enc, &raw).unwrap();
        let gz = enc.finish().unwrap();
        let man = Manifest { commit: format!("c-{text}"), files: 1, sha256: sha256_hex(&gz), ..Default::default() };
        (gz, serde_json::to_vec(&man).unwrap())
    }

    fn put(dir: &Path, b: &(Vec<u8>, Vec<u8>)) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(ARCHIVE_NAME), &b.0).unwrap();
        std::fs::write(dir.join(MANIFEST_NAME), &b.1).unwrap();
    }

    #[test]
    fn embedded_copy_is_extracted_under_the_manifest_hash_and_reused() {
        let data = temp("extract");
        let b = tiny("one");
        let dir = ensure_extracted(&data, &b.0, &b.1).unwrap();
        assert_eq!(dir, data.join("runtime-src").join(sha256_hex(&b.1)));
        assert_eq!(std::fs::read(dir.join(ARCHIVE_NAME)).unwrap(), b.0);
        assert_eq!(std::fs::read(dir.join(MANIFEST_NAME)).unwrap(), b.1);
        assert_eq!(verified(&dir).unwrap().commit, "c-one");
        // Nothing but the final folder is left beside it.
        assert_eq!(std::fs::read_dir(data.join("runtime-src")).unwrap().count(), 1);
        // Reuse: the files are not rewritten.
        let before = std::fs::metadata(dir.join(ARCHIVE_NAME)).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert_eq!(ensure_extracted(&data, &b.0, &b.1).unwrap(), dir);
        assert_eq!(std::fs::metadata(dir.join(ARCHIVE_NAME)).unwrap().modified().unwrap(), before);
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn corrupt_or_partial_extraction_is_replaced() {
        let data = temp("corrupt");
        let b = tiny("two");
        let dir = ensure_extracted(&data, &b.0, &b.1).unwrap();
        std::fs::write(dir.join(ARCHIVE_NAME), &b.0[..b.0.len() / 2]).unwrap();
        assert!(verified(&dir).is_none());
        assert_eq!(ensure_extracted(&data, &b.0, &b.1).unwrap(), dir);
        assert_eq!(std::fs::read(dir.join(ARCHIVE_NAME)).unwrap(), b.0);
        assert!(verified(&dir).is_some());
        // Manifest missing: replaced as well.
        std::fs::remove_file(dir.join(MANIFEST_NAME)).unwrap();
        ensure_extracted(&data, &b.0, &b.1).unwrap();
        assert!(verified(&dir).is_some());
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn an_embedded_copy_that_does_not_match_its_manifest_is_refused() {
        let data = temp("badembed");
        let b = tiny("three");
        let mut bad = b.0.clone();
        bad.push(0);
        assert_eq!(ensure_extracted(&data, &bad, &b.1), Err(BundleError::Corrupt));
        assert_eq!(ensure_extracted(&data, &b.0, b"not json"), Err(BundleError::Corrupt));
        assert!(!data.join("runtime-src").exists());
        let _ = std::fs::remove_dir_all(&data);
    }

    fn embedded_pair() -> (Vec<u8>, Vec<u8>) {
        let (gz, man) = embedded();
        (gz.to_vec(), man.to_vec())
    }

    #[test]
    fn a_resources_folder_with_the_embedded_archive_wins_over_extracting_it() {
        let (data, res) = (temp("prio-data"), temp("prio-res"));
        put(&res.join("resources"), &embedded_pair());
        let got = resolve(Some(&res), &data).unwrap();
        assert_eq!(got, res.join("resources"));
        assert!(!data.join("runtime-src").exists(), "nothing is extracted when resources is usable");
        // The resource folder itself (no `resources` sub-folder) is accepted too.
        let res2 = temp("prio-res2");
        put(&res2, &embedded_pair());
        assert_eq!(resolve(Some(&res2), &data).unwrap(), res2);
        for d in [data, res, res2] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// After the launcher's own update replaced only the program of an installed copy, its `resources` folder still
    /// holds the previous, intact runtime source: the new launcher uses its own (embedded) one, so that the Play page
    /// offers the game update it brings.
    #[test]
    fn an_intact_resources_folder_of_another_version_is_passed_over() {
        let (data, res) = (temp("stale-data"), temp("stale-res"));
        let older = tiny("previous-launcher");
        put(&res.join("resources"), &older);
        assert!(verified(&res.join("resources")).is_some(), "intact, just not this launcher's");
        assert!(locate(Some(&res)).is_none());
        let got = resolve(Some(&res), &data).unwrap();
        assert!(got.starts_with(data.join("runtime-src")), "{}", got.display());
        assert_eq!(std::fs::read(got.join(MANIFEST_NAME)).unwrap(), embedded().1);
        assert_ne!(load_manifest(&got).unwrap().commit, "c-previous-launcher");
        for d in [data, res] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn a_resources_folder_whose_archive_does_not_match_is_passed_over() {
        let (data, res) = (temp("skip-data"), temp("skip-res"));
        let b = tiny("tampered");
        put(&res, &(b.0[..b.0.len() - 3].to_vec(), b.1.clone()));
        assert!(locate(Some(&res)).is_none());
        let got = resolve(Some(&res), &data).unwrap();
        assert!(got.starts_with(data.join("runtime-src")), "{}", got.display());
        let (egz, eman) = embedded();
        assert_eq!(std::fs::read(got.join(ARCHIVE_NAME)).unwrap(), egz);
        assert_eq!(std::fs::read(got.join(MANIFEST_NAME)).unwrap(), eman);
        // No resource folder at all: the same copy, reused.
        assert_eq!(resolve(None, &data).unwrap(), got);
        for d in [data, res] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn an_unwritable_data_folder_gives_an_io_error() {
        let base = temp("unwritable");
        let file = base.join("a-file");
        std::fs::write(&file, b"x").unwrap();
        let b = tiny("four");
        assert!(matches!(ensure_extracted(&file, &b.0, &b.1), Err(BundleError::Io(_))));
        let _ = std::fs::remove_dir_all(&base);
    }
}
