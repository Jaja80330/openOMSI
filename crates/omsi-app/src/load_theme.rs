//! The loading screen's look, set from the edition's web panel: a background picture, a logo,
//! a line under the map's name, the tips and the accent colour. The game asks the panel for it
//! once a start (`THEME_URL`, a small JSON file naming the pictures) on a thread of its own and
//! keeps the last answer on disk, so the screen has its look at once the next time - before the
//! panel answers, and without a connection at all. Nothing set, or the panel out of reach with
//! nothing kept: the plain screen (black, the built-in tips).
//!
//! The JSON (every key optional):
//!
//! ```json
//! { "enabled": true, "background": "https://…/bg.jpg", "logo": "https://…/logo.png",
//!   "dim": 0.45, "subtitle": "…", "tips": ["…", "…"], "tip_seconds": 8, "accent": "#f2a900" }
//! ```
//!
//! The pictures are PNG or JPEG; an address starting with `/` is taken on the panel's own
//! host. `OMSI_LOADING_THEME` names another panel (empty: none at all).

use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// The edition's panel (TRANSDEV TLA): the loading screen's look.
pub const THEME_URL: &str = "http://51.254.143.223:3000/public/omsi/loading.json";

/// The JSON's size at the most, and a picture's (bytes).
const MAX_JSON: u64 = 64 * 1024;
const MAX_PICTURE: u64 = 8 * 1024 * 1024;
/// The background is kept at this size at the most (it is drawn over the whole window), the
/// logo at this one (pixels).
const MAX_BACKGROUND: (u32, u32) = (2560, 1440);
const MAX_LOGO: (u32, u32) = (1024, 512);
/// Tips at the most, and a tip's length (characters).
const MAX_TIPS: usize = 40;
const MAX_TIP_CHARS: usize = 200;

/// The look the panel gave.
pub struct Theme {
    /// Which answer this is (the JSON's hash): the pictures go to the graphics card again
    /// when it changes.
    pub generation: u64,
    pub background: Option<image::RgbaImage>,
    pub logo: Option<image::RgbaImage>,
    /// How much darker the background is drawn (0 to 0.9), for the texts over it.
    pub dim: f32,
    /// The line under the map's name (empty: none).
    pub subtitle: String,
    /// The tips in place of the game's (empty: the game's).
    pub tips: Vec<String>,
    /// How long a tip stands (seconds).
    pub tip_seconds: f32,
    /// The bar's colour (none: the interface's accent).
    pub accent: Option<[u8; 3]>,
}

/// What the JSON says, before the pictures are fetched.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Spec {
    pub enabled: bool,
    pub background: String,
    pub logo: String,
    pub dim: f32,
    pub subtitle: String,
    pub tips: Vec<String>,
    pub tip_seconds: f32,
    pub accent: Option<[u8; 3]>,
}

static CURRENT: Mutex<Option<Arc<Theme>>> = Mutex::new(None);
static STARTED: OnceLock<()> = OnceLock::new();

/// The look now (none: the plain screen).
pub fn current() -> Option<Arc<Theme>> {
    CURRENT.lock().ok().and_then(|c| c.clone())
}

fn set(theme: Option<Theme>) {
    if let Ok(mut c) = CURRENT.lock() {
        *c = theme.map(Arc::new);
    }
}

/// The panel's address (`OMSI_LOADING_THEME`, or the edition's).
fn url() -> Option<String> {
    let u = match omsi_cfg::env::var_os("OMSI_LOADING_THEME") {
        Some(v) => v.to_string_lossy().trim().to_string(),
        None => THEME_URL.to_string(),
    };
    (u.starts_with("http://") || u.starts_with("https://")).then_some(u)
}

/// Take the kept look, then ask the panel for the current one (on a thread: the screens never
/// wait for it). Once a start; the game and the launcher may both ask.
pub fn start() {
    if STARTED.set(()).is_err() {
        return;
    }
    let Some(url) = url() else { return };
    let spawned = std::thread::Builder::new().name("loading theme".into()).spawn(move || {
        let kept = read_kept();
        let kept_json = kept.as_ref().map(|k| k.0.clone());
        if let Some((json, theme)) = kept {
            log::info!("loading theme: the kept one ({} bytes)", json.len());
            set(Some(theme));
        }
        match fetch(&url, kept_json.as_deref()) {
            Ok(Some((json, theme))) => {
                log::info!("loading theme: from {url}");
                keep(&json, theme.as_ref());
                set(theme);
            }
            Ok(None) => log::info!("loading theme: unchanged"),
            Err(e) => log::warn!("loading theme: {url}: {e}"),
        }
    });
    if let Err(e) = spawned {
        log::warn!("loading theme: no thread: {e}");
    }
}

/// The panel's answer: None when it is the kept one; `(json, None)` when the panel has no look
/// set (the kept one goes).
#[allow(clippy::type_complexity)]
fn fetch(url: &str, kept: Option<&str>) -> Result<Option<(String, Option<Theme>)>, String> {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(8)).user_agent("openOMSI").build();
    let json = String::from_utf8(get(&agent, url, MAX_JSON)?).map_err(|_| "not UTF-8".to_string())?;
    if kept == Some(json.as_str()) {
        return Ok(None);
    }
    let spec = parse(&json).ok_or("not a loading theme")?;
    if !spec.enabled {
        return Ok(Some((json, None)));
    }
    let picture = |addr: &str, max: (u32, u32)| -> Option<(Vec<u8>, image::RgbaImage)> {
        if addr.is_empty() {
            return None;
        }
        let full = resolve(url, addr)?;
        match get(&agent, &full, MAX_PICTURE).and_then(|b| decode(&b, max).map(|i| (b, i))) {
            Ok(p) => Some(p),
            Err(e) => {
                log::warn!("loading theme: {full}: {e}");
                None
            }
        }
    };
    let bg = picture(&spec.background, MAX_BACKGROUND);
    let logo = picture(&spec.logo, MAX_LOGO);
    keep_picture("background", bg.as_ref().map(|p| p.0.as_slice()));
    keep_picture("logo", logo.as_ref().map(|p| p.0.as_slice()));
    Ok(Some((json.clone(), Some(theme_of(&json, spec, bg.map(|p| p.1), logo.map(|p| p.1))))))
}

fn get(agent: &ureq::Agent, url: &str, max: u64) -> Result<Vec<u8>, String> {
    let r = agent.get(url).call().map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    r.into_reader().take(max + 1).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    if buf.len() as u64 > max {
        return Err(format!("more than {max} bytes"));
    }
    Ok(buf)
}

/// A PNG or a JPEG, made no bigger than `max`.
fn decode(bytes: &[u8], max: (u32, u32)) -> Result<image::RgbaImage, String> {
    let format = image::guess_format(bytes).map_err(|e| e.to_string())?;
    if !matches!(format, image::ImageFormat::Png | image::ImageFormat::Jpeg) {
        return Err(format!("{format:?} is not a PNG or a JPEG"));
    }
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().map_err(|e| e.to_string())?;
    let (w, h) = (img.width(), img.height());
    let k = (max.0 as f32 / w as f32).min(max.1 as f32 / h as f32);
    let img = if k < 1.0 { img.resize(((w as f32 * k) as u32).max(1), ((h as f32 * k) as u32).max(1), image::imageops::FilterType::Triangle) } else { img };
    Ok(img.to_rgba8())
}

/// `addr` as a full address: as it is, or on the panel's host when it starts with `/`.
pub(crate) fn resolve(base: &str, addr: &str) -> Option<String> {
    let a = addr.trim();
    if a.starts_with("http://") || a.starts_with("https://") {
        return Some(a.to_string());
    }
    if !a.starts_with('/') || a.starts_with("//") {
        return None;
    }
    let scheme_end = base.find("://")? + 3;
    let host_end = base[scheme_end..].find('/').map(|i| scheme_end + i).unwrap_or(base.len());
    Some(format!("{}{a}", &base[..host_end]))
}

/// The JSON read (None: not an object).
pub(crate) fn parse(json: &str) -> Option<Spec> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let o = v.as_object()?;
    let text = |k: &str| o.get(k).and_then(|x| x.as_str()).map(|s| s.trim().to_string()).unwrap_or_default();
    let num = |k: &str| o.get(k).and_then(|x| x.as_f64()).filter(|x| x.is_finite());
    let tips = o
        .get("tips")
        .and_then(|t| t.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str()).map(|s| s.trim().chars().take(MAX_TIP_CHARS).collect::<String>()).filter(|s| !s.is_empty()).take(MAX_TIPS).collect())
        .unwrap_or_default();
    Some(Spec {
        enabled: o.get("enabled").and_then(|x| x.as_bool()).unwrap_or(true),
        background: text("background"),
        logo: text("logo"),
        dim: num("dim").unwrap_or(0.45).clamp(0.0, 0.9) as f32,
        subtitle: text("subtitle").chars().take(MAX_TIP_CHARS).collect(),
        tips,
        tip_seconds: num("tip_seconds").unwrap_or(8.0).clamp(3.0, 60.0) as f32,
        accent: parse_color(&text("accent")),
    })
}

/// `#rrggbb` (or `rrggbb`).
pub(crate) fn parse_color(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let c = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([c(0)?, c(2)?, c(4)?])
}

fn theme_of(json: &str, spec: Spec, background: Option<image::RgbaImage>, logo: Option<image::RgbaImage>) -> Theme {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    json.hash(&mut h);
    Theme { generation: h.finish(), background, logo, dim: spec.dim, subtitle: spec.subtitle, tips: spec.tips, tip_seconds: spec.tip_seconds, accent: spec.accent }
}

// --- kept on disk: ~/.openomsi/cache/loading-theme/ -------------------------------------------

fn dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".openomsi").join("cache").join("loading-theme"))
}

fn read_kept() -> Option<(String, Theme)> {
    let d = dir()?;
    let json = std::fs::read_to_string(d.join("theme.json")).ok()?;
    let spec = parse(&json)?;
    if !spec.enabled {
        return None;
    }
    let pic = |name: &str, max| std::fs::read(d.join(name)).ok().and_then(|b| decode(&b, max).ok());
    let bg = if spec.background.is_empty() { None } else { pic("background", MAX_BACKGROUND) };
    let logo = if spec.logo.is_empty() { None } else { pic("logo", MAX_LOGO) };
    Some((json.clone(), theme_of(&json, spec, bg, logo)))
}

fn keep(json: &str, theme: Option<&Theme>) {
    let Some(d) = dir() else { return };
    let _ = std::fs::create_dir_all(&d);
    if theme.is_none() {
        let _ = std::fs::remove_file(d.join("background"));
        let _ = std::fs::remove_file(d.join("logo"));
    }
    if let Err(e) = std::fs::write(d.join("theme.json"), json) {
        log::warn!("loading theme: not kept: {e}");
    }
}

fn keep_picture(name: &str, bytes: Option<&[u8]>) {
    let Some(d) = dir() else { return };
    let _ = std::fs::create_dir_all(&d);
    let p = d.join(name);
    let _ = match bytes {
        Some(b) => std::fs::write(p, b),
        None => std::fs::remove_file(p),
    };
}

// --- for the screens ---------------------------------------------------------------------------

/// The tip to show now: the panel's, one after another every `tip_seconds`; None: the
/// game's own (`ui::load_tip`).
pub fn tip_now() -> Option<String> {
    static T0: OnceLock<std::time::Instant> = OnceLock::new();
    let theme = current()?;
    if theme.tips.is_empty() {
        return None;
    }
    let t0 = *T0.get_or_init(std::time::Instant::now);
    let n = (t0.elapsed().as_secs_f32() / theme.tip_seconds) as usize;
    // (a different first tip each start)
    let first = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as usize).unwrap_or(0) / 100_000;
    Some(theme.tips[(first + n) % theme.tips.len()].clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_json_is_read() {
        let s = parse(r##"{"background":"/public/omsi/bg.jpg?v=3","dim":2,"subtitle":" Bienvenue ","tips":["a"," ",3,"b"],"tip_seconds":1,"accent":"#F2A900"}"##).unwrap();
        assert!(s.enabled);
        assert_eq!(s.background, "/public/omsi/bg.jpg?v=3");
        assert_eq!(s.dim, 0.9);
        assert_eq!(s.subtitle, "Bienvenue");
        assert_eq!(s.tips, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(s.tip_seconds, 3.0);
        assert_eq!(s.accent, Some([0xf2, 0xa9, 0x00]));
        assert!(!parse(r#"{"enabled":false}"#).unwrap().enabled);
        assert!(parse("[1]").is_none());
        assert!(parse("not json").is_none());
    }

    #[test]
    fn colours_and_addresses() {
        assert_eq!(parse_color("336699"), Some([0x33, 0x66, 0x99]));
        assert_eq!(parse_color("#36f"), None);
        assert_eq!(parse_color("#zzzzzz"), None);
        let base = "http://1.2.3.4:3000/public/omsi/loading.json";
        assert_eq!(resolve(base, "/public/omsi/bg.jpg").as_deref(), Some("http://1.2.3.4:3000/public/omsi/bg.jpg"));
        assert_eq!(resolve(base, "https://x.fr/a.png").as_deref(), Some("https://x.fr/a.png"));
        assert_eq!(resolve(base, "bg.jpg"), None);
        assert_eq!(resolve(base, "//evil/x.png"), None);
        assert_eq!(resolve("https://panel.fr", "/a.png").as_deref(), Some("https://panel.fr/a.png"));
    }

    #[test]
    fn pictures_are_made_small_enough() {
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(image::RgbaImage::new(400, 100)).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let img = decode(&png, (200, 200)).unwrap();
        assert_eq!(img.dimensions(), (200, 50));
        assert!(decode(b"GIF89a....", (10, 10)).is_err());
    }
}
