//! A dedicated server's timetable for the players who join: the `TTData` folder of the map it
//! runs (lines, trips, tracks, stops) goes to every joining game, which plays with it for the
//! session instead of its own copy - the server's dispatch edits its timetable, and the
//! players' duties, departure boards and IBIS must be the same as its AI buses'.
//!
//! The web gateway serves `GET /ttdata` (the list) and `GET /ttdata/<n>` (file `n` of the
//! list). Nothing else is served: no path a client names is ever opened, only the files the
//! server listed itself, plain names in the one folder.
//!
//! The list is text: `OMSITTDATA/1 <version> <map folder>` then a line per file,
//! `<size> <sha256> <name>`. The version is the SHA-256 of the file lines: the same folder
//! has the same version, a changed file another one.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const MAGIC: &str = "OMSITTDATA/1";
/// The most one file may have, the most files and the most bytes in all.
pub const MAX_FILE: u64 = 16 << 20;
pub const MAX_FILES: usize = 5000;
pub const MAX_TOTAL: u64 = 256 << 20;

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub size: u64,
    pub sha256: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub version: String,
    pub map: String,
    pub entries: Vec<Entry>,
}

/// A file name that may be in the list: a plain name in the folder, nothing that climbs out
/// of it or means something else on another system.
pub fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && name != "."
        && name != ".."
        && !name.starts_with('.')
        && !name.chars().any(|c| matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control())
        && !name.ends_with(' ')
        && !name.ends_with('.')
}

/// A map folder name as the list carries it (one word of the first line may not hold a
/// space: they go as `%20`, a `%` as `%25`).
fn map_out(map: &str) -> String {
    map.replace('%', "%25").replace(' ', "%20")
}

fn map_in(map: &str) -> String {
    map.replace("%20", " ").replace("%25", "%")
}

pub fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

impl Manifest {
    fn version_of(entries: &[Entry]) -> String {
        let mut h = Sha256::new();
        for e in entries {
            h.update(format!("{} {} {}\n", e.size, e.sha256, e.name).as_bytes());
        }
        hex(&h.finalize())[..16].to_string()
    }

    pub fn to_text(&self) -> String {
        let mut s = format!("{MAGIC} {} {}\n", self.version, map_out(&self.map));
        for e in &self.entries {
            s.push_str(&format!("{} {} {}\n", e.size, e.sha256, e.name));
        }
        s
    }

    /// A list as a server sent it; refused whole when anything in it is out of bounds.
    pub fn parse(text: &str) -> Result<Manifest, String> {
        let mut lines = text.lines();
        let head = lines.next().ok_or("empty list")?;
        let mut w = head.split(' ');
        if w.next() != Some(MAGIC) {
            return Err("not a timetable list".into());
        }
        let version = w.next().ok_or("no version")?.to_string();
        let map = map_in(w.next().ok_or("no map")?);
        if !name_ok(&map) {
            return Err("bad map folder".into());
        }
        let mut entries = Vec::new();
        let mut total = 0u64;
        let mut seen = std::collections::HashSet::new();
        for l in lines.filter(|l| !l.is_empty()) {
            let mut p = l.splitn(3, ' ');
            let size: u64 = p.next().and_then(|x| x.parse().ok()).ok_or("bad size")?;
            let sha256 = p.next().ok_or("no hash")?.to_string();
            let name = p.next().ok_or("no name")?.to_string();
            if size > MAX_FILE || sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) || !name_ok(&name) {
                return Err(format!("refused entry: {name}"));
            }
            if !seen.insert(name.to_lowercase()) {
                return Err(format!("{name} twice"));
            }
            total += size;
            entries.push(Entry { size, sha256, name });
        }
        if entries.len() > MAX_FILES || total > MAX_TOTAL {
            return Err("the list is too large".into());
        }
        if Manifest::version_of(&entries) != version {
            return Err("the list does not match its version".into());
        }
        Ok(Manifest { version, map, entries })
    }
}

// -------------------------------------------------------------------------------------
// the server

struct Served {
    manifest: Manifest,
    text: String,
    files: Vec<PathBuf>,
}

static SERVED: Mutex<Option<Served>> = Mutex::new(None);

/// List `dir` (the `TTData` folder of the map in folder `map`) for the joining games. The
/// version served, or None when there is nothing to serve.
pub fn publish(map: &str, dir: &Path) -> Option<String> {
    let mut names: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| (e.file_name().to_string_lossy().to_string(), e.path()))
        .filter(|(n, _)| name_ok(n))
        .collect();
    names.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    let mut entries = Vec::new();
    let mut files = Vec::new();
    let mut total = 0u64;
    for (name, path) in names {
        let Ok(data) = std::fs::read(&path) else { continue };
        if data.len() as u64 > MAX_FILE {
            log::warn!("timetable: {name} is too large to go to the players");
            continue;
        }
        total += data.len() as u64;
        if entries.len() >= MAX_FILES || total > MAX_TOTAL {
            log::warn!("timetable: the folder is too large; the rest does not go to the players");
            break;
        }
        entries.push(Entry { size: data.len() as u64, sha256: sha256_hex(&data), name });
        files.push(path);
    }
    if entries.is_empty() || !name_ok(map) {
        *SERVED.lock().unwrap_or_else(|e| e.into_inner()) = None;
        return None;
    }
    let manifest = Manifest { version: Manifest::version_of(&entries), map: map.to_string(), entries };
    let text = manifest.to_text();
    let v = manifest.version.clone();
    log::info!("timetable: {} files of {} go to the joining players (version {v})", files.len(), dir.display());
    *SERVED.lock().unwrap_or_else(|e| e.into_inner()) = Some(Served { manifest, text, files });
    Some(v)
}

/// The version served (empty: none), for `/status`.
pub fn version() -> String {
    SERVED.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|s| s.manifest.version.clone()).unwrap_or_default()
}

/// The gateway's answer to `GET /ttdata[/<n>]`: (status, content type, body).
pub fn answer(path: &str) -> (&'static str, &'static str, Vec<u8>) {
    let served = SERVED.lock().unwrap_or_else(|e| e.into_inner());
    let Some(s) = served.as_ref() else {
        return ("404 Not Found", "text/plain", b"this server serves no timetable".to_vec());
    };
    let rest = path.trim_start_matches("/ttdata").trim_start_matches('/');
    if rest.is_empty() {
        return ("200 OK", "text/plain; charset=utf-8", s.text.clone().into_bytes());
    }
    let Some(k) = rest.parse::<usize>().ok().filter(|&k| k < s.files.len()) else {
        return ("404 Not Found", "text/plain", b"no such file".to_vec());
    };
    match std::fs::read(&s.files[k]) {
        Ok(d) if d.len() as u64 <= MAX_FILE => ("200 OK", "application/octet-stream", d),
        _ => ("404 Not Found", "text/plain", b"gone".to_vec()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_stay_in_the_folder() {
        assert!(name_ok("11-11s.ttl"));
        assert!(name_ok("11_AM_HC_PETIT VILTAIN.ttp"));
        for bad in ["", ".", "..", "../x.ttl", "a/b.ttp", "a\\b.ttp", "C:x", ".hidden", "x.ttl.", "x "] {
            assert!(!name_ok(bad), "{bad}");
        }
    }

    #[test]
    fn a_folder_is_listed_served_and_read_back() {
        let dir = std::env::temp_dir().join(format!("omsi_ttdata_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("11-11s.ttl"), b"[newtour]\n").unwrap();
        std::fs::write(dir.join("A B.ttp"), b"[station]\n").unwrap();
        std::fs::write(dir.join("sub").join("x.ttp"), b"no").unwrap();
        let v = publish("Grand Paris-Moulon", &dir).unwrap();
        assert_eq!(version(), v);
        // (the NEROSY Leitstelle works the version out the same way, in Python)
        assert_eq!(v, "acbb0a7ee1deeb92");
        let (status, _, body) = answer("/ttdata");
        assert_eq!(status, "200 OK");
        let m = Manifest::parse(&String::from_utf8(body).unwrap()).unwrap();
        assert_eq!(m.map, "Grand Paris-Moulon");
        assert_eq!(m.version, v);
        assert_eq!(m.entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["11-11s.ttl", "A B.ttp"]);
        let (_, _, f) = answer("/ttdata/1");
        assert_eq!(f, b"[station]\n");
        assert_eq!(sha256_hex(&f), m.entries[1].sha256);
        assert_eq!(answer("/ttdata/2").0, "404 Not Found");
        assert_eq!(answer("/ttdata/../x").0, "404 Not Found");
        // a changed file: another version
        std::fs::write(dir.join("A B.ttp"), b"[station]\r\n").unwrap();
        assert_ne!(publish("Grand Paris-Moulon", &dir).unwrap(), v);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_list_out_of_bounds_is_refused() {
        let ok = Manifest { version: String::new(), map: "M".into(), entries: vec![Entry { size: 3, sha256: "a".repeat(64), name: "x.ttl".into() }] };
        let ok = Manifest { version: Manifest::version_of(&ok.entries), ..ok };
        assert!(Manifest::parse(&ok.to_text()).is_ok());
        assert!(Manifest::parse(&ok.to_text().replace("x.ttl", "../x.ttl")).is_err());
        assert!(Manifest::parse(&ok.to_text().replace(&ok.version, "0000000000000000")).is_err());
        assert!(Manifest::parse("HELLO 1 M\n").is_err());
    }
}
