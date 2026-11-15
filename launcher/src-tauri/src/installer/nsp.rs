//! Quick checks of the player's three files, made when they are picked (before anything is installed): the NSP
//! container's file table and its plain-text content-meta XML when the dump has one, and the names in prod.keys.
//! Nothing is decrypted here; only the file table, the small XML and the key *names* are read. Key values are never
//! read into a result, logged or shown. The exact version check (SHA-256 of the extracted `main`) still runs during the
//! install, before any long step.

use serde::Serialize;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Title id of the game and of its update.
pub const TITLE_BASE: u64 = 0x0100_C250_0FC2_0000;
pub const TITLE_UPDATE: u64 = 0x0100_C250_0FC2_0800;
use super::gamever::{CURRENT, PREVIOUS};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Meta {
    pub kind: String,
    pub title_id: u64,
    pub version: u64,
}

/// Result of a file check. `status`: `ok`, `warn` (accepted, checked again during the install) or `bad`; `code` is
/// translated by the interface; `params` fill its text.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FileCheck {
    pub status: &'static str,
    pub code: &'static str,
    pub params: Vec<(String, String)>,
    pub size: Option<u64>,
    /// Size of the Program NCA (used for the merge progress).
    pub program_size: Option<u64>,
}

fn check(status: &'static str, code: &'static str) -> FileCheck {
    FileCheck { status, code, params: Vec::new(), size: None, program_size: None }
}

/// Largest content-meta XML read (dump tools write a few hundred bytes).
const XML_MAX: u64 = 64 * 1024;

/// A PFS0 file table and where its content-meta XML is, if it has one of at most [`XML_MAX`] bytes: (absolute offset,
/// size), not checked against the file's length.
pub type Pfs0Table = (Vec<Entry>, Option<(u64, u64)>);

/// Reads the PFS0 file table of an NSP.
pub fn pfs0_entries(f: &mut (impl Read + Seek)) -> std::io::Result<Pfs0Table> {
    let mut h = [0u8; 16];
    f.read_exact(&mut h)?;
    if &h[0..4] != b"PFS0" {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "not PFS0"));
    }
    let n = u32::from_le_bytes(h[4..8].try_into().unwrap()) as usize;
    let strsz = u32::from_le_bytes(h[8..12].try_into().unwrap()) as usize;
    if n == 0 || n > 512 || strsz > 64 * 1024 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "implausible PFS0 table"));
    }
    let mut ents = vec![0u8; 24 * n];
    f.read_exact(&mut ents)?;
    let mut strs = vec![0u8; strsz];
    f.read_exact(&mut strs)?;
    let data_start = (16 + 24 * n + strsz) as u64;
    let mut out = Vec::with_capacity(n);
    let mut offsets = Vec::with_capacity(n);
    for i in 0..n {
        let e = &ents[24 * i..24 * i + 24];
        let off = u64::from_le_bytes(e[0..8].try_into().unwrap());
        let size = u64::from_le_bytes(e[8..16].try_into().unwrap());
        let noff = u32::from_le_bytes(e[16..20].try_into().unwrap()) as usize;
        let end = strs[noff.min(strsz)..].iter().position(|&b| b == 0).map(|p| noff + p).unwrap_or(strsz);
        let name = String::from_utf8_lossy(&strs[noff.min(strsz)..end]).into_owned();
        out.push(Entry { name, size });
        offsets.push(off);
    }
    // Where the content-meta XML is, if any (an offset past u64 is no XML at all).
    let xml = out.iter().zip(&offsets).find(|(e, _)| e.name.ends_with(".cnmt.xml") && e.size <= XML_MAX).and_then(|(e, o)| Some((data_start.checked_add(*o)?, e.size)));
    Ok((out, xml))
}

fn tag(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find('<')? + start;
    Some(xml[start..end].trim().to_string())
}

fn parse_hex_or_dec(s: &str) -> Option<u64> {
    let s = s.trim();
    match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(h) => u64::from_str_radix(h, 16).ok(),
        None => s.parse().ok(),
    }
}

/// The content-meta XML (dump tools add it next to the NCAs): type, title id, version.
pub fn parse_cnmt_xml(xml: &str) -> Option<Meta> {
    Some(Meta { kind: tag(xml, "Type")?, title_id: parse_hex_or_dec(&tag(xml, "Id")?)?, version: parse_hex_or_dec(&tag(xml, "Version")?)? })
}

pub struct NspInfo {
    pub entries: Vec<Entry>,
    pub meta: Option<Meta>,
    pub size: u64,
}

pub fn inspect(path: &Path) -> std::io::Result<NspInfo> {
    let mut f = std::fs::File::open(path)?;
    let size = f.metadata()?.len();
    let mut magic = [0u8; 4];
    f.read_exact(&mut magic)?;
    f.seek(SeekFrom::Start(0))?;
    if &magic != b"PFS0" {
        // An XCI cartridge image: the game's files are in its `secure` HFS0 partition, and no content-meta XML is
        // stored in clear text (the check during the install still verifies the extracted code's hash).
        let entries = xci_entries(&mut f, size)?;
        return Ok(NspInfo { entries, meta: None, size });
    }
    let (entries, xml) = pfs0_entries(&mut f)?;
    let mut meta = None;
    // Exactly the entry the table located (at most XML_MAX bytes), and only when it lies inside the file.
    if let Some((at, len)) = xml {
        if at.checked_add(len).is_none_or(|end| end > size) {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "content-meta XML past the end of the file"));
        }
        f.seek(SeekFrom::Start(at))?;
        let mut buf = vec![0u8; len.min(XML_MAX) as usize];
        f.read_exact(&mut buf)?;
        meta = parse_cnmt_xml(&String::from_utf8_lossy(&buf));
    }
    Ok(NspInfo { entries, meta, size })
}

/// One entry of an HFS0 partition: its name, where its data starts (relative to the partition's data) and its size.
struct HfsEntry {
    name: String,
    offset: u64,
    size: u64,
}

/// The file table of the HFS0 partition at `at` (an XCI's root or `secure` partition), bounded by `bound` bytes:
/// its entries and the absolute offset where its data area starts (entry offsets are relative to that).
///
/// An entry is 0x40 bytes: offset (u64), size (u64), name offset (u32), hashed size (u32), hash (0x20).
fn hfs0_entries(f: &mut (impl Read + Seek), at: u64, bound: u64) -> std::io::Result<(Vec<HfsEntry>, u64)> {
    if bound < 16 {
        return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "truncated HFS0 header"));
    }
    f.seek(SeekFrom::Start(at))?;
    let mut h = [0u8; 16];
    f.read_exact(&mut h)?;
    if &h[0..4] != b"HFS0" {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "not HFS0"));
    }
    let n = u32::from_le_bytes(h[4..8].try_into().unwrap()) as usize;
    let strsz = u32::from_le_bytes(h[8..12].try_into().unwrap()) as usize;
    if n == 0 || n > 4096 || strsz > 1024 * 1024 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "implausible HFS0 table"));
    }
    let table = 16 + 64 * n + strsz;
    if table as u64 > bound {
        return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "HFS0 table past the partition"));
    }
    let mut ents = vec![0u8; 64 * n];
    f.read_exact(&mut ents)?;
    let mut strs = vec![0u8; strsz];
    f.read_exact(&mut strs)?;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let e = &ents[64 * i..64 * i + 64];
        let off = u64::from_le_bytes(e[0..8].try_into().unwrap());
        let size = u64::from_le_bytes(e[8..16].try_into().unwrap());
        let noff = u32::from_le_bytes(e[16..20].try_into().unwrap()) as usize;
        if noff >= strsz || table as u64 + off + size > bound {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "HFS0 entry out of bounds"));
        }
        let end = strs[noff..].iter().position(|&b| b == 0).map(|p| noff + p).unwrap_or(strsz);
        let name = String::from_utf8_lossy(&strs[noff..end]).into_owned();
        if name.is_empty() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "empty HFS0 name"));
        }
        out.push(HfsEntry { name, offset: off, size });
    }
    Ok((out, at + table as u64))
}

/// The files of an XCI's `secure` partition (the game's NCAs).
pub fn xci_entries(f: &mut (impl Read + Seek), size: u64) -> std::io::Result<Vec<Entry>> {
    if size < 0x140 {
        return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "truncated XCI"));
    }
    f.seek(SeekFrom::Start(0x100))?;
    let mut head = [0u8; 4];
    f.read_exact(&mut head)?;
    if &head != b"HEAD" {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "not an XCI or NSP"));
    }
    f.seek(SeekFrom::Start(0x130))?;
    let mut b = [0u8; 8];
    f.read_exact(&mut b)?;
    let root = u64::from_le_bytes(b);
    if root < 0x140 || root >= size {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "XCI root partition out of bounds"));
    }
    let (parts, root_data) = hfs0_entries(f, root, size - root)?;
    let secure = parts.iter().find(|p| p.name == "secure").ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "no secure partition"))?;
    let at = root_data.checked_add(secure.offset).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "secure partition offset"))?;
    let (entries, _) = hfs0_entries(f, at, secure.size)?;
    Ok(entries.into_iter().map(|e| Entry { name: e.name, size: e.size }).collect())
}

/// The Program NCA: the largest plain `.nca` (not `.cnmt.nca`).
pub fn program_nca(entries: &[Entry]) -> Option<&Entry> {
    entries.iter().filter(|e| e.name.ends_with(".nca") && !e.name.ends_with(".cnmt.nca")).max_by_key(|e| e.size)
}

fn has_update_ticket(entries: &[Entry]) -> bool {
    let id = format!("{TITLE_UPDATE:016x}");
    entries.iter().any(|e| e.name.ends_with(".tik") && e.name.to_ascii_lowercase().starts_with(&id))
}

/// `update_previous`: the file is the update Kalamar built before; `version` is what it is, `needed` what Kalamar needs now.
fn previous_update() -> FileCheck {
    let mut c = check("bad", "update_previous");
    c.params.push(("version".into(), PREVIOUS.display.into()));
    c.params.push(("needed".into(), CURRENT.display.into()));
    c
}

fn version_text(v: u64) -> String {
    format!("v{}", v >> 16)
}

pub fn check_base(path: &Path) -> FileCheck {
    let info = match inspect(path) {
        Ok(i) => i,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return check("bad", "file_missing"),
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData || e.kind() == std::io::ErrorKind::UnexpectedEof => return check("bad", "not_nsp"),
        Err(_) => return check("bad", "unreadable"),
    };
    let prog = program_nca(&info.entries);
    let mut c = match (&info.meta, prog) {
        (_, None) => check("bad", "no_program"),
        (_, Some(p)) if p.name == CURRENT.update_program_nca || p.name == PREVIOUS.update_program_nca || has_update_ticket(&info.entries) => check("bad", "base_is_update"),
        (Some(m), _) if m.title_id == TITLE_UPDATE => check("bad", "base_is_update"),
        (Some(m), _) if m.kind == "Application" && m.title_id == TITLE_BASE => check("ok", "base_ok"),
        (Some(m), _) if m.title_id != TITLE_BASE => {
            let mut c = check("bad", "other_title");
            c.params.push(("title".into(), format!("{:016X}", m.title_id)));
            c
        }
        (Some(_), _) => check("bad", "not_base"),
        (None, Some(_)) => check("warn", "base_unverified"),
    };
    c.size = Some(info.size);
    c.program_size = prog.map(|p| p.size);
    c
}

pub fn check_update(path: &Path) -> FileCheck {
    let info = match inspect(path) {
        Ok(i) => i,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return check("bad", "file_missing"),
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData || e.kind() == std::io::ErrorKind::UnexpectedEof => return check("bad", "not_nsp"),
        Err(_) => return check("bad", "unreadable"),
    };
    let prog = program_nca(&info.entries);
    let mut c = match (&info.meta, prog) {
        (_, None) => check("bad", "no_program"),
        (_, Some(p)) if p.name == CURRENT.update_program_nca => check("ok", "update_ok"),
        (Some(m), _) if m.kind == "Patch" && m.title_id == TITLE_UPDATE && m.version == CURRENT.update_version => check("ok", "update_ok"),
        // The previous version's update: named for what it is (it was the one Kalamar used before), not "another version".
        (_, Some(p)) if p.name == PREVIOUS.update_program_nca => previous_update(),
        (Some(m), _) if m.kind == "Patch" && m.title_id == TITLE_UPDATE && m.version == PREVIOUS.update_version => previous_update(),
        (Some(m), _) if m.kind == "Patch" && m.title_id == TITLE_UPDATE => {
            let mut c = check("bad", "update_wrong_version");
            c.params.push(("version".into(), version_text(m.version)));
            c.params.push(("needed".into(), CURRENT.display.into()));
            c
        }
        (Some(m), _) if m.title_id == TITLE_BASE => check("bad", "update_is_base"),
        (Some(m), _) => {
            let mut c = check("bad", "other_title");
            c.params.push(("title".into(), format!("{:016X}", m.title_id)));
            c
        }
        (None, Some(_)) => check("warn", "update_unverified"),
    };
    c.size = Some(info.size);
    c.program_size = prog.map(|p| p.size);
    c
}

/// Key names the extraction needs, by prefix (a numbered family needs at least one member).
const NEEDED_KEYS: &[&str] = &["header_key", "key_area_key_application_", "titlekek_", "master_key_"];

/// Checks prod.keys by its key names only: present, a text file of `name = hex` lines, with the families the
/// extraction needs. Values are checked for being hex but never kept.
pub fn check_keys(path: &Path) -> FileCheck {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return check("bad", "file_missing"),
        Err(_) => return check("bad", "unreadable"),
    };
    if meta.len() == 0 || meta.len() > 256 * 1024 {
        return check("bad", "keys_format");
    }
    let Ok(bytes) = std::fs::read(path) else { return check("bad", "unreadable") };
    let text = String::from_utf8_lossy(&bytes);
    let mut names: Vec<String> = Vec::new();
    let mut lines = 0;
    for l in text.lines() {
        let l = l.trim();
        if l.is_empty() || l.starts_with(';') || l.starts_with('#') {
            continue;
        }
        let Some((n, v)) = l.split_once('=') else { return check("bad", "keys_format") };
        let v = v.trim();
        if v.is_empty() || !v.chars().all(|c| c.is_ascii_hexdigit()) {
            return check("bad", "keys_format");
        }
        names.push(n.trim().to_ascii_lowercase());
        lines += 1;
    }
    let missing: Vec<&str> = NEEDED_KEYS.iter().copied().filter(|k| !names.iter().any(|n| if k.ends_with('_') { n.starts_with(k) } else { n == k })).collect();
    let mut c = if lines == 0 {
        check("bad", "keys_format")
    } else if missing.is_empty() {
        check("ok", "keys_ok")
    } else {
        let mut c = check("bad", "keys_incomplete");
        c.params.push(("missing".into(), missing.iter().map(|k| k.trim_end_matches('_')).collect::<Vec<_>>().join(", ")));
        c
    };
    c.params.push(("count".into(), lines.to_string()));
    c.size = Some(meta.len());
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nsp(files: &[(&str, &[u8])]) -> Vec<u8> {
        nsp_sized(&files.iter().map(|(n, d)| (*n, d.len() as u64, *d)).collect::<Vec<_>>())
    }

    /// Like [`nsp`], with the size field of each entry written as given (a crafted table).
    fn nsp_sized(files: &[(&str, u64, &[u8])]) -> Vec<u8> {
        let mut strs = Vec::new();
        let mut ents = Vec::new();
        let mut data = Vec::new();
        for (n, size, d) in files {
            let off = data.len() as u64;
            ents.extend_from_slice(&off.to_le_bytes());
            ents.extend_from_slice(&size.to_le_bytes());
            ents.extend_from_slice(&(strs.len() as u32).to_le_bytes());
            ents.extend_from_slice(&0u32.to_le_bytes());
            strs.extend_from_slice(n.as_bytes());
            strs.push(0);
            data.extend_from_slice(d);
        }
        while strs.len() % 16 != 0 {
            strs.push(0);
        }
        let mut out = b"PFS0".to_vec();
        out.extend_from_slice(&(files.len() as u32).to_le_bytes());
        out.extend_from_slice(&(strs.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend(ents);
        out.extend(strs);
        out.extend(data);
        out
    }

    /// A minimal XCI: a HEAD header, a root HFS0 with a `secure` partition, and one file in it. No game data.
    /// An HFS0 entry is offset (u64), size (u64), name offset (u32), hashed size (u32), then the hash.
    #[test]
    fn an_xci_lists_the_files_of_its_secure_partition() {
        fn entry(buf: &mut [u8], off: u64, size: u64, name: u32) {
            buf[0..8].copy_from_slice(&off.to_le_bytes());
            buf[8..16].copy_from_slice(&size.to_le_bytes());
            buf[16..20].copy_from_slice(&name.to_le_bytes());
        }
        let mut img = vec![0u8; 0x2000];
        img[0x100..0x104].copy_from_slice(b"HEAD");
        img[0x130..0x138].copy_from_slice(&0x200u64.to_le_bytes());
        // root HFS0 at 0x200: one `secure` entry (0x100 from the data area, 0x400 bytes).
        img[0x200..0x204].copy_from_slice(b"HFS0");
        img[0x204..0x208].copy_from_slice(&1u32.to_le_bytes());
        img[0x208..0x20c].copy_from_slice(&7u32.to_le_bytes()); // "secure\0"
        entry(&mut img[0x210..0x250], 0x100, 0x400, 0);
        img[0x250..0x257].copy_from_slice(b"secure\0");
        // its data area starts at 0x257; the secure entry points 0x100 into it, with one 0x100-byte fake NCA.
        let at = 0x200 + 16 + 64 + 7 + 0x100;
        img[at..at + 4].copy_from_slice(b"HFS0");
        img[at + 4..at + 8].copy_from_slice(&1u32.to_le_bytes());
        img[at + 8..at + 12].copy_from_slice(&9u32.to_le_bytes()); // "fake.nca\0"
        entry(&mut img[at + 16..at + 16 + 64], 0x100, 0x100, 0);
        img[at + 80..at + 89].copy_from_slice(b"fake.nca\0");
        let p = write("xci", &img);
        let info = inspect(&p).unwrap();
        assert_eq!(info.size, 0x2000);
        assert_eq!(info.entries.len(), 1);
        assert_eq!(info.entries[0].name, "fake.nca");
        assert_eq!(info.entries[0].size, 0x100);
        assert!(info.meta.is_none());
        assert_eq!(program_nca(&info.entries).unwrap().name, "fake.nca");
        std::fs::remove_file(&p).ok();
    }

    fn write(tag: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("s3l-nsp-{tag}-{}", crate::util::random_hex(4)));
        std::fs::write(&p, bytes).unwrap();
        p
    }

    const BASE_XML: &str = "<ContentMeta><Type>Application</Type><Id>0x0100c2500fc20000</Id><Version>0</Version></ContentMeta>";
    fn patch_xml(version: u64) -> String {
        format!("<ContentMeta><Type>Patch</Type><Id>0x0100c2500fc20800</Id><Version>{version}</Version></ContentMeta>")
    }
    const OLD_XML: &str = "<ContentMeta><Type>Patch</Type><Id>0x0100c2500fc20800</Id><Version>2621440</Version></ContentMeta>";

    #[test]
    fn base_and_update_are_recognised() {
        let base = write("b", &nsp(&[("x.cnmt.xml", BASE_XML.as_bytes()), ("8974fd227095cfb96e1191a0dac9756f.nca", &[0u8; 64]), ("y.cnmt.nca", &[0u8; 8])]));
        let c = check_base(&base);
        assert_eq!((c.status, c.code), ("ok", "base_ok"));
        assert_eq!(c.program_size, Some(64));
        let upd = write("u", &nsp(&[("x.cnmt.xml", patch_xml(CURRENT.update_version).as_bytes()), (CURRENT.update_program_nca, &[0u8; 32])]));
        assert_eq!(check_update(&upd).code, "update_ok");
        // swapped files are named as such
        assert_eq!(check_base(&upd).code, "base_is_update");
        assert_eq!(check_update(&base).code, "update_is_base");
        for p in [base, upd] {
            std::fs::remove_file(p).unwrap();
        }
    }

    #[test]
    fn the_previous_update_is_named_for_what_it_is() {
        // With its content-meta XML, with only its Program NCA's name, and given as the base game.
        let with_xml = write("p1", &nsp(&[("x.cnmt.xml", patch_xml(PREVIOUS.update_version).as_bytes()), ("0123.nca", &[0u8; 32])]));
        let by_nca = write("p2", &nsp(&[(PREVIOUS.update_program_nca, &[0u8; 32])]));
        for p in [&with_xml, &by_nca] {
            let c = check_update(p);
            assert_eq!((c.status, c.code), ("bad", "update_previous"));
            assert_eq!(c.params, [("version".to_string(), PREVIOUS.display.to_string()), ("needed".to_string(), CURRENT.display.to_string())]);
        }
        assert_eq!(check_base(&with_xml).code, "base_is_update");
        assert_eq!(check_base(&by_nca).code, "base_is_update");
        // The current update is still accepted by either mark.
        let now = write("p3", &nsp(&[("x.cnmt.xml", patch_xml(CURRENT.update_version).as_bytes()), ("0123.nca", &[0u8; 32])]));
        assert_eq!(check_update(&now).code, "update_ok");
        for p in [with_xml, by_nca, now] {
            std::fs::remove_file(p).unwrap();
        }
    }

    #[test]
    fn wrong_or_unknown_updates() {
        let old = write("o", &nsp(&[("x.cnmt.xml", OLD_XML.as_bytes()), ("0123.nca", &[0u8; 32])]));
        let c = check_update(&old);
        assert_eq!((c.status, c.code), ("bad", "update_wrong_version"));
        assert_eq!(c.params[0].1, "v40");
        assert_eq!(c.params[1], ("needed".to_string(), CURRENT.display.to_string()));
        // no XML in the dump: accepted, checked again after extraction
        let bare = write("n", &nsp(&[("0123.nca", &[0u8; 32])]));
        assert_eq!(check_update(&bare).status, "warn");
        assert_eq!(check_base(&bare).status, "warn");
        let junk = write("j", b"not a container at all");
        assert_eq!(check_update(&junk).code, "not_nsp");
        assert_eq!(check_update(Path::new("Z:/nope/none.nsp")).code, "file_missing");
        for p in [old, bare, junk] {
            std::fs::remove_file(p).unwrap();
        }
    }

    #[test]
    fn a_crafted_table_never_sizes_the_xml_read() {
        // 125 bytes: a first content-meta entry claiming 2^47 - 1 bytes, a second small one (the one located), a program.
        // The check used to allocate the first one's size while reading at the second one's offset (an abort).
        let f = nsp_sized(&[("a.cnmt.xml", 0x7FFF_FFFF_FFFF, b""), ("b.cnmt.xml", 4, b"<x/>"), ("c.nca", 1, b"z")]);
        assert_eq!(f.len(), 125);
        let p = write("alloc", &f);
        let c = check_base(&p);
        assert_eq!((c.status, c.code), ("warn", "base_unverified"));
        assert_eq!(check_update(&p).code, "update_unverified");
        // The located XML lies past the end of the file: a broken container, nothing allocated for it.
        std::fs::write(&p, nsp_sized(&[("x.cnmt.xml", 60_000, b"<ContentMeta>"), ("c.nca", 1, b"z")])).unwrap();
        assert_eq!(check_base(&p).code, "not_nsp");
        // An entry offset that overflows once the data start is added (a panic in debug builds) means no XML.
        let mut f = nsp(&[("x.cnmt.xml", BASE_XML.as_bytes()), ("c.nca", b"z")]);
        f[16..24].copy_from_slice(&(u64::MAX - 8).to_le_bytes());
        std::fs::write(&p, &f).unwrap();
        assert_eq!(check_base(&p).code, "base_unverified");
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    fn keys_are_checked_by_name_only() {
        let hex = "00".repeat(16);
        let good = format!("header_key = {hex}{hex}\nkey_area_key_application_00 = {hex}\ntitlekek_00 = {hex}\nmaster_key_00 = {hex}\n");
        let p = write("k", good.as_bytes());
        let c = check_keys(&p);
        assert_eq!(c.code, "keys_ok");
        // the result never carries a value
        assert!(!format!("{c:?}").contains(&hex));
        std::fs::write(&p, format!("header_key = {hex}\n")).unwrap();
        let c = check_keys(&p);
        assert_eq!(c.code, "keys_incomplete");
        assert!(c.params[0].1.contains("titlekek"));
        std::fs::write(&p, "hello world").unwrap();
        assert_eq!(check_keys(&p).code, "keys_format");
        std::fs::remove_file(p).unwrap();
    }
}
