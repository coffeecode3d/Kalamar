//! Programs the launcher carries ready-built, so the player's PC needs neither the .NET SDK, the Visual Studio C++
//! build tools, Git, nor a C compiler for them: `hactool.exe` (SciresM/hactool, ISC, with MacNativeGames' Windows patch,
//! MIT, our patches, and its bundled mbedtls 2.6.1, Apache-2.0) and the shader translator `totk_xlate.exe` (ours, MIT,
//! with Ryujinx's shader recompiler, MIT, and the NativeAOT .NET runtime, MIT). Both are free software built from the
//! engine repository's sources (`tools/build_hactool.py`, `tools/build_xlate.py`); neither holds anything of a game.
//!
//! `resources/kalamar-tools.tar.gz` + `kalamar-tools.json` (made by `examples/bundle_tools.rs`) hold them with their
//! license texts. The manifest names the translator's inputs digest (`bundle::XLATE_INPUTS` of the runtime source it
//! was built from): the wizard installs it only when that is the digest of the source it installs, else it builds the
//! translator the old way. Like the runtime source, the pair is also compiled into the executable.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

pub const ARCHIVE_NAME: &str = if cfg!(windows) { "kalamar-tools.tar.gz" } else { "kalamar-tools-linux.tar.gz" };
pub const MANIFEST_NAME: &str = if cfg!(windows) { "kalamar-tools.json" } else { "kalamar-tools-linux.json" };
pub const HACTOOL: &str = if cfg!(windows) { "bin/hactool.exe" } else { "bin/hactool" };
pub const XLATE_EXE: &str = if cfg!(windows) { "xlate/totk_xlate.exe" } else { "xlate/totk_xlate" };
pub const XLATE_ID: &str = "xlate/totk_xlate.id";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct Manifest {
    /// SHA-256 of the archive.
    pub sha256: String,
    /// Every file of the archive (path, SHA-256).
    pub files: Vec<(String, String)>,
    /// Engine commit the two programs were built from.
    pub engine_commit: String,
    /// `bundle::XLATE_INPUTS` digest of the runtime source the translator was built from.
    pub xlate_inputs: String,
    /// The translator's own identity (`tools/build_xlate.py`, the `totk_xlate.id` beside it).
    pub xlate_id: String,
    /// hactool's pinned commit and the patches applied.
    pub hactool: String,
    /// hactool reads an NCA in place inside its NSP (`range.patch`): the RomFS merge then copies nothing.
    pub hactool_ranges: bool,
}

#[derive(Debug, Clone)]
pub struct Tools {
    pub manifest: Manifest,
    pub files: BTreeMap<String, Vec<u8>>,
}

// Windows builds carry the two programs inside the executable (they are a few MB). The Linux build does not: the
// release workflow builds them from the engine's sources and puts them in the bundle's resources folder, where
// `load` finds them; without them the wizard builds both from source (`build_hactool.py`, `build_xlate.py`).
#[cfg(windows)]
static EMBEDDED_ARCHIVE: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/resources/kalamar-tools.tar.gz"));
#[cfg(windows)]
static EMBEDDED_MANIFEST: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/resources/kalamar-tools.json"));

pub fn embedded() -> (&'static [u8], &'static [u8]) {
    #[cfg(windows)]
    {
        (EMBEDDED_ARCHIVE, EMBEDDED_MANIFEST)
    }
    #[cfg(not(windows))]
    {
        (&[], &[])
    }
}

/// The archive and manifest, checked: the archive's SHA-256 is the manifest's and every file's is listed. `None` when
/// anything does not match.
pub fn parse(gz: &[u8], manifest: &[u8]) -> Option<Tools> {
    let manifest: Manifest = serde_json::from_slice(manifest).ok()?;
    if super::bundle::sha256_hex(gz) != manifest.sha256 {
        return None;
    }
    let mut files = BTreeMap::new();
    let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(gz));
    for e in ar.entries().ok()? {
        let mut e = e.ok()?;
        if !e.header().entry_type().is_file() {
            continue;
        }
        let path = e.path().ok()?.to_string_lossy().replace('\\', "/");
        let mut data = Vec::new();
        e.read_to_end(&mut data).ok()?;
        files.insert(path, data);
    }
    let listed: BTreeMap<&str, &str> = manifest.files.iter().map(|(p, h)| (p.as_str(), h.as_str())).collect();
    if listed.len() != files.len() || files.iter().any(|(p, d)| listed.get(p.as_str()) != Some(&super::bundle::sha256_hex(d).as_str())) {
        return None;
    }
    Some(Tools { manifest, files })
}

/// The pair compiled into the executable. The copy in `dir` (the bundle folder the wizard was given) is used only when
/// it is that same pair (the same archive SHA-256): a pair dropped next to the launcher, valid in itself but another one,
/// never replaces the programs the launcher carries and runs.
pub fn load(dir: Option<&Path>) -> Option<Tools> {
    // The copy compiled in is checked once per process (the Play page asks for it often).
    static EMBEDDED: std::sync::OnceLock<Option<Tools>> = std::sync::OnceLock::new();
    let embedded = EMBEDDED
        .get_or_init(|| {
            let (gz, m) = embedded();
            parse(gz, m)
        })
        .clone();
    #[cfg(not(windows))]
    {
        // Linux: the release's resource folder, built by the release workflow (engine tools/build_hactool.py,
        // tools/build_xlate.py); a source build has none and the wizard builds both from source.
        let Some(d) = dir else { return embedded };
        let (Ok(gz), Ok(m)) = (std::fs::read(d.join(ARCHIVE_NAME)), std::fs::read(d.join(MANIFEST_NAME))) else { return embedded };
        return parse(&gz, &m).or(embedded);
    }
    #[cfg(windows)]
    load_with(dir, embedded)
}

#[cfg_attr(not(windows), allow(dead_code))]
fn load_with(dir: Option<&Path>, embedded: Option<Tools>) -> Option<Tools> {
    let embedded = embedded?;
    if let Some(d) = dir
        && let (Ok(gz), Ok(m)) = (std::fs::read(d.join(ARCHIVE_NAME)), std::fs::read(d.join(MANIFEST_NAME)))
        && let Some(t) = parse(&gz, &m)
        && t.manifest.sha256 == embedded.manifest.sha256
    {
        return Some(t);
    }
    Some(embedded)
}

impl Tools {
    pub fn has(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    /// Writes `path` of the archive to `to` unless a file with the same content is there (a temporary name renamed
    /// over it, so a reader never sees half a file).
    pub fn install(&self, path: &str, to: &Path) -> std::io::Result<bool> {
        let data = self.files.get(path).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, path.to_string()))?;
        if std::fs::read(to).is_ok_and(|have| have == *data) {
            return Ok(false);
        }
        if let Some(d) = to.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = to.with_extension("new");
        std::fs::write(&tmp, data)?;
        std::fs::rename(&tmp, to)?;
        Ok(true)
    }

    /// The license texts, written to `dir`.
    pub fn install_licences(&self, dir: &Path) -> std::io::Result<usize> {
        let mut n = 0;
        for (p, _) in self.files.iter().filter(|(p, _)| p.starts_with("licenses/")) {
            // A path of the archive is checked before it is joined: no `..`, root or drive leaves `dir`.
            let Some(rel) = super::portable::safe_rel(&p["licenses/".len()..]) else { continue };
            self.install(p, &dir.join(rel))?;
            n += 1;
        }
        Ok(n)
    }

    /// The translator carried here was built from the sources whose digest is `xlate_inputs`.
    pub fn translator_for(&self, xlate_inputs: &str) -> bool {
        !xlate_inputs.is_empty() && self.manifest.xlate_inputs == xlate_inputs && self.has(XLATE_EXE) && self.has(XLATE_ID)
    }
}

/// Traces of the machine a program was built on, which must never ship: a Windows profile path (`Users\` or `Users/`
/// followed by a name, as a linker's PDB path or a compiler's source path leaves them), the user's name and the given
/// folders (the repositories), each in ASCII and in UTF-16LE. Returns what was found, with its offset.
pub fn machine_traces(data: &[u8], user: &str, folders: &[&Path]) -> Vec<String> {
    let utf16 = |s: &str| -> Vec<u8> { s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect() };
    let find_all = |needle: &[u8]| -> Vec<usize> {
        if needle.is_empty() || needle.len() > data.len() {
            return Vec::new();
        }
        data.windows(needle.len()).enumerate().filter(|(_, w)| *w == needle).map(|(i, _)| i).collect()
    };
    let mut found = Vec::new();
    // "Users\<name>": the separator, then a letter or digit (one byte, or one UTF-16 unit).
    for word in ["Users", "users", "USERS"] {
        for sep in ['\\', '/'] {
            let text = format!("{word}{sep}");
            for (needle, wide) in [(text.as_bytes().to_vec(), false), (utf16(&text), true)] {
                for at in find_all(&needle) {
                    let next = data.get(at + needle.len()).copied().unwrap_or(0);
                    let next_hi = if wide { data.get(at + needle.len() + 1).copied().unwrap_or(1) } else { 0 };
                    if next.is_ascii_alphanumeric() && next_hi == 0 {
                        found.push(format!("{}\"{text}<name>\" at {at}", if wide { "UTF-16 " } else { "" }));
                    }
                }
            }
        }
    }
    let mut words: Vec<String> = Vec::new();
    // A short user name would match by chance inside binary data.
    if user.chars().count() >= 3 {
        words.push(user.to_string());
    }
    for f in folders {
        let s = f.display().to_string();
        let s = s.trim_start_matches(r"\\?\").to_string();
        if s.len() >= 6 {
            words.push(s.replace('/', "\\"));
            words.push(s.replace('\\', "/"));
        }
    }
    for w in words {
        for (needle, wide) in [(w.as_bytes().to_vec(), false), (utf16(&w), true)] {
            if let Some(at) = find_all(&needle).first() {
                found.push(format!("{}{w:?} at {at}", if wide { "UTF-16 " } else { "" }));
            }
        }
    }
    found
}

/// The name of the user running this program (`USERNAME`, `USER`).
pub fn current_user() -> String {
    std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default()
}

/// Makes the archive and manifest from `(path, bytes)` (deterministic: sorted, no dates). Used by
/// `examples/bundle_tools.rs` and the tests.
pub fn build(files: &[(String, Vec<u8>)], mut manifest: Manifest) -> (Vec<u8>, Vec<u8>) {
    let mut sorted: Vec<&(String, Vec<u8>)> = files.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    {
        let mut b = tar::Builder::new(&mut gz);
        b.mode(tar::HeaderMode::Deterministic);
        for (p, d) in &sorted {
            let mut h = tar::Header::new_gnu();
            h.set_size(d.len() as u64);
            h.set_mode(0o644);
            h.set_mtime(0);
            h.set_cksum();
            b.append_data(&mut h, p, &d[..]).expect("tar");
        }
        b.finish().expect("tar");
    }
    let gz = gz.finish().expect("gzip");
    manifest.files = sorted.iter().map(|(p, d)| (p.clone(), super::bundle::sha256_hex(d))).collect();
    manifest.sha256 = super::bundle::sha256_hex(&gz);
    let m = serde_json::to_vec_pretty(&manifest).expect("json");
    (gz, m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> (Vec<u8>, Vec<u8>) {
        build(
            &[
                (HACTOOL.into(), b"MZ hactool".to_vec()),
                (XLATE_EXE.into(), b"MZ xlate".to_vec()),
                (XLATE_ID.into(), b"abc\n".to_vec()),
                ("licenses/hactool-LICENSE.txt".into(), b"ISC".to_vec()),
            ],
            Manifest { xlate_inputs: "digest-1".into(), hactool_ranges: true, ..Default::default() },
        )
    }

    #[test]
    fn a_valid_pair_is_read_and_a_tampered_one_refused() {
        let (gz, m) = sample();
        let t = parse(&gz, &m).unwrap();
        assert_eq!(t.files.len(), 4);
        assert!(t.translator_for("digest-1") && !t.translator_for("digest-2") && !t.translator_for(""));
        let mut bad = gz.clone();
        let n = bad.len();
        bad[n - 5] ^= 1;
        assert!(parse(&bad, &m).is_none());
        // a manifest listing another hash for a file
        let mut man: Manifest = serde_json::from_slice(&m).unwrap();
        man.files[0].1 = "00".repeat(32);
        assert!(parse(&gz, &serde_json::to_vec(&man).unwrap()).is_none());
    }

    #[test]
    fn install_writes_once_and_replaces_a_different_file() {
        let (gz, m) = sample();
        let t = parse(&gz, &m).unwrap();
        let d = std::env::temp_dir().join(format!("s3l-prebuilt-{}", crate::util::random_hex(4)));
        let to = d.join("bin").join("hactool.exe");
        assert!(t.install(HACTOOL, &to).unwrap());
        assert!(!t.install(HACTOOL, &to).unwrap(), "same content: not rewritten");
        std::fs::write(&to, b"old").unwrap();
        assert!(t.install(HACTOOL, &to).unwrap());
        assert_eq!(std::fs::read(&to).unwrap(), b"MZ hactool");
        assert_eq!(t.install_licences(&d.join("licenses")).unwrap(), 1);
        assert!(d.join("licenses").join("hactool-LICENSE.txt").is_file());
        super::super::sys::remove_all(&d).unwrap();
    }

    /// A pair dropped in the bundle folder (valid in itself, its own hashes) must not replace the programs the launcher
    /// carries, and a license path of the archive is never joined unchecked.
    #[test]
    fn a_planted_pair_never_replaces_the_carried_one() {
        let (gz, m) = sample();
        let carried = parse(&gz, &m).unwrap();
        let d = std::env::temp_dir().join(format!("s3l-plant-{}", crate::util::random_hex(4)));
        std::fs::create_dir_all(&d).unwrap();
        let (fgz, fm) = build(&[(HACTOOL.into(), b"MZ FORGED".to_vec())], Manifest::default());
        std::fs::write(d.join(ARCHIVE_NAME), &fgz).unwrap();
        std::fs::write(d.join(MANIFEST_NAME), &fm).unwrap();
        assert!(parse(&fgz, &fm).is_some(), "the planted pair is valid in itself");
        let t = load_with(Some(&d), Some(carried.clone())).unwrap();
        assert_eq!(t.files[HACTOOL], b"MZ hactool", "the carried hactool is installed");
        // The same pair as the carried one in the folder is fine; no carried pair at all: nothing is trusted.
        std::fs::write(d.join(ARCHIVE_NAME), &gz).unwrap();
        std::fs::write(d.join(MANIFEST_NAME), &m).unwrap();
        assert_eq!(load_with(Some(&d), Some(carried)).unwrap().manifest.sha256, parse(&gz, &m).unwrap().manifest.sha256);
        assert!(load_with(Some(&d), None).is_none());
        // A license path that climbs out of the folder is skipped.
        let t = Tools { manifest: Manifest::default(), files: BTreeMap::from([("licenses/../../escaped.txt".to_string(), b"x".to_vec()), ("licenses/ok.txt".to_string(), b"y".to_vec())]) };
        let lic = d.join("a").join("b").join("licenses");
        assert_eq!(t.install_licences(&lic).unwrap(), 1);
        assert!(lic.join("ok.txt").is_file() && !d.join("a").join("escaped.txt").exists() && !d.join("escaped.txt").exists());
        super::super::sys::remove_all(&d).unwrap();
    }

    /// What ships with the launcher is valid, carries both programs and their licenses, and its translator was built
    /// from the runtime source that ships beside it.
    #[cfg(windows)]
    #[test]
    fn shipped_tools_match_the_shipped_source() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
        let gz = std::fs::read(dir.join(ARCHIVE_NAME)).expect("run examples/bundle_tools.rs");
        let m = std::fs::read(dir.join(MANIFEST_NAME)).unwrap();
        let t = parse(&gz, &m).expect("valid pair");
        let (egz, em) = embedded();
        assert!(egz == gz.as_slice() && em == m.as_slice(), "the embedded copy differs from resources/: rebuild");
        assert!(t.has(HACTOOL) && t.has(XLATE_EXE) && t.has(XLATE_ID));
        assert!(t.manifest.hactool_ranges);
        for lic in ["hactool-LICENSE.txt", "mbedtls-LICENSE.txt", "MacNativeGames-LICENSE.txt", "Ryujinx-LICENSE.txt", "totk_xlate-LICENSE.txt", "dotnet-runtime-LICENSE.txt"] {
            assert!(t.has(&format!("licenses/{lic}")), "{lic}");
        }
        let src = std::fs::read(dir.join(super::super::bundle::ARCHIVE_NAME)).unwrap();
        let files = super::super::bundle::read_archive(&src).unwrap();
        let digest = super::super::bundle::inputs_hash_archive(&files, super::super::bundle::XLATE_INPUTS);
        assert!(t.translator_for(&digest), "the shipped translator was built from other sources than the shipped runtime source");
        assert_eq!(String::from_utf8_lossy(&t.files[XLATE_ID]).trim(), t.manifest.xlate_id);
        // Nothing of the machine that built them: no profile path, user name or repository path in any file.
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo = manifest_dir.parent().unwrap_or(&manifest_dir).to_path_buf();
        let projects = repo.parent().unwrap_or(&repo).to_path_buf();
        for (p, d) in &t.files {
            let traces = machine_traces(d, &current_user(), &[&repo, &projects]);
            assert!(traces.is_empty(), "{p} carries traces of the build machine: {traces:?}");
        }
    }

    #[test]
    fn traces_of_the_build_machine_are_found_in_ascii_and_utf16() {
        let user = "Someone";
        let repo = std::path::Path::new(r"D:\work\kalamar-repo");
        let wide = |s: &str| -> Vec<u8> { s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect() };
        // a PDB path left by the linker
        // built from its parts, so that no line of this file holds a whole profile path
        let mut exe = [b"MZ....RSDS....C:\\Users\\".as_slice(), user.as_bytes(), b"\\Documents\\x\\totk_xlate.pdb\0"].concat();
        let t = machine_traces(&exe, user, &[repo]);
        assert!(t.iter().any(|x| x.contains("Users\\<name>")) && t.iter().any(|x| x.contains("Someone")), "{t:?}");
        // a UTF-16 string, forward slashes, another user
        exe = wide("c:/users/user/source/file.cs");
        assert!(machine_traces(&exe, user, &[repo]).iter().any(|x| x.starts_with("UTF-16 \"users/<name>")));
        // the repository path, in either slash style
        assert!(!machine_traces(b"..D:/work/kalamar-repo/src..", user, &[repo]).is_empty());
        assert!(!machine_traces(&wide(r"D:\work\kalamar-repo"), user, &[repo]).is_empty());
        // clean data, and "Users\" not followed by a name
        assert!(machine_traces(b"MZ hactool (c) SciresM; see Users\\ manual", user, &[repo]).is_empty());
        assert!(machine_traces(b"MZ plain program", user, &[repo]).is_empty());
        // a user name too short to search for
        assert!(machine_traces(b"ab cd ab", "ab", &[]).is_empty());
    }
}
