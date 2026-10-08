//! A dedicated server's welcome: a Markdown text the server serves at `GET /welcome` (see
//! `omsi_net::ws::welcome_text`), fetched by a joining game with the server's timetable and
//! shown once its world is there - a window in the middle of the screen, scrolled when the
//! text is longer than it, closed with its Play button (or Enter, or Escape). A server that
//! has none shows nothing.
//!
//! The Markdown is what such a text needs: headings (`#` to `######`), paragraphs, lists
//! (`-`, `*`, `+`, `1.`), quotes (`>`), rules (`---`), code blocks (```` ``` ````),
//! pictures alone on their line (`![text](name.png)`) and, in a line, `**bold**`,
//! `*italic*`, `` `code` `` and `[links](…)` (their text).
//!
//! A text that begins with a tag is HTML instead, read into the same blocks: `h1`-`h6`, `p`
//! (and `div` …), `br`, `ul`/`ol`/`li`, `blockquote`, `pre`, `hr`, `img`, and `b`/`strong`,
//! `i`/`em`, `code`, `a` in a line; scripts and styles are left out, other tags are read
//! past. A picture is one of the server's (`GET /welcome/<name>`, PNG or JPEG): a picture
//! from elsewhere on the web is not fetched (its text is shown) - the players' games ask
//! the server they joined and nobody else.

use std::sync::Mutex;
use std::time::Duration;

/// A picture of the welcome: its name in the text and its pixels (RGBA).
#[derive(Clone)]
pub struct Picture {
    pub src: String,
    pub width: u32,
    pub height: u32,
    pub rgba: std::sync::Arc<Vec<u8>>,
}

/// The pictures fetched at most, and the widest one kept (px; wider ones are scaled down).
const MAX_PICTURES: usize = 12;
const MAX_PICTURE_WIDTH: u32 = 1600;

/// The welcome fetched while joining (and its pictures), for the game to show once its world
/// is there.
static FETCHED: Mutex<Option<(String, Vec<Picture>)>> = Mutex::new(None);

/// The server's name of a picture's `src`: a plain file name (`images/` before it is
/// allowed); none for a picture elsewhere.
pub fn picture_name(src: &str) -> Option<&str> {
    let s = src.trim().trim_start_matches("./");
    let s = s.strip_prefix("images/").unwrap_or(s);
    (!s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')) && !s.starts_with('.')).then_some(s)
}

/// A joining game: the welcome of the server at the first of `bases` that has one, and the
/// server's pictures it shows.
pub fn fetch_any(bases: &[String]) {
    let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(5)).timeout(Duration::from_secs(10)).build();
    for b in bases {
        match agent.get(&format!("{b}/welcome")).call() {
            Ok(r) => {
                let mut body = Vec::new();
                let limit = omsi_net::ws::MAX_WELCOME;
                if std::io::Read::read_to_end(&mut std::io::Read::take(r.into_reader(), limit), &mut body).is_err() {
                    continue;
                }
                let text = String::from_utf8_lossy(&body).replace("\r\n", "\n").trim().to_string();
                if !text.is_empty() {
                    let pictures = fetch_pictures(&agent, b, &text);
                    log::info!("welcome: the server's welcome ({} bytes, {} pictures)", text.len(), pictures.len());
                    *FETCHED.lock().unwrap_or_else(|e| e.into_inner()) = Some((text, pictures));
                }
                return;
            }
            // (a server without a welcome, or an older one: nothing to show)
            Err(ureq::Error::Status(..)) => return,
            Err(ureq::Error::Transport(_)) => continue,
        }
    }
}

/// The server's pictures the welcome shows, decoded (and scaled down when very wide).
fn fetch_pictures(agent: &ureq::Agent, base: &str, text: &str) -> Vec<Picture> {
    let mut out: Vec<Picture> = Vec::new();
    for b in parse(text).into_iter().filter(|b| b.kind == Kind::Image) {
        let Some(name) = picture_name(&b.marker) else { continue };
        if out.len() >= MAX_PICTURES || out.iter().any(|p| p.src == b.marker) {
            continue;
        }
        let Ok(r) = agent.get(&format!("{base}/welcome/{name}")).call() else {
            log::info!("welcome: picture {name} not served");
            continue;
        };
        let mut body = Vec::new();
        if std::io::Read::read_to_end(&mut std::io::Read::take(r.into_reader(), omsi_net::ws::MAX_WELCOME_IMAGE), &mut body).is_err() {
            continue;
        }
        let img = match image::load_from_memory(&body) {
            Ok(i) => i,
            Err(e) => {
                log::info!("welcome: picture {name}: {e}");
                continue;
            }
        };
        let img = if img.width() > MAX_PICTURE_WIDTH { img.resize(MAX_PICTURE_WIDTH, u32::MAX, image::imageops::FilterType::Triangle) } else { img };
        let rgba = img.to_rgba8();
        out.push(Picture { src: b.marker.clone(), width: rgba.width(), height: rgba.height(), rgba: std::sync::Arc::new(rgba.into_raw()) });
    }
    out
}

/// The welcome fetched, once.
pub fn take() -> Option<(String, Vec<Picture>)> {
    FETCHED.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// The welcome window (or the messages', in the same window): the text, how far it is
/// scrolled (px) and its button's label.
pub struct Welcome {
    pub blocks: Vec<Block>,
    pub pictures: Vec<Picture>,
    pub scroll: f32,
    pub button: &'static str,
    /// A question: what its button does (a Cancel button beside it closes the window).
    pub action: Option<Action>,
}

/// What a question's button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// The player leaves the duty taken from its own menu.
    LeaveDuty,
}

impl Welcome {
    pub fn new(text: &str) -> Welcome {
        Welcome { blocks: parse(text), pictures: Vec::new(), scroll: 0.0, button: "Play", action: None }
    }

    /// The server's welcome with its pictures.
    pub fn with_pictures(text: &str, pictures: Vec<Picture>) -> Welcome {
        Welcome { pictures, ..Welcome::new(text) }
    }

    /// Leave the duty taken from the own menu (`line`, `tour`)? Its button leaves it.
    pub fn leave_duty(line: &str, tour: &str) -> Welcome {
        let md = format!(
            "## {}\n\n**{} / {}**\n\n{}\n",
            omsi_ui::tr("Leave the duty?"),
            literal(line),
            literal(tour),
            literal(&omsi_ui::tr("The duty is free again for the other players and the AI buses."))
        );
        Welcome { blocks: parse(&md), pictures: Vec::new(), scroll: 0.0, button: "Leave the duty", action: Some(Action::LeaveDuty) }
    }

    /// The dispatcher's messages, newest first, in the welcome's window.
    pub fn inbox(list: &[InboxMessage]) -> Welcome {
        Welcome { blocks: parse(&inbox_text(list)), pictures: Vec::new(), scroll: 0.0, button: "Close", action: None }
    }
}

/// A message of the server's dispatcher (`notify`) as it came: when (this device's local
/// date and time, `dd/mm/yyyy hh:mm`), how urgent, and what it said.
#[derive(Debug, Clone, PartialEq)]
pub struct InboxMessage {
    pub at: String,
    pub urgent: bool,
    pub text: String,
}

/// The messages kept (the oldest go).
pub const INBOX_KEEP: usize = 100;

/// A text as it is in the Markdown: every sign the Markdown reads, escaped.
fn literal(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    for c in t.chars() {
        if c.is_ascii_punctuation() {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The messages as the window's Markdown, newest first.
pub fn inbox_text(list: &[InboxMessage]) -> String {
    let mut md = format!("# {}\n\n", omsi_ui::tr("Messages"));
    if list.is_empty() {
        md.push_str(&format!("*{}*\n", literal(&omsi_ui::tr("No message from the dispatcher yet"))));
        return md;
    }
    for (k, m) in list.iter().rev().enumerate() {
        if k > 0 {
            md.push_str("\n---\n\n");
        }
        md.push_str(&format!("**{}**{}\n\n{}\n", literal(&m.at), if m.urgent { " · ⚠" } else { "" }, literal(&m.text)));
    }
    md
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Heading(u8),
    Para,
    /// A list item, its depth (0 the outermost) and marker (`•`, `2.`).
    Item(u8),
    Quote,
    Code,
    Rule,
    /// A picture: its `src` is the block's `marker`, its text the spans.
    Image,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub link: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub text: String,
    pub style: Style,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub kind: Kind,
    /// A list item's marker.
    pub marker: String,
    pub spans: Vec<Span>,
}

impl Block {
    fn text(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }
}

/// The blocks of a welcome: HTML when it begins with a tag, Markdown otherwise.
pub fn parse(text: &str) -> Vec<Block> {
    if text.trim_start().starts_with('<') {
        parse_html(text)
    } else {
        parse_markdown(text)
    }
}

/// The blocks of a Markdown text.
pub fn parse_markdown(text: &str) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    // the paragraph (or quote) being gathered from its lines
    let mut open: Option<(Kind, String, String)> = None;
    let flush = |open: &mut Option<(Kind, String, String)>, out: &mut Vec<Block>| {
        if let Some((kind, marker, t)) = open.take() {
            let t = t.trim().to_string();
            if !t.is_empty() || kind == Kind::Item(0) {
                out.push(Block { kind, marker, spans: inline(&t) });
            }
        }
    };
    let mut code = false;
    for raw in text.lines() {
        let line = raw.trim_end();
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            flush(&mut open, &mut out);
            code = !code;
            continue;
        }
        if code {
            out.push(Block { kind: Kind::Code, marker: String::new(), spans: vec![Span { text: line.replace('\t', "    "), style: Style { code: true, ..Default::default() } }] });
            continue;
        }
        if trimmed.is_empty() {
            flush(&mut open, &mut out);
            continue;
        }
        let indent = line.len() - trimmed.len();
        // a picture alone on its line
        if let Some(rest) = trimmed.strip_prefix("![") {
            if let Some((alt, tail)) = rest.split_once("](") {
                if let Some(src) = tail.strip_suffix(')') {
                    flush(&mut open, &mut out);
                    out.push(Block { kind: Kind::Image, marker: src.trim().to_string(), spans: vec![Span { text: alt.to_string(), style: Style::default() }] });
                    continue;
                }
            }
        }
        // a heading
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') {
            flush(&mut open, &mut out);
            let t = trimmed[hashes..].trim().trim_end_matches('#').trim();
            out.push(Block { kind: Kind::Heading(hashes as u8), marker: String::new(), spans: inline(t) });
            continue;
        }
        // a rule
        let bare: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
        if bare.len() >= 3 && ["-", "*", "_"].iter().any(|m| bare.chars().all(|c| c.to_string() == *m)) {
            flush(&mut open, &mut out);
            out.push(Block { kind: Kind::Rule, marker: String::new(), spans: Vec::new() });
            continue;
        }
        // a list item
        let depth = (indent / 2).min(4) as u8;
        let bullet = ["- ", "* ", "+ "].iter().find(|m| trimmed.starts_with(*m)).map(|_| ("•".to_string(), &trimmed[2..]));
        let numbered = {
            let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
            let rest = &trimmed[digits..];
            (digits > 0 && digits < 4 && (rest.starts_with(". ") || rest.starts_with(") "))).then(|| (format!("{}.", &trimmed[..digits]), &rest[2..]))
        };
        if let Some((marker, rest)) = bullet.or(numbered) {
            flush(&mut open, &mut out);
            open = Some((Kind::Item(depth), marker, rest.trim().to_string()));
            continue;
        }
        // a quote
        if let Some(rest) = trimmed.strip_prefix('>') {
            let rest = rest.trim();
            match open.as_mut() {
                Some((Kind::Quote, _, t)) => {
                    t.push(' ');
                    t.push_str(rest);
                }
                _ => {
                    flush(&mut open, &mut out);
                    open = Some((Kind::Quote, String::new(), rest.to_string()));
                }
            }
            continue;
        }
        // a paragraph's line (or the next line of a list item, indented)
        match open.as_mut() {
            Some((Kind::Para, _, t)) | Some((Kind::Item(_), _, t)) | Some((Kind::Quote, _, t)) => {
                t.push(' ');
                t.push_str(trimmed);
            }
            _ => {
                flush(&mut open, &mut out);
                open = Some((Kind::Para, String::new(), trimmed.to_string()));
            }
        }
    }
    flush(&mut open, &mut out);
    out
}

/// `&amp;` and the others, and `&#233;` / `&#xE9;`.
fn entities(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    let mut rest = t;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let end = tail[..tail.len().min(12)].find(';');
        let decoded = end.and_then(|e| {
            let name = &tail[1..e];
            let c = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" | "#39" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                "eacute" => Some('é'),
                "egrave" => Some('è'),
                "agrave" => Some('à'),
                "ccedil" => Some('ç'),
                "euro" => Some('€'),
                "laquo" => Some('«'),
                "raquo" => Some('»'),
                "hellip" => Some('…'),
                "mdash" => Some('—'),
                "ndash" => Some('–'),
                "copy" => Some('©'),
                "reg" => Some('®'),
                "trade" => Some('™'),
                "deg" => Some('°'),
                "middot" => Some('·'),
                "bull" => Some('•'),
                "rsquo" => Some('’'),
                "lsquo" => Some('‘'),
                "ldquo" => Some('“'),
                "rdquo" => Some('”'),
                "ecirc" => Some('ê'),
                "ocirc" => Some('ô'),
                "acirc" => Some('â'),
                "icirc" => Some('î'),
                "ucirc" => Some('û'),
                "ugrave" => Some('ù'),
                "Eacute" => Some('É'),
                _ => name
                    .strip_prefix("#x")
                    .or_else(|| name.strip_prefix("#X"))
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok()))
                    .and_then(char::from_u32),
            };
            c.map(|c| (c, e + 1))
        });
        match decoded {
            Some((c, n)) => {
                out.push(c);
                rest = &tail[n..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// An HTML tag's attribute (`src="…"`, `alt='…'`, `width=200`).
fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find(name) {
        let at = from + i;
        let before = lower[..at].chars().last();
        let after = lower[at + name.len()..].trim_start();
        if before.is_some_and(|c| c.is_whitespace()) && after.starts_with('=') {
            let v = tag[tag.len() - after.len() + 1..].trim_start();
            let value = match v.chars().next() {
                Some(q @ ('"' | '\'')) => v[1..].split(q).next().unwrap_or(""),
                _ => v.split(|c: char| c.is_whitespace() || c == '>' || c == '/').next().unwrap_or(""),
            };
            return Some(entities(value));
        }
        from = at + name.len();
    }
    None
}

/// The blocks of an HTML text (see the module's doc for what is read).
pub fn parse_html(html: &str) -> Vec<Block> {
    struct List {
        ordered: bool,
        n: u32,
    }
    let mut out: Vec<Block> = Vec::new();
    let mut spans: Vec<Span> = Vec::new();
    let mut kind = Kind::Para;
    let mut marker = String::new();
    let (mut bold, mut italic, mut code, mut link) = (0u32, 0u32, 0u32, 0u32);
    let mut lists: Vec<List> = Vec::new();
    let mut quote = 0u32;
    let mut pre = false;
    let mut pre_text = String::new();
    let base = |quote: u32| if quote > 0 { Kind::Quote } else { Kind::Para };
    // the text gathered as one block
    fn flush(out: &mut Vec<Block>, spans: &mut Vec<Span>, kind: Kind, marker: &mut String) {
        // (the blanks at the block's ends go)
        if let Some(f) = spans.first_mut() {
            f.text = f.text.trim_start().to_string();
        }
        if let Some(l) = spans.last_mut() {
            l.text = l.text.trim_end().to_string();
        }
        spans.retain(|s| !s.text.is_empty());
        if !spans.is_empty() {
            out.push(Block { kind, marker: std::mem::take(marker), spans: std::mem::take(spans) });
        }
        spans.clear();
    }
    let mut rest = html;
    while !rest.is_empty() {
        // a comment
        if let Some(r) = rest.strip_prefix("<!--") {
            rest = r.find("-->").map(|i| &r[i + 3..]).unwrap_or("");
            continue;
        }
        if rest.starts_with('<') {
            let Some(end) = rest.find('>') else { break };
            let tag = &rest[1..end];
            rest = &rest[end + 1..];
            let closing = tag.starts_with('/');
            let name: String = tag.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
            match name.as_str() {
                "script" | "style" | "head" | "title" if !closing => {
                    let close = format!("</{name}");
                    rest = rest.to_ascii_lowercase().find(&close).map(|i| &rest[i..]).unwrap_or("");
                    if let Some(e) = rest.find('>') {
                        rest = &rest[e + 1..];
                    }
                }
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    flush(&mut out, &mut spans, kind, &mut marker);
                    kind = if closing { base(quote) } else { Kind::Heading(name[1..].parse().unwrap_or(1)) };
                }
                "p" | "div" | "section" | "article" | "header" | "footer" | "center" | "main" | "table" | "tr" => {
                    flush(&mut out, &mut spans, kind, &mut marker);
                    if !matches!(kind, Kind::Item(_)) || closing {
                        kind = base(quote);
                    }
                }
                "br" => {
                    let k = kind;
                    flush(&mut out, &mut spans, kind, &mut marker);
                    kind = if matches!(k, Kind::Item(_)) { Kind::Para } else { k };
                }
                "ul" | "ol" => {
                    flush(&mut out, &mut spans, kind, &mut marker);
                    if closing {
                        lists.pop();
                    } else {
                        lists.push(List { ordered: name == "ol", n: 0 });
                    }
                    kind = base(quote);
                }
                "li" => {
                    flush(&mut out, &mut spans, kind, &mut marker);
                    if closing {
                        kind = base(quote);
                    } else {
                        let depth = lists.len().saturating_sub(1).min(4) as u8;
                        marker = match lists.last_mut() {
                            Some(l) if l.ordered => {
                                l.n += 1;
                                format!("{}.", l.n)
                            }
                            _ => "•".into(),
                        };
                        kind = Kind::Item(depth);
                    }
                }
                "blockquote" => {
                    flush(&mut out, &mut spans, kind, &mut marker);
                    quote = if closing { quote.saturating_sub(1) } else { quote + 1 };
                    kind = base(quote);
                }
                "hr" => {
                    flush(&mut out, &mut spans, kind, &mut marker);
                    out.push(Block { kind: Kind::Rule, marker: String::new(), spans: Vec::new() });
                }
                "pre" => {
                    flush(&mut out, &mut spans, kind, &mut marker);
                    if closing {
                        for line in entities(pre_text.trim_matches('\n')).lines() {
                            out.push(Block { kind: Kind::Code, marker: String::new(), spans: vec![Span { text: line.replace('\t', "    "), style: Style { code: true, ..Default::default() } }] });
                        }
                        pre_text.clear();
                    }
                    pre = !closing;
                }
                "img" => {
                    flush(&mut out, &mut spans, kind, &mut marker);
                    let src = attr(tag, "src").unwrap_or_default();
                    if !src.is_empty() {
                        let alt = attr(tag, "alt").unwrap_or_default();
                        out.push(Block { kind: Kind::Image, marker: src, spans: vec![Span { text: alt, style: Style::default() }] });
                    }
                }
                "b" | "strong" => bold = if closing { bold.saturating_sub(1) } else { bold + 1 },
                "i" | "em" => italic = if closing { italic.saturating_sub(1) } else { italic + 1 },
                "code" | "tt" | "kbd" => code = if closing { code.saturating_sub(1) } else { code + 1 },
                "a" => link = if closing { link.saturating_sub(1) } else { link + 1 },
                _ => {}
            }
            continue;
        }
        let end = rest.find('<').unwrap_or(rest.len());
        let raw = &rest[..end];
        rest = &rest[end..];
        if pre {
            pre_text.push_str(raw);
            continue;
        }
        // (blanks and line ends in the text are one blank, as a browser shows them)
        let mut text = String::with_capacity(raw.len());
        let mut blank = false;
        for c in entities(raw).chars() {
            if c.is_whitespace() && c != '\u{a0}' {
                if !blank {
                    text.push(' ');
                }
                blank = true;
            } else {
                text.push(if c == '\u{a0}' { ' ' } else { c });
                blank = false;
            }
        }
        if text.trim().is_empty() && spans.is_empty() {
            continue;
        }
        let style = Style { bold: bold > 0, italic: italic > 0, code: code > 0, link: link > 0 };
        match spans.last_mut() {
            Some(l) if l.style == style => {
                if l.text.ends_with(' ') && text.starts_with(' ') {
                    text.remove(0);
                }
                l.text.push_str(&text);
            }
            _ => spans.push(Span { text, style }),
        }
    }
    flush(&mut out, &mut spans, kind, &mut marker);
    out
}

/// The spans of a line: bold, italic, code and links (their text).
pub fn inline(t: &str) -> Vec<Span> {
    let c: Vec<char> = t.chars().collect();
    let mut out: Vec<Span> = Vec::new();
    let mut style = Style::default();
    let mut cur = String::new();
    let push = |out: &mut Vec<Span>, cur: &mut String, style: Style| {
        if !cur.is_empty() {
            match out.last_mut() {
                Some(l) if l.style == style => l.text.push_str(cur),
                _ => out.push(Span { text: cur.clone(), style }),
            }
            cur.clear();
        }
    };
    let find = |from: usize, pat: &[char]| (from..c.len()).find(|&k| c[k..].starts_with(pat));
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch == '\\' && i + 1 < c.len() && c[i + 1].is_ascii_punctuation() {
            cur.push(c[i + 1]);
            i += 2;
            continue;
        }
        if ch == '`' {
            if let Some(end) = find(i + 1, &['`']) {
                push(&mut out, &mut cur, style);
                cur.extend(&c[i + 1..end]);
                push(&mut out, &mut cur, Style { code: true, ..style });
                i = end + 1;
                continue;
            }
        }
        // [text](url) and ![alt](url): the text
        if ch == '[' || (ch == '!' && c.get(i + 1) == Some(&'[')) {
            let open = if ch == '!' { i + 1 } else { i };
            if let Some(close) = find(open + 1, &[']', '(']) {
                if let Some(end) = find(close + 2, &[')']) {
                    push(&mut out, &mut cur, style);
                    let inner: String = c[open + 1..close].iter().collect();
                    for s in inline(&inner) {
                        cur.push_str(&s.text);
                        push(&mut out, &mut cur, Style { link: ch == '[', bold: style.bold || s.style.bold, italic: style.italic || s.style.italic || ch == '!', ..style });
                    }
                    i = end + 1;
                    continue;
                }
            }
        }
        if (ch == '*' || ch == '_') && c.get(i + 1) == Some(&ch) {
            // (bold: opened where a closing pair follows, closed where it is bold)
            if style.bold || find(i + 2, &[ch, ch]).is_some() {
                push(&mut out, &mut cur, style);
                style.bold = !style.bold;
                i += 2;
                continue;
            }
        }
        if ch == '*' || ch == '_' {
            // (an underscore inside a word is a letter of it: snake_case)
            let inside_word = ch == '_' && i > 0 && c[i - 1].is_alphanumeric() && c.get(i + 1).is_some_and(|n| n.is_alphanumeric());
            let opens = !style.italic && c.get(i + 1).is_some_and(|n| !n.is_whitespace()) && (i + 1..c.len()).any(|k| c[k] == ch);
            if !inside_word && (style.italic || opens) {
                push(&mut out, &mut cur, style);
                style.italic = !style.italic;
                i += 1;
                continue;
            }
        }
        cur.push(ch);
        i += 1;
    }
    push(&mut out, &mut cur, style);
    out
}

/// How a run of text is drawn: its tone (the colour the interface gives it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Body,
    Heading,
    Muted,
    Link,
    Code,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub x: f32,
    pub text: String,
    pub px: f32,
    pub bold: bool,
    pub tone: Tone,
    /// Drawn on a plate (inline code).
    pub plate: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Deco {
    None,
    Rule,
    /// A quote's bar, at its left.
    Quote,
    /// A code block's line: a plate across the width.
    Code,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub y: f32,
    pub h: f32,
    pub runs: Vec<Run>,
    pub deco: Deco,
    /// A picture on the line: its `src`, where it begins and how wide it is drawn (its
    /// height is the line's).
    pub image: Option<(String, f32, f32)>,
}

/// The lines of `blocks` laid out `width` wide with a body text of `px` pixels;
/// `measure(text, bold, px)` is how wide a text is drawn, `picture(src)` how large a picture
/// is (none: not there - its text is shown); a picture is drawn at its size scaled as the
/// text (`px / 16`), no wider than the text and no higher than `max_picture`, in the middle.
/// Returns the lines and the whole height.
pub fn layout(blocks: &[Block], width: f32, px: f32, measure: &dyn Fn(&str, bool, f32) -> f32, picture: &dyn Fn(&str) -> Option<(u32, u32)>, max_picture: f32) -> (Vec<Line>, f32) {
    let mut lines: Vec<Line> = Vec::new();
    let mut y = 0.0f32;
    let gap = px * 0.7;
    let mut prev: Option<Kind> = None;
    for b in blocks {
        let (size, bold, tone) = match b.kind {
            Kind::Heading(1) => (px * 1.6, true, Tone::Heading),
            Kind::Heading(2) => (px * 1.35, true, Tone::Heading),
            Kind::Heading(3) => (px * 1.15, true, Tone::Heading),
            Kind::Heading(_) => (px, true, Tone::Heading),
            Kind::Quote => (px, false, Tone::Muted),
            Kind::Code => (px * 0.95, false, Tone::Code),
            _ => (px, false, Tone::Body),
        };
        let lh = (size * 1.42).ceil();
        // the space before the block: none between the lines of a code block or the items
        // of a list, more before a heading
        let same_run = matches!((prev, b.kind), (Some(Kind::Code), Kind::Code) | (Some(Kind::Item(_)), Kind::Item(_)));
        if prev.is_some() {
            y += if same_run {
                px * 0.15
            } else if matches!(b.kind, Kind::Heading(_)) {
                gap * 1.4
            } else {
                gap
            };
        }
        prev = Some(b.kind);
        if b.kind == Kind::Rule {
            lines.push(Line { y, h: px, runs: Vec::new(), deco: Deco::Rule, image: None });
            y += px;
            continue;
        }
        if b.kind == Kind::Image {
            match picture(&b.marker).filter(|(w, h)| *w > 0 && *h > 0) {
                Some((iw, ih)) => {
                    let mut w = (iw as f32 * px / 16.0).min(width);
                    let mut h = w * ih as f32 / iw as f32;
                    if h > max_picture {
                        h = max_picture;
                        w = h * iw as f32 / ih as f32;
                    }
                    lines.push(Line { y, h: h.round(), runs: Vec::new(), deco: Deco::None, image: Some((b.marker.clone(), ((width - w) * 0.5).max(0.0), w)) });
                    y += h.round();
                }
                None => {
                    // (a picture not there: its text, if it has one)
                    let alt = b.text();
                    if !alt.trim().is_empty() {
                        let lh = (px * 1.42).ceil();
                        lines.push(Line { y, h: lh, runs: vec![Run { x: 0.0, text: format!("[{}]", alt.trim()), px, bold: false, tone: Tone::Muted, plate: false }], deco: Deco::None, image: None });
                        y += lh;
                    }
                }
            }
            continue;
        }
        let (indent, deco) = match b.kind {
            Kind::Item(d) => (px * 1.4 * (d as f32 + 1.0), Deco::None),
            Kind::Quote => (px * 1.0, Deco::Quote),
            Kind::Code => (px * 0.6, Deco::Code),
            _ => (0.0, Deco::None),
        };
        let mut rows: Vec<Vec<Run>> = vec![Vec::new()];
        let mut x = indent;
        let marker = matches!(b.kind, Kind::Item(_)).then(|| {
            let mw = measure(&b.marker, false, size);
            Run { x: (indent - mw - px * 0.4).max(0.0), text: b.marker.clone(), px: size, bold: false, tone: Tone::Muted, plate: false }
        });
        let room = (width - px * 0.6).max(px * 4.0);
        // word by word (a code block's line as it is, cut where it does not fit)
        let words: Vec<(String, Style)> = if b.kind == Kind::Code {
            vec![(b.text(), Style { code: true, ..Default::default() })]
        } else {
            b.spans
                .iter()
                .flat_map(|s| split_words(&s.text).into_iter().map(move |w| (w, s.style)))
                .collect()
        };
        for (word, style) in words {
            let wb = bold || style.bold;
            let mut word = word;
            loop {
                let w = measure(word.trim_end(), wb, size);
                let fits = x + w <= room;
                let line_empty = rows.last().is_some_and(|r| r.is_empty());
                if !fits && !line_empty {
                    // on to the next line (without the blank the line began with)
                    rows.push(Vec::new());
                    x = indent;
                    word = word.trim_start().to_string();
                    continue;
                }
                if !fits {
                    // longer than a whole line: cut where it fits
                    let mut k = word.len();
                    while k > 1 && measure(&word[..k], wb, size) > room - x {
                        k = word[..k].char_indices().last().map(|(i, _)| i).unwrap_or(0).max(1);
                        while !word.is_char_boundary(k) {
                            k -= 1;
                        }
                    }
                    let head = word[..k].to_string();
                    let rest = word[k..].to_string();
                    put(rows.last_mut().unwrap(), &mut x, head, style, wb, size, tone, measure);
                    if rest.is_empty() {
                        break;
                    }
                    rows.push(Vec::new());
                    x = indent;
                    word = rest;
                    continue;
                }
                put(rows.last_mut().unwrap(), &mut x, word, style, wb, size, tone, measure);
                break;
            }
        }
        if let Some(m) = marker {
            rows[0].insert(0, m);
        }
        for runs in rows {
            lines.push(Line { y, h: lh, runs, deco, image: None });
            y += lh;
        }
    }
    (lines, y.ceil())
}

/// A word (with the blank after it) on the row: onto the run before it when drawn alike.
#[allow(clippy::too_many_arguments)]
fn put(row: &mut Vec<Run>, x: &mut f32, word: String, style: Style, bold: bool, px: f32, tone: Tone, measure: &dyn Fn(&str, bool, f32) -> f32) {
    let tone = if style.code && tone != Tone::Code {
        Tone::Code
    } else if style.link {
        Tone::Link
    } else if style.italic && tone == Tone::Body {
        Tone::Muted
    } else {
        tone
    };
    let plate = style.code && tone == Tone::Code;
    match row.last_mut() {
        Some(r) if r.bold == bold && r.tone == tone && r.plate == plate && r.px == px => {
            r.text.push_str(&word);
            *x = r.x + measure(&r.text, bold, px);
        }
        _ => {
            let w = measure(&word, bold, px);
            row.push(Run { x: *x, text: word, px, bold, tone, plate });
            *x += w;
        }
    }
}

/// The words of a text, each with the blanks after it.
fn split_words(t: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut blank = false;
    for ch in t.chars() {
        if ch.is_whitespace() {
            blank = true;
            cur.push(' ');
        } else {
            if blank && !cur.trim().is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            blank = false;
            cur.push(ch);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(spans: &[Span]) -> String {
        spans.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn the_blocks_of_a_welcome() {
        let b = parse("# NEROSY Transit\n\nBienvenue sur le **serveur**.\nDeux lignes, un paragraphe.\n\n## Règles\n- pas de *klaxon*\n- respecter les `feux`\n  et les priorités\n1. un\n2) deux\n\n> une citation\n> sur deux lignes\n\n---\n```\ncode  ici\n```");
        let kinds: Vec<Kind> = b.iter().map(|b| b.kind).collect();
        assert_eq!(kinds, [Kind::Heading(1), Kind::Para, Kind::Heading(2), Kind::Item(0), Kind::Item(0), Kind::Item(0), Kind::Item(0), Kind::Quote, Kind::Rule, Kind::Code]);
        assert_eq!(plain(&b[1].spans), "Bienvenue sur le serveur. Deux lignes, un paragraphe.");
        assert!(b[1].spans.iter().any(|s| s.text == "serveur" && s.style.bold));
        assert_eq!(plain(&b[4].spans), "respecter les feux et les priorités");
        assert_eq!(b[5].marker, "1.");
        assert_eq!(b[6].marker, "2.");
        assert_eq!(plain(&b[7].spans), "une citation sur deux lignes");
        assert_eq!(plain(&b[9].spans), "code  ici");
    }

    #[test]
    fn inline_marks() {
        let s = inline("a **b** *c* `d` [e](http://x) snake_case f\\*g");
        assert_eq!(plain(&s), "a b c d e snake_case f*g");
        assert!(s.iter().any(|x| x.text == "b" && x.style.bold));
        assert!(s.iter().any(|x| x.text == "c" && x.style.italic));
        assert!(s.iter().any(|x| x.text == "d" && x.style.code));
        assert!(s.iter().any(|x| x.text == "e" && x.style.link));
        // a lone star is a star
        assert_eq!(plain(&inline("5 * 3")), "5 * 3");
    }

    #[test]
    fn lines_wrap_within_the_width() {
        let m = |t: &str, _: bool, px: f32| t.chars().count() as f32 * px * 0.5;
        let b = parse("un deux trois quatre cinq six sept huit neuf dix\n\nmotextrêmementlongquinetientpassurunelignemotextrêmementlong");
        let (lines, h) = layout(&b, 100.0, 10.0, &m, &|_| None, 500.0);
        assert!(lines.len() >= 4, "{lines:?}");
        for l in &lines {
            for r in &l.runs {
                assert!(r.x + m(r.text.trim_end(), r.bold, r.px) <= 100.0 + 0.01, "{r:?}");
            }
        }
        assert!(h >= lines.last().unwrap().y + lines.last().unwrap().h - 0.5);
        // the words in their order, nothing lost
        let all: String = lines.iter().flat_map(|l| l.runs.iter().map(|r| r.text.as_str())).collect::<Vec<_>>().join("");
        assert_eq!(all.split_whitespace().collect::<String>(), "undeuxtroisquatrecinqsixsepthuitneufdixmotextrêmementlongquinetientpassurunelignemotextrêmementlong");
    }

    #[test]
    fn the_messages_newest_first_and_as_they_were_written() {
        let list = vec![
            InboxMessage { at: "07/10/2026 21:58".into(), urgent: false, text: "Ligne 14 : *déviation* # rue_du_Moulon".into() },
            InboxMessage { at: "07/10/2026 22:41".into(), urgent: true, text: "- Retour au dépôt".into() },
        ];
        let b = parse(&inbox_text(&list));
        let texts: Vec<String> = b.iter().map(|b| plain(&b.spans)).collect();
        assert_eq!(b[0].kind, Kind::Heading(1));
        assert_eq!(texts[1], "07/10/2026 22:41 · ⚠");
        // (a message's signs are not Markdown)
        assert_eq!(b[2].kind, Kind::Para);
        assert_eq!(texts[2], "- Retour au dépôt");
        assert_eq!(b[3].kind, Kind::Rule);
        assert_eq!(texts[5], "Ligne 14 : *déviation* # rue_du_Moulon");
        assert!(b[5].spans.iter().all(|s| !s.style.italic));
        assert_eq!(parse(&inbox_text(&[])).len(), 2);
    }

    #[test]
    fn a_welcome_in_html() {
        let b = parse("<h1>Bienvenue &amp; bonne route</h1>
<p>Un <b>serveur</b> <i>français</i>,
  sur deux   lignes.<br>Puis une autre.</p><img src=\"images/plan.png\" alt='Le plan'>
<ul><li>un</li><li>deux <a href=x>lien</a></li></ul><ol><li>premier</li></ol><blockquote>cite</blockquote><hr><pre>a  b
 c</pre><script>alert(1)</script><!-- note --><p>&#233;t&eacute; &copy;</p>");
        let texts: Vec<(Kind, String)> = b.iter().map(|b| (b.kind, plain(&b.spans))).collect();
        assert_eq!(texts[0], (Kind::Heading(1), "Bienvenue & bonne route".into()));
        assert_eq!(texts[1], (Kind::Para, "Un serveur français, sur deux lignes.".into()));
        assert!(b[1].spans.iter().any(|s| s.text == "serveur" && s.style.bold));
        assert!(b[1].spans.iter().any(|s| s.text == "français" && s.style.italic));
        assert_eq!(texts[2], (Kind::Para, "Puis une autre.".into()));
        assert_eq!((b[3].kind, b[3].marker.as_str(), texts[3].1.as_str()), (Kind::Image, "images/plan.png", "Le plan"));
        assert_eq!((b[4].kind, b[4].marker.as_str()), (Kind::Item(0), "•"));
        assert!(b[5].spans.iter().any(|s| s.text == "lien" && s.style.link));
        assert_eq!((b[6].kind, b[6].marker.as_str()), (Kind::Item(0), "1."));
        assert_eq!(texts[7], (Kind::Quote, "cite".into()));
        assert_eq!(b[8].kind, Kind::Rule);
        assert_eq!(texts[9], (Kind::Code, "a  b".into()));
        assert_eq!(texts[10], (Kind::Code, " c".into()));
        assert_eq!(texts[11], (Kind::Para, "été ©".into()));
        assert_eq!(b.len(), 12, "{texts:?}");
        // a Markdown picture alone on its line
        let m = parse("Texte

![Plan du réseau](plan.png)");
        assert_eq!((m[1].kind, m[1].marker.as_str()), (Kind::Image, "plan.png"));
    }

    #[test]
    fn pictures_are_the_servers_and_fit_the_window() {
        assert_eq!(picture_name("images/plan.png"), Some("plan.png"));
        assert_eq!(picture_name("./logo_2.jpg"), Some("logo_2.jpg"));
        assert_eq!(picture_name("https://example.org/x.png"), None);
        assert_eq!(picture_name("../x.png"), None);
        let m = |t: &str, _: bool, px: f32| t.chars().count() as f32 * px * 0.5;
        let b = parse("![a](wide.png)

![b](tall.png)

![Absente](none.png)");
        let size = |s: &str| match s { "wide.png" => Some((2000, 500)), "tall.png" => Some((100, 3000)), _ => None };
        let (lines, _) = layout(&b, 400.0, 16.0, &m, &size, 300.0);
        let wide = lines[0].image.clone().unwrap();
        assert_eq!((wide.2, lines[0].h), (400.0, 100.0));
        let tall = lines[1].image.clone().unwrap();
        assert_eq!(lines[1].h, 300.0);
        assert!((tall.2 - 10.0).abs() < 0.01 && (tall.1 - 195.0).abs() < 0.01);
        // a picture not there: its text
        assert_eq!(lines[2].runs[0].text, "[Absente]");
    }

    #[test]
    fn a_list_item_has_its_marker_left_of_its_text() {
        let m = |t: &str, _: bool, px: f32| t.chars().count() as f32 * px * 0.5;
        let (lines, _) = layout(&parse("- point"), 300.0, 10.0, &m, &|_| None, 500.0);
        assert_eq!(lines[0].runs[0].text, "•");
        assert!(lines[0].runs[0].x < lines[0].runs[1].x);
    }
}
