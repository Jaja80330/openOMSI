//! A dedicated server's welcome: a Markdown text the server serves at `GET /welcome` (see
//! `omsi_net::ws::welcome_text`), fetched by a joining game with the server's timetable and
//! shown once its world is there - a window in the middle of the screen, scrolled when the
//! text is longer than it, closed with its Play button (or Enter, or Escape). A server that
//! has none shows nothing.
//!
//! The Markdown is what such a text needs: headings (`#` to `######`), paragraphs, lists
//! (`-`, `*`, `+`, `1.`), quotes (`>`), rules (`---`), code blocks (```` ``` ````) and, in
//! a line, `**bold**`, `*italic*`, `` `code` `` and `[links](…)` (their text). Nothing of
//! it is read as HTML.

use std::sync::Mutex;
use std::time::Duration;

/// The welcome fetched while joining, for the game to show once its world is there.
static FETCHED: Mutex<Option<String>> = Mutex::new(None);

/// A joining game: the welcome of the server at the first of `bases` that has one.
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
                    log::info!("welcome: the server's welcome ({} bytes)", text.len());
                    *FETCHED.lock().unwrap_or_else(|e| e.into_inner()) = Some(text);
                }
                return;
            }
            // (a server without a welcome, or an older one: nothing to show)
            Err(ureq::Error::Status(..)) => return,
            Err(ureq::Error::Transport(_)) => continue,
        }
    }
}

/// The welcome fetched, once.
pub fn take() -> Option<String> {
    FETCHED.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// The welcome window: the text and how far it is scrolled (px).
pub struct Welcome {
    pub blocks: Vec<Block>,
    pub scroll: f32,
}

impl Welcome {
    pub fn new(text: &str) -> Welcome {
        Welcome { blocks: parse(text), scroll: 0.0 }
    }
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

/// The blocks of a Markdown text.
pub fn parse(text: &str) -> Vec<Block> {
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
}

/// The lines of `blocks` laid out `width` wide with a body text of `px` pixels;
/// `measure(text, bold, px)` is how wide a text is drawn. Returns the lines and the whole
/// height.
pub fn layout(blocks: &[Block], width: f32, px: f32, measure: &dyn Fn(&str, bool, f32) -> f32) -> (Vec<Line>, f32) {
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
            lines.push(Line { y, h: px, runs: Vec::new(), deco: Deco::Rule });
            y += px;
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
            lines.push(Line { y, h: lh, runs, deco });
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
        let (lines, h) = layout(&b, 100.0, 10.0, &m);
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
    fn a_list_item_has_its_marker_left_of_its_text() {
        let m = |t: &str, _: bool, px: f32| t.chars().count() as f32 * px * 0.5;
        let (lines, _) = layout(&parse("- point"), 300.0, 10.0, &m);
        assert_eq!(lines[0].runs[0].text, "•");
        assert!(lines[0].runs[0].x < lines[0].runs[1].x);
    }
}
