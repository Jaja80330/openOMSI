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
//!   "dim": 0.45, "subtitle": "…", "tips": ["…", "…"], "tip_seconds": 8, "accent": "#f2a900",
//!   "show_title": true, "title": "", "show_bar": true, "bar_height": 3, "bar_width": "narrow",
//!   "show_percent": true, "show_tips": true, "tips_random": true, "logo_height": 120,
//!   "text_color": "#ececec", "layout": "center", "template": "line", "panel_color": "#0b1424",
//!   "line_label": "TLA" }
//! ```
//!
//! `title` replaces the map's name (empty: the name), `bar_width` is `narrow` or `wide`,
//! `layout` `center` or `bottom` (the name, the bar and the tip near the window's bottom);
//! `show_tips: false` shows no tip at all (no tips given: the game's own). `template` is the
//! layout: `classic` (in the middle, as without a look), `line` (a band along the bottom with
//! the line's diagram), `sign` (a destination sign) or `poster` (a column down the left edge);
//! `panel_color` is the band's, sign's or column's colour, `line_label` the sign's line number.
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
    /// Which parts show, how large and where.
    pub style: Style,
}

/// Which parts of the loading screen show, how large and where (the plain screen's without a
/// look).
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    /// The map's name (or `title`) over the bar.
    pub show_title: bool,
    /// In place of the map's name (empty: the name).
    pub title: String,
    pub show_bar: bool,
    /// The bar's thickness and whether it is wide (60 % of the window) or narrow (points).
    pub bar_height: f32,
    pub bar_wide: bool,
    pub show_percent: bool,
    /// No tip at all when off.
    pub show_tips: bool,
    /// The tips in any order (else as listed).
    pub tips_random: bool,
    /// The logo's height at the most (points).
    pub logo_height: f32,
    /// The texts' colour (none: the interface's).
    pub text_color: Option<[u8; 3]>,
    /// The name, the bar and the tip near the window's bottom (else in its middle; `classic`).
    pub bottom: bool,
    /// The layout.
    pub template: Template,
    /// The colour of the band, the sign's box or the column (none: the network's night blue).
    pub panel_color: Option<[u8; 3]>,
    /// The line's number on the destination sign (empty: TLA).
    pub line_label: String,
}

/// The loading screen's layouts (`shell::loading`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Template {
    /// In the middle: the name, a thin bar and the tip.
    #[default]
    Classic,
    /// A solid band along the bottom: the destination, and the line's diagram for the bar.
    Line,
    /// A bus's destination sign: the line's number, the name in its lamps, a row of lamps.
    Sign,
    /// A solid column down the left edge: the name very large, the percentage in large figures.
    Poster,
}

impl Default for Style {
    fn default() -> Style {
        Style { show_title: true, title: String::new(), show_bar: true, bar_height: 3.0, bar_wide: false, show_percent: true, show_tips: true, tips_random: true, logo_height: 120.0, text_color: None, bottom: false, template: Template::Classic, panel_color: None, line_label: String::new() }
    }
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
    pub style: Style,
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
    let flag = |k: &str, d: bool| o.get(k).and_then(|x| x.as_bool()).unwrap_or(d);
    let d = Style::default();
    let style = Style {
        show_title: flag("show_title", d.show_title),
        title: text("title").chars().take(MAX_TIP_CHARS).collect(),
        show_bar: flag("show_bar", d.show_bar),
        bar_height: num("bar_height").map(|v| v as f32).unwrap_or(d.bar_height).clamp(1.0, 16.0).round(),
        bar_wide: text("bar_width") == "wide",
        show_percent: flag("show_percent", d.show_percent),
        show_tips: flag("show_tips", d.show_tips),
        tips_random: flag("tips_random", d.tips_random),
        logo_height: num("logo_height").map(|v| v as f32).unwrap_or(d.logo_height).clamp(24.0, 320.0),
        text_color: parse_color(&text("text_color")),
        bottom: text("layout") == "bottom",
        template: match text("template").as_str() {
            "line" => Template::Line,
            "sign" => Template::Sign,
            "poster" => Template::Poster,
            _ => Template::Classic,
        },
        panel_color: parse_color(&text("panel_color")),
        line_label: text("line_label").chars().take(6).collect(),
    };
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
        style,
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
    Theme { generation: h.finish(), background, logo, dim: spec.dim, subtitle: spec.subtitle, tips: spec.tips, tip_seconds: spec.tip_seconds, accent: spec.accent, style: spec.style }
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

/// The tip to show now: the panel's, one after another every `tip_seconds` (empty: the panel
/// shows none); None: the game's own (`ui::load_tip`).
pub fn tip_now() -> Option<String> {
    static T0: OnceLock<std::time::Instant> = OnceLock::new();
    static SEED: OnceLock<u64> = OnceLock::new();
    let theme = current()?;
    if !theme.style.show_tips {
        return Some(String::new());
    }
    if theme.tips.is_empty() {
        return None;
    }
    let t0 = *T0.get_or_init(std::time::Instant::now);
    let n = (t0.elapsed().as_secs_f32() / theme.tip_seconds) as usize;
    let seed = *SEED.get_or_init(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1) | 1);
    Some(theme.tips[tip_index(theme.tips.len(), n, theme.style.tips_random, seed)].clone())
}

/// The `n`th tip shown of `len`: as listed, or in an order of their own each start (every tip
/// once a round, before any comes again).
pub(crate) fn tip_index(len: usize, n: usize, random: bool, seed: u64) -> usize {
    if !random || len < 2 {
        return n % len.max(1);
    }
    let mut order: Vec<usize> = (0..len).collect();
    // (shuffled by the start's seed and the round's number: xorshift)
    let mut x = (seed ^ ((n / len) as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15)) | 1;
    for i in (1..len).rev() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        order.swap(i, (x % (i as u64 + 1)) as usize);
    }
    order[n % len]
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
        // the parts: as the plain screen when not said
        assert_eq!(s.style, Style::default());
        let t = parse(r##"{"show_title":false,"title":"TLA","show_bar":false,"bar_height":99,"bar_width":"wide","show_percent":false,"show_tips":false,"tips_random":false,"logo_height":5,"text_color":"#ffffff","layout":"bottom"}"##).unwrap().style;
        assert_eq!(t, Style { show_title: false, title: "TLA".into(), show_bar: false, bar_height: 16.0, bar_wide: true, show_percent: false, show_tips: false, tips_random: false, logo_height: 24.0, text_color: Some([255, 255, 255]), bottom: true, ..Style::default() });
        let u = parse(r##"{"template":"sign","panel_color":"#102030","line_label":"N140-LONG"}"##).unwrap().style;
        assert_eq!((u.template, u.panel_color, u.line_label.as_str()), (Template::Sign, Some([0x10, 0x20, 0x30]), "N140-L"));
        assert_eq!(parse(r##"{"template":"autre"}"##).unwrap().style.template, Template::Classic);
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
    fn every_tip_comes_once_a_round() {
        assert_eq!((0..5).map(|n| tip_index(3, n, false, 7)).collect::<Vec<_>>(), vec![0, 1, 2, 0, 1]);
        for seed in [1u64, 42, 0xDEAD_BEEF] {
            for round in 0..4 {
                let mut seen: Vec<usize> = (0..6).map(|k| tip_index(6, round * 6 + k, true, seed)).collect();
                seen.sort();
                assert_eq!(seen, (0..6).collect::<Vec<_>>());
            }
        }
        assert_eq!(tip_index(1, 5, true, 3), 0);
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
