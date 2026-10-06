//! The server's timetable for a joining game (see `omsi_net::ttdata`): a dedicated server
//! lists the `TTData` folder of its map at the start, and a game that joins it fetches that
//! folder before its world is made. The copy goes into a content folder of its own for the
//! session (`~/.openomsi/server-ttdata/<pid>`), the first content root: the map's `TTData`
//! resolves to it as a whole (`omsi_cfg::resolve_path`), so a line the server took off is
//! gone here as well. Files already fetched once are kept by their hash
//! (`~/.openomsi/server-ttdata/store`) and copied from there the next time.

use crate::Args;
use omsi_net::ttdata::{sha256_hex, Manifest};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

/// The map folder of `args.map` (`maps/<folder>/global.cfg`) and that folder on disk.
fn map_folder(args: &Args) -> Option<(String, PathBuf)> {
    let (_, cfg) = omsi_cfg::find_in_roots(&args.map)?;
    let dir = cfg.parent()?.to_path_buf();
    let name = dir.file_name()?.to_string_lossy().to_string();
    Some((name, dir))
}

/// A dedicated server: its map's timetable for the players who join.
pub fn publish(args: &Args) {
    let Some((name, dir)) = map_folder(args) else {
        log::warn!("timetable: the map's folder was not found; no timetable for the players");
        return;
    };
    let tt = omsi_cfg::resolve_path(&dir, "TTData");
    if omsi_net::ttdata::publish(&name, &tt).is_none() {
        log::info!("timetable: {} holds nothing to serve", tt.display());
    }
}

fn base_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".openomsi").join("server-ttdata"))
}

static SESSION_ROOT: Mutex<Option<PathBuf>> = Mutex::new(None);

/// A server's web gateway port when only its session's address is known (`web_port`'s
/// default).
const WEB_PORT: u16 = 27025;

/// What a fetch came to.
#[derive(Debug, Default)]
pub struct Report {
    pub files: usize,
    pub fetched: usize,
    pub version: String,
}

/// Where the server's gateway may answer, in the order to try: the WebSocket joined
/// through (`https://host` of `wss://host/ws`), else the address the player gave
/// (`omsi_net::ws::web_bases`, and over https for a name: a server behind a web proxy), and
/// the gateway's usual port on the session's address.
pub fn web_bases(joined_ws: Option<&str>, target: Option<&str>, host: std::net::SocketAddr) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |u: String| {
        if !out.contains(&u) {
            out.push(u);
        }
    };
    if joined_ws.is_some() {
        add(web_base(joined_ws, host));
        return out;
    }
    if let Some(t) = target.map(str::trim).filter(|t| !t.is_empty()) {
        for b in omsi_net::ws::web_bases(t) {
            add(b);
        }
        let name = t.trim_start_matches("https://").trim_start_matches("http://").split(['/', ':']).next().unwrap_or("");
        if !name.is_empty() && name.parse::<std::net::IpAddr>().is_err() && !name.starts_with('[') {
            add(format!("https://{name}"));
        }
    }
    add(web_base(None, host));
    out
}

/// The web address of the server joined (`https://host` of `wss://host/ws`), or the
/// gateway's usual port on the host's address.
pub fn web_base(joined_ws: Option<&str>, host: std::net::SocketAddr) -> String {
    match joined_ws {
        Some(u) => {
            let u = u.strip_suffix("/ws").unwrap_or(u);
            let u = u.split('?').next().unwrap_or(u);
            if let Some(rest) = u.strip_prefix("wss://") {
                format!("https://{rest}")
            } else if let Some(rest) = u.strip_prefix("ws://") {
                format!("http://{rest}")
            } else {
                u.to_string()
            }
        }
        None => format!("http://{}:{WEB_PORT}", host.ip()),
    }
}

fn get(agent: &ureq::Agent, url: &str, limit: u64) -> Result<Vec<u8>, String> {
    let r = agent.get(url).call().map_err(|e| match e {
        ureq::Error::Status(404, _) => "the server serves no timetable".to_string(),
        ureq::Error::Status(code, _) => format!("the server answered {code}"),
        ureq::Error::Transport(t) => format!("{UNREACHABLE}{t}"),
    })?;
    let mut body = Vec::new();
    std::io::Read::read_to_end(&mut std::io::Read::take(r.into_reader(), limit + 1), &mut body).map_err(|e| e.to_string())?;
    if body.len() as u64 > limit {
        return Err("too large".into());
    }
    Ok(body)
}

/// What an error of an address that did not answer starts with.
const UNREACHABLE: &str = "unreachable: ";

/// A joining game: the server's timetable, from the first of `bases` that gives it. What
/// went wrong is the first answer that was not a timetable (another web server may answer
/// at one of the addresses), else why the last could not be reached.
pub fn fetch_any(bases: &[String]) -> Result<Report, String> {
    let mut answered: Option<String> = None;
    let mut last = "no address to ask".to_string();
    for b in bases {
        match fetch(b) {
            Ok(r) => return Ok(r),
            Err(e) if e.starts_with(UNREACHABLE) => last = format!("{b}: {e}"),
            Err(e) => {
                answered.get_or_insert(format!("{b}: {e}"));
            }
        }
    }
    Err(answered.unwrap_or(last))
}

/// A joining game: the server's timetable for this session, from the gateway at `base`.
pub fn fetch(base: &str) -> Result<Report, String> {
    if omsi_cfg::env::var_os("OMSI_NO_SERVER_TTDATA").is_some() {
        return Err("switched off (OMSI_NO_SERVER_TTDATA)".into());
    }
    let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(5)).timeout(Duration::from_secs(30)).build();
    let list = get(&agent, &format!("{base}/ttdata"), 4 << 20)?;
    let m = Manifest::parse(&String::from_utf8_lossy(&list))?;
    // (the server's map, as this game spells its folder: the session goes onto it after
    // this, `lan::take_host_map`)
    let (_, cfg) = omsi_cfg::find_in_roots(&format!("maps/{}/global.cfg", m.map)).ok_or_else(|| format!("the server's map {} is not installed here", m.map))?;
    let mine = cfg.parent().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().to_string()).ok_or("no map folder")?;
    let base_dir = base_dir().ok_or("no home folder")?;
    let store = base_dir.join("store");
    let root = base_dir.join(std::process::id().to_string());
    let dir = root.join("maps").join(&mine).join("TTData");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&store).map_err(|e| e.to_string())?;
    let mut report = Report { files: m.entries.len(), version: m.version.clone(), ..Default::default() };
    for (k, e) in m.entries.iter().enumerate() {
        let kept = store.join(&e.sha256);
        let data = match std::fs::read(&kept).ok().filter(|d| sha256_hex(d) == e.sha256) {
            Some(d) => d,
            None => {
                let d = get(&agent, &format!("{base}/ttdata/{k}"), e.size)?;
                if d.len() as u64 != e.size || sha256_hex(&d) != e.sha256 {
                    return Err(format!("{} came damaged", e.name));
                }
                let _ = std::fs::write(&kept, &d);
                report.fetched += 1;
                d
            }
        };
        std::fs::write(dir.join(&e.name), &data).map_err(|e| e.to_string())?;
    }
    omsi_cfg::mark_sandbox(root.clone());
    omsi_cfg::add_content_root_first(root.clone());
    *SESSION_ROOT.lock().unwrap_or_else(|e| e.into_inner()) = Some(root);
    Ok(report)
}

/// The session is over: the server's timetable goes, the map's own is back.
pub fn clean_up() {
    let taken = SESSION_ROOT.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(dir) = taken {
        omsi_cfg::remove_content_root(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        log::info!("timetable: the server's timetable of this session was removed");
    }
}

/// The copies of games no longer running (one that ended without cleaning up).
pub fn remove_stale() {
    let Some(base) = base_dir() else { return };
    let Ok(rd) = std::fs::read_dir(&base) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if let Ok(pid) = name.parse::<u32>() {
            if pid != std::process::id() && !crate::lan_mods::process_alive(pid) {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Against a running server (OMSI_TTDATA_TEST_URL, OMSI_TTDATA_TEST_ROOT: an OMSI 2
    /// folder with its map): the folder comes, the map's `TTData` resolves to it, and goes.
    #[test]
    #[ignore]
    fn a_running_servers_timetable_comes_and_goes() {
        let url = std::env::var("OMSI_TTDATA_TEST_URL").unwrap();
        omsi_cfg::add_content_root(PathBuf::from(std::env::var("OMSI_TTDATA_TEST_ROOT").unwrap()));
        let r = fetch(&url).unwrap();
        assert!(r.files > 0);
        let list = Manifest::parse(&String::from_utf8(ureq::get(&format!("{url}/ttdata")).call().unwrap().into_string().unwrap().into_bytes()).unwrap()).unwrap();
        let (_, cfg) = omsi_cfg::find_in_roots(&format!("maps/{}/global.cfg", list.map)).unwrap();
        let tt = omsi_cfg::resolve_path(cfg.parent().unwrap(), "TTData");
        assert!(tt.starts_with(SESSION_ROOT.lock().unwrap().as_ref().unwrap()), "{}", tt.display());
        // the whole folder, nothing of the map's own besides
        assert_eq!(omsi_cfg::vfs::read_dir_paths(&tt).len(), list.entries.len());
        let again = fetch(&url).unwrap();
        assert_eq!(again.fetched, 0, "kept by their hash the second time");
        clean_up();
        let own = omsi_cfg::resolve_path(cfg.parent().unwrap(), "TTData");
        assert!(!own.to_string_lossy().contains("server-ttdata"), "{}", own.display());
    }

    #[test]
    fn the_web_address_of_the_server_joined() {
        let h: std::net::SocketAddr = "203.0.113.5:27015".parse().unwrap();
        assert_eq!(web_base(Some("wss://play.example.org/ws"), h), "https://play.example.org");
        assert_eq!(web_base(Some("ws://10.0.0.2:27025/ws"), h), "http://10.0.0.2:27025");
        assert_eq!(web_base(None, h), "http://203.0.113.5:27025");
        let b = web_bases(None, Some("play.example.org"), h);
        assert!(b.contains(&"https://play.example.org".to_string()), "{b:?}");
        assert_eq!(b.last().unwrap(), "http://203.0.113.5:27025");
        assert_eq!(web_bases(Some("wss://play.example.org/ws"), Some("x"), h), ["https://play.example.org"]);
        assert!(!web_bases(None, Some("203.0.113.5:27015"), h).iter().any(|u| u.starts_with("https://203")), "an IP has no certificate");
    }
}
