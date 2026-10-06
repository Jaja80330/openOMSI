//! The dispatch radio ("Phonie SAE") of a dedicated server: the drivers and the dispatcher
//! of the server's web dispatch page talk as a bus company's control room talks to its
//! buses - over an analogue two-way radio, push to talk.
//!
//! * A driver asks to be called with the green button over the navigator (`radio request`);
//!   its handset says what the radio does: free, a request out, a call on, or no radio (no
//!   dispatcher on duty). The dispatcher sees the request and takes it (acknowledged), drops
//!   it, or calls the driver back. Unanswered for a minute, the request is over for the
//!   driver (the button can be pressed again); the dispatcher still has it.
//! * The dispatcher's calls: an individual call (the dispatcher and one bus, both talk),
//!   a selective call (the dispatcher to the buses chosen) and a general call (to every
//!   bus on the server); in the last two only the dispatcher talks. Only the dispatcher
//!   starts and ends a call - a driver cannot hang up. A call reaches the players who drive a
//!   bus of their own: one on foot, or riding in another's bus, has no radio.
//! * Push to talk: a key in the game (`radio_ptt`, the right Ctrl key unless moved), a key
//!   on the dispatch page. The dispatcher's key wins: while the dispatcher talks, the
//!   driver's voice is not passed on (and the driver's game sends none).
//!
//! A bus's scripts see the radio and work it, for a terminal of its own in the cab
//! (`Radio::bus_link`, the variables in docs/MODDING.md): `Phonie_State`, `Phonie_Call`,
//! `Phonie_Talk`, `Phonie_Request`, `Phonie_PTT`, `Phonie_Sending` and `Phonie_Receiving`
//! are written into the variables the bus declares; `Phonie_Cmd_Request` (set to 1: a
//! request goes out, and it is set back to 0) and `Phonie_Cmd_PTT` (1 while the cab's key
//! is held) are read from them.
//!
//! The voice goes as `omsi_net::dispatch` frames (8 kHz ADPCM); the radio's sound is made
//! where it is heard (`omsi_audio::twoway`). The server tells each player where its radio
//! stands every two seconds and whenever it changes: `radio state <call> <talk> <request>
//! <dispatcher>` (`none|individual|selective|general`, `-|dispatcher|driver`,
//! `none|pending|taken`, `on|off`: a dispatcher's console is connected) - a game that hears
//! no more of it for a while takes the radio for gone.

use omsi_net::dispatch::{self, ConsoleIn, ConsoleOut, Frame};
use omsi_net::LanSession;
use serde_json::{json, Value};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The calls a dispatcher makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallKind {
    Individual,
    Selective,
    General,
}

impl CallKind {
    fn word(self) -> &'static str {
        match self {
            CallKind::Individual => "individual",
            CallKind::Selective => "selective",
            CallKind::General => "general",
        }
    }

    fn parse(w: &str) -> Option<CallKind> {
        match w {
            "individual" => Some(CallKind::Individual),
            "selective" => Some(CallKind::Selective),
            "general" => Some(CallKind::General),
            _ => None,
        }
    }

    /// What the HUD says over the navigator.
    pub(crate) fn label(self) -> &'static str {
        match self {
            CallKind::Individual => "Individual call",
            CallKind::Selective => "Selective call",
            CallKind::General => "General call",
        }
    }
}

/// Who talks on the radio now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Talk {
    Nobody,
    Dispatcher,
    Driver,
}

impl Talk {
    fn word(self) -> &'static str {
        match self {
            Talk::Nobody => "-",
            Talk::Dispatcher => "dispatcher",
            Talk::Driver => "driver",
        }
    }

    fn parse(w: &str) -> Talk {
        match w {
            "dispatcher" => Talk::Dispatcher,
            "driver" => Talk::Driver,
            _ => Talk::Nobody,
        }
    }
}

/// A driver's request to be called.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Request {
    None,
    Pending,
    /// The dispatcher has seen it and will call back.
    Taken,
}

impl Request {
    fn word(self) -> &'static str {
        match self {
            Request::None => "none",
            Request::Pending => "pending",
            Request::Taken => "taken",
        }
    }

    fn parse(w: &str) -> Request {
        match w {
            "pending" => Request::Pending,
            "taken" => Request::Taken,
            _ => Request::None,
        }
    }
}

/// A voice is heard as talking this long after its last frame.
const TALK_HOLD: Duration = Duration::from_millis(300);
/// How often the server tells every player where its radio stands.
const STATE_EVERY: f32 = 2.0;
/// A game that heard nothing of the radio for this long takes it for gone.
const STATE_GONE: Duration = Duration::from_secs(7);

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// ---------------------------------------------------------------------------------------
// the server

#[derive(Debug, Clone)]
struct PendingRequest {
    player: u32,
    /// When it came (Unix seconds, for the dispatch page).
    since: u64,
    taken: bool,
}

#[derive(Debug, Clone)]
struct Call {
    kind: CallKind,
    /// The players called (a general call: everybody, as they come and go).
    members: Vec<u32>,
    since: u64,
}

/// The radio of a dedicated server: the requests, the call, who talks.
pub(crate) struct RadioServer {
    requests: Vec<PendingRequest>,
    call: Option<Call>,
    /// The console whose key is down (or whose voice came last), and until when it talks.
    dispatcher: Option<(u64, Instant)>,
    /// The driver talking in an individual call, and until when.
    driver: Option<(u32, Instant)>,
    /// What each player was told last, and when the next round of states is due.
    told: std::collections::HashMap<u32, String>,
    refresh: f32,
    /// What the consoles were told last.
    console_state: String,
    console_refresh: f32,
}

impl RadioServer {
    /// The radio of this server: the dispatch page's consoles may connect from now on.
    pub(crate) fn new() -> RadioServer {
        dispatch::open_consoles();
        log::info!("server: dispatch radio on (consoles at the gateway's /dispatch)");
        RadioServer { requests: Vec::new(), call: None, dispatcher: None, driver: None, told: Default::default(), refresh: 0.0, console_state: String::new(), console_refresh: 0.0 }
    }

    fn talk(&self) -> Talk {
        let now = Instant::now();
        if self.dispatcher.is_some_and(|(_, until)| until > now) {
            Talk::Dispatcher
        } else if self.driver.is_some_and(|(_, until)| until > now) {
            Talk::Driver
        } else {
            Talk::Nobody
        }
    }

    /// A player's `radio …` command.
    pub(crate) fn command(&mut self, from: u32, arg: &str) {
        if arg.trim() == "request" {
            // (a driver already in a call with the dispatcher has nothing to ask for)
            let in_call = self.call.as_ref().is_some_and(|c| c.kind == CallKind::Individual && c.members.contains(&from));
            if in_call {
                return;
            }
            match self.requests.iter_mut().find(|r| r.player == from) {
                Some(r) => r.taken = false,
                None => {
                    log::info!("radio: player {from} asks to be called");
                    self.requests.push(PendingRequest { player: from, since: unix_now(), taken: false });
                }
            }
            self.refresh = 0.0;
            self.console_refresh = 0.0;
        }
    }

    /// Once a server frame: the consoles' messages and voices, the players' voices, the
    /// players that left, the states told.
    pub(crate) fn tick(&mut self, lan: &mut LanSession, dt: f32) {
        let names: std::collections::HashMap<u32, String> = lan.peers().map(|p| (p.pose.id, p.pose.name.clone())).collect();
        let mut here: Vec<u32> = names.keys().copied().collect();
        here.sort_unstable();
        // who drives a bus of their own: a player on foot, or riding in another's bus, has no
        // radio to be called on
        let mut drivers: Vec<u32> = lan.peers().filter(|p| p.pose.walker.is_none() && p.pose.has_vehicle()).map(|p| p.pose.id).collect();
        drivers.sort_unstable();
        let name = |id: u32| names.get(&id).cloned().unwrap_or_default();
        // players gone: their requests, their place in a call
        self.requests.retain(|r| here.contains(&r.player));
        if let Some(c) = self.call.as_mut() {
            match c.kind {
                CallKind::General => c.members = drivers.clone(),
                _ => c.members.retain(|m| here.contains(m)),
            }
            if c.members.is_empty() && c.kind != CallKind::General {
                log::info!("radio: the {} call ends, nobody left in it", c.kind.word());
                self.call = None;
            }
        }
        for msg in dispatch::take_console_input() {
            match msg {
                ConsoleIn::Opened(id) => {
                    log::info!("radio: dispatch console {id} on");
                    self.console_refresh = 0.0;
                }
                ConsoleIn::Closed(id) => {
                    if self.dispatcher.is_some_and(|(c, _)| c == id) {
                        self.dispatcher = None;
                    }
                }
                ConsoleIn::Text(id, text) => self.console_command(id, &text, &drivers),
                ConsoleIn::Voice(id, frame) => {
                    if Frame::from_bytes(&frame).is_none() {
                        continue;
                    }
                    // (another console's key held: one dispatcher talks at a time)
                    if self.dispatcher.is_some_and(|(c, until)| c != id && until > Instant::now()) {
                        continue;
                    }
                    self.dispatcher = Some((id, Instant::now() + TALK_HOLD));
                    if let Some(c) = self.call.as_ref() {
                        for &m in &c.members {
                            lan.send_radio(m, dispatch::DISPATCHER, &frame);
                        }
                    }
                }
            }
        }
        // a driver's voice: only in an individual call, only while the dispatcher is quiet
        for (from, frame) in lan.take_radio() {
            let called = self.call.as_ref().is_some_and(|c| c.kind == CallKind::Individual && c.members == [from]);
            if !called || self.talk() == Talk::Dispatcher || Frame::from_bytes(&frame).is_none() {
                continue;
            }
            self.driver = Some((from, Instant::now() + TALK_HOLD));
            let mut out = from.to_le_bytes().to_vec();
            out.extend_from_slice(&frame);
            dispatch::to_consoles(ConsoleOut::Binary(out));
        }
        // every player: where the radio stands (at once when it changed)
        let on_duty = dispatch::console_count() > 0;
        self.refresh -= dt;
        let talk = self.talk();
        let due = self.refresh <= 0.0;
        if due {
            self.refresh = STATE_EVERY;
        }
        for &id in &here {
            let (call, talk) = match self.call.as_ref().filter(|c| c.members.contains(&id)) {
                Some(c) => (c.kind.word(), talk.word()),
                None => ("none", "-"),
            };
            let req = match self.requests.iter().find(|r| r.player == id) {
                Some(r) if r.taken => Request::Taken,
                Some(_) => Request::Pending,
                None => Request::None,
            };
            let line = format!("radio state {call} {talk} {} {}", req.word(), if on_duty { "on" } else { "off" });
            if due || self.told.get(&id) != Some(&line) {
                lan.command(id, &line);
                self.told.insert(id, line);
            }
        }
        self.told.retain(|id, _| here.contains(id));
        // the consoles: the requests, the call, who talks
        self.console_refresh -= dt;
        let state = json!({
            "t": "state",
            "requests": self.requests.iter().map(|r| json!({"player": r.player, "name": name(r.player), "since": r.since, "taken": r.taken})).collect::<Vec<_>>(),
            "call": self.call.as_ref().map(|c| json!({"kind": c.kind.word(), "players": c.members, "since": c.since})),
            "talk": talk.word(),
            "driver": self.driver.filter(|_| talk == Talk::Driver).map(|(d, _)| d),
            "players": here.iter().map(|&id| json!({"id": id, "name": name(id), "bus": drivers.contains(&id)})).collect::<Vec<_>>(),
        })
        .to_string();
        if self.console_refresh <= 0.0 || state != self.console_state {
            self.console_refresh = STATE_EVERY;
            dispatch::to_consoles(ConsoleOut::Text(state.clone()));
            self.console_state = state;
        }
    }

    /// A console's message: `{"t": "call" | "end" | "take" | "drop" | "ptt", …}`.
    /// (`drivers`: the players a call can reach, those driving a bus of their own)
    fn console_command(&mut self, console: u64, text: &str, drivers: &[u32]) {
        let Ok(v) = serde_json::from_str::<Value>(text) else {
            return;
        };
        let player = v.get("player").and_then(Value::as_u64).map(|p| p as u32);
        match v.get("t").and_then(Value::as_str).unwrap_or("") {
            "call" => {
                let Some(kind) = v.get("kind").and_then(Value::as_str).and_then(CallKind::parse) else {
                    return;
                };
                let asked: Vec<u32> = v.get("players").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(|p| p as u32).collect()).unwrap_or_default();
                let members: Vec<u32> = match kind {
                    CallKind::General => drivers.to_vec(),
                    _ => asked.into_iter().filter(|p| drivers.contains(p)).collect(),
                };
                if members.is_empty() && kind != CallKind::General {
                    return;
                }
                if kind == CallKind::Individual && members.len() != 1 {
                    return;
                }
                // a driver called back: the request is answered
                if kind == CallKind::Individual {
                    self.requests.retain(|r| r.player != members[0]);
                }
                log::info!("radio: {} call to {:?}", kind.word(), members);
                self.call = Some(Call { kind, members, since: unix_now() });
                self.driver = None;
            }
            "end" => {
                if let Some(c) = self.call.take() {
                    log::info!("radio: the {} call ends", c.kind.word());
                }
                self.driver = None;
            }
            "take" => {
                if let Some(r) = self.requests.iter_mut().find(|r| Some(r.player) == player) {
                    r.taken = true;
                }
            }
            "drop" => self.requests.retain(|r| Some(r.player) != player),
            "ptt" => {
                let on = v.get("on").and_then(Value::as_bool).unwrap_or(false);
                let mine = self.dispatcher.is_some_and(|(c, _)| c == console);
                let other_talks = self.dispatcher.is_some_and(|(c, until)| c != console && until > Instant::now());
                if on && !other_talks {
                    // (held until the key is let go; the voice frames keep it on after that)
                    self.dispatcher = Some((console, Instant::now() + Duration::from_secs(60)));
                    self.driver = None;
                } else if !on && mine {
                    self.dispatcher = Some((console, Instant::now() + TALK_HOLD));
                }
            }
            _ => {}
        }
        self.refresh = 0.0;
        self.console_refresh = 0.0;
    }
}

// ---------------------------------------------------------------------------------------
// the game

/// What the HUD shows of the radio.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RadioHud {
    pub call: Option<CallKind>,
    pub talk: Talk,
    /// The button's handset.
    pub button: Button,
    /// A request can be sent now (the button can be pressed).
    pub can_request: bool,
    /// Our key is down and our voice goes out.
    pub sending: bool,
    /// Our key is down but the dispatcher talks (or the call lets us not talk).
    pub blocked: bool,
    /// No microphone to talk with (why).
    pub mic_error: Option<String>,
}

/// What the button's handset says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Button {
    /// Free: a request can go out.
    Idle,
    /// A request is out, waiting for its answer.
    Requested,
    /// A call is on.
    InCall,
    /// No radio: the server's is gone, or no dispatcher is on duty.
    Unavailable,
}

impl Button {
    /// The icon (`assets/icons/custom`).
    pub(crate) fn icon(self) -> &'static str {
        match self {
            Button::Idle => "phonie_call",
            Button::Requested => "phonie_call_out",
            Button::InCall => "phonie_call_in",
            Button::Unavailable => "phonie_unavailable",
        }
    }
}

/// The variables of the bus driven (`Radio::bus_link`): only those its scripts declare.
pub(crate) trait BusVars {
    fn var(&self, name: &str) -> Option<f32>;
    /// Whether the bus has the variable.
    fn set_var(&mut self, name: &str, v: f32) -> bool;
}

impl BusVars for omsi_sim::VehicleInstance {
    fn var(&self, name: &str) -> Option<f32> {
        omsi_sim::VehicleInstance::var(self, name)
    }

    fn set_var(&mut self, name: &str, v: f32) -> bool {
        omsi_sim::VehicleInstance::set_var(self, name, v)
    }
}

/// A request unanswered this long is over in the game (the dispatcher keeps it).
const REQUEST_WAIT: Duration = Duration::from_secs(60);

/// The radio in a player's game.
pub(crate) struct Radio {
    /// When the server last said where the radio stands (none: it runs no radio).
    heard: Option<Instant>,
    call: Option<CallKind>,
    talk: Talk,
    request: Request,
    /// A dispatcher's console is connected.
    on_duty: bool,
    /// When our request went out (it is over for us `REQUEST_WAIT` later).
    requested: Option<Instant>,
    /// The key (`radio_ptt`) is held.
    pub ptt: bool,
    /// The bus's own key (`Phonie_Cmd_PTT`) is held.
    bus_ptt: bool,
    sending: bool,
    mic: Option<omsi_audio::twoway::Mic>,
    mic_error: Option<String>,
    encoder: dispatch::Encoder,
    fx: omsi_audio::twoway::RadioFx,
    out: std::sync::Arc<omsi_audio::stream::StreamBuf>,
    voice: Option<omsi_audio::VoiceId>,
    /// A transmission is coming in, its last frame when.
    receiving: Option<Instant>,
}

/// The incoming voice's buffer before it plays (s): enough for a frame that is late.
const PREBUFFER: f32 = 0.12;
/// After this long without a frame, the other side let go of its key.
const RX_END: Duration = Duration::from_millis(180);

impl Radio {
    pub(crate) fn new() -> Radio {
        Radio {
            heard: None,
            call: None,
            talk: Talk::Nobody,
            request: Request::None,
            on_duty: false,
            requested: None,
            ptt: false,
            bus_ptt: false,
            sending: false,
            mic: None,
            mic_error: None,
            encoder: dispatch::Encoder::default(),
            fx: omsi_audio::twoway::RadioFx::new(dispatch::RATE),
            out: std::sync::Arc::new(omsi_audio::stream::StreamBuf::live(dispatch::RATE, PREBUFFER)),
            voice: None,
            receiving: None,
        }
    }

    /// The server runs a radio (it told us where ours stands lately).
    pub(crate) fn available(&self) -> bool {
        self.heard.is_some_and(|t| t.elapsed() < STATE_GONE)
    }

    /// The server's `radio …` command.
    pub(crate) fn on_command(&mut self, arg: &str) {
        let mut w = arg.split_whitespace();
        if w.next() != Some("state") {
            return;
        }
        let call = CallKind::parse(w.next().unwrap_or("none"));
        let talk = Talk::parse(w.next().unwrap_or("-"));
        let request = Request::parse(w.next().unwrap_or("none"));
        let was = (self.call, self.request);
        self.heard = Some(Instant::now());
        self.call = call;
        self.talk = talk;
        self.request = request;
        self.on_duty = w.next() != Some("off");
        // answered (called back) or dropped: our request is over (a state that left before
        // the request reached the server says nothing of it)
        if (was.1 != Request::None && request == Request::None) || call.is_some() {
            self.requested = None;
        }
        // the terminal's tones: a call beginning (individual, or selective and general), any
        // call ending
        use omsi_audio::twoway::Tone;
        match (was.0, call) {
            (w, Some(c)) if w != Some(c) => self.play_tone(if c == CallKind::Individual { Tone::IndividualStart } else { Tone::GroupStart }),
            (Some(_), None) => self.play_tone(Tone::CallEnd),
            _ => {}
        }
    }

    /// Our request is out and not yet over (answered, dropped or a minute old).
    fn requesting(&self) -> bool {
        self.requested.is_some_and(|t| t.elapsed() < REQUEST_WAIT)
    }

    /// The button can be pressed: a dispatcher on duty, no call, no request of ours out.
    pub(crate) fn can_request(&self) -> bool {
        self.available() && self.on_duty && self.call.is_none() && !self.requesting()
    }

    /// The button: a request to be called (there is no taking it back).
    pub(crate) fn request(&mut self, lan: &mut LanSession) {
        if !self.can_request() {
            return;
        }
        self.requested = Some(Instant::now());
        lan.command(1, "radio request");
    }

    fn play(&mut self, samples: &[f32]) {
        self.out.push(dispatch::RATE, samples.iter().map(|&s| [s, s]));
    }

    /// One of the terminal's tones, and a little silence after it: the buffer starts once it
    /// holds `PREBUFFER` (the release tone alone is shorter, it waited there unheard).
    fn play_tone(&mut self, t: omsi_audio::twoway::Tone) {
        self.play(omsi_audio::twoway::tone(t));
        self.play(&[0.0; (PREBUFFER * dispatch::RATE as f32) as usize]);
    }

    /// Once a frame: the voices that came, ours sent while the key is down.
    pub(crate) fn tick(&mut self, lan: &mut LanSession, audio: Option<&omsi_audio::AudioEngine>, gain: f32) {
        if self.heard.is_some() && !self.available() {
            // the server went quiet: no call, no request
            self.call = None;
            self.talk = Talk::Nobody;
            self.request = Request::None;
            self.requested = None;
        }
        // what comes in, through the radio's sound
        for (_speaker, bytes) in lan.take_radio() {
            let Some(frame) = Frame::from_bytes(&bytes) else {
                continue;
            };
            if self.receiving.is_none() {
                let open = self.fx.open();
                self.play(&open);
            }
            self.receiving = Some(Instant::now());
            let samples = self.fx.voice(&frame.decode());
            self.play(&samples);
        }
        // the other side let go of its key: in an individual call the release tone, in the
        // others the squelch closing (a call that ended has its own tone)
        if self.receiving.is_some_and(|t| t.elapsed() > RX_END) {
            self.receiving = None;
            match self.call {
                Some(CallKind::Individual) => self.play_tone(omsi_audio::twoway::Tone::PttRelease),
                Some(_) => {
                    let tail = self.fx.tail();
                    self.play(&tail);
                }
                None => {}
            }
        }
        if let Some(audio) = audio {
            if self.voice.is_none_or(|v| !audio.is_playing(v)) {
                self.voice = Some(audio.play_stream(self.out.clone(), omsi_audio::VoiceParams { gain, important: true, ..Default::default() }));
            }
        }
        // the microphone: open while an individual call lasts
        let may_talk = self.call == Some(CallKind::Individual);
        if may_talk && self.mic.is_none() && self.mic_error.is_none() {
            match omsi_audio::twoway::Mic::open(dispatch::RATE) {
                Ok(m) => {
                    log::info!("radio: microphone {}", m.name);
                    self.mic = Some(m);
                }
                Err(e) => {
                    log::warn!("radio: no microphone: {e}");
                    self.mic_error = Some(e);
                }
            }
        } else if !may_talk {
            self.mic = None;
            self.mic_error = None;
        }
        let ptt = self.ptt || self.bus_ptt;
        let send = ptt && may_talk && self.talk != Talk::Dispatcher;
        if send && !self.sending {
            self.encoder.restart();
            if let Some(m) = self.mic.as_ref() {
                m.clear();
            }
        }
        let was_sending = self.sending;
        self.sending = send && self.mic.is_some();
        // our own key let go, in an individual call: the release tone here as well
        if was_sending && !self.sending && !ptt && may_talk {
            self.play_tone(omsi_audio::twoway::Tone::PttRelease);
        }
        if let Some(m) = self.mic.as_ref() {
            while let Some(s) = m.take(dispatch::FRAME_SAMPLES) {
                if self.sending {
                    let f = self.encoder.encode(&s);
                    lan.send_radio(1, lan.my_id, &f.to_bytes());
                }
            }
        }
    }

    /// The bus's scripts and the radio, once a frame before `tick`: what the radio does
    /// into the variables the bus declares, a request and the key from them.
    pub(crate) fn bus_link(&mut self, bus: &mut impl BusVars, lan: &mut LanSession) {
        // the cab's terminal: its call button, its key
        if bus.var("Phonie_Cmd_Request").is_some_and(|x| x >= 0.5) {
            bus.set_var("Phonie_Cmd_Request", 0.0);
            self.request(lan);
        }
        self.bus_ptt = bus.var("Phonie_Cmd_PTT").is_some_and(|x| x >= 0.5);
        let hud = self.hud();
        let state = match hud.as_ref().map(|h| h.button) {
            Some(Button::Idle) => 0.0,
            Some(Button::Requested) => 1.0,
            Some(Button::InCall) => 2.0,
            Some(Button::Unavailable) | None => 3.0,
        };
        let call = match self.call.filter(|_| self.available()) {
            None => 0.0,
            Some(CallKind::Individual) => 1.0,
            Some(CallKind::Selective) => 2.0,
            Some(CallKind::General) => 3.0,
        };
        let talk = match self.talk {
            Talk::Nobody => 0.0,
            Talk::Dispatcher => 1.0,
            Talk::Driver => 2.0,
        };
        let flag = |b: bool| if b { 1.0 } else { 0.0 };
        bus.set_var("Phonie_State", state);
        bus.set_var("Phonie_Call", call);
        bus.set_var("Phonie_Talk", talk);
        bus.set_var("Phonie_Request", flag(self.requesting()));
        bus.set_var("Phonie_PTT", flag(self.ptt || self.bus_ptt));
        bus.set_var("Phonie_Sending", flag(self.sending));
        bus.set_var("Phonie_Receiving", flag(self.receiving.is_some()));
    }

    pub(crate) fn hud(&self) -> Option<RadioHud> {
        // (a server that never ran a radio shows no button; one whose radio went, the radio
        // unavailable)
        self.heard?;
        let button = if !self.available() || !self.on_duty {
            Button::Unavailable
        } else if self.call.is_some() {
            Button::InCall
        } else if self.requesting() {
            Button::Requested
        } else {
            Button::Idle
        };
        Some(RadioHud {
            call: self.call.filter(|_| self.available()),
            talk: self.talk,
            button,
            can_request: self.can_request(),
            sending: self.sending,
            blocked: (self.ptt || self.bus_ptt) && !self.sending && self.call.is_some(),
            mic_error: self.mic_error.clone(),
        })
    }
}

impl Drop for Radio {
    fn drop(&mut self) {
        self.out.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_line_is_read() {
        let mut r = Radio::new();
        assert!(!r.available());
        assert!(r.hud().is_none(), "no radio on the server: no button");
        r.on_command("state none - none off");
        assert!(r.available() && !r.can_request());
        assert_eq!(r.hud().unwrap().button, Button::Unavailable);
        r.on_command("state none - none on");
        assert!(r.can_request());
        assert_eq!(r.hud().unwrap().button, Button::Idle);
        // a request out: the button says so and cannot be pressed again
        r.requested = Some(Instant::now());
        r.on_command("state none - pending on");
        assert_eq!(r.hud().unwrap().button, Button::Requested);
        assert!(!r.can_request());
        // a minute without an answer: over for the driver, whatever the server still says
        r.requested = Some(Instant::now() - REQUEST_WAIT - Duration::from_secs(1));
        r.on_command("state none - taken on");
        assert_eq!(r.hud().unwrap().button, Button::Idle);
        assert!(r.can_request());
        // called back
        r.on_command("state individual dispatcher none on");
        assert_eq!(r.call, Some(CallKind::Individual));
        assert_eq!(r.talk, Talk::Dispatcher);
        assert_eq!(r.hud().unwrap().button, Button::InCall);
        assert!(!r.can_request());
        // something else of the radio's is no state
        r.on_command("hello");
        assert_eq!(r.call, Some(CallKind::Individual));
        // an older server says nothing of its dispatcher: taken for on duty
        r.on_command("state none - none");
        assert!(r.can_request());
    }

    /// The consoles' hub is the process's: the tests that use it take turns (each would take
    /// the other's messages).
    fn hub_test() -> std::sync::MutexGuard<'static, ()> {
        static HUB_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let guard = HUB_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let _ = dispatch::take_console_input();
        guard
    }

    /// A server with its radio, a driver's game and a dispatcher's console, all on the
    /// loopback: the request, taken, the call back, the voices both ways, the dispatcher's
    /// key over the driver's, the call's end.
    #[test]
    fn a_driver_asks_and_the_dispatcher_calls_back() {
        let _hub = hub_test();
        use omsi_net::{Pose, WorldInfo};
        let world = || WorldInfo { map: "m".into(), date: "2026-10-05".into(), time: 36000.0, weather: String::new(), season: String::new() };
        let mut host = LanSession::host(27940, "Server", world(), true).unwrap();
        let port = host.local_addr().unwrap().port();
        let mut driver = LanSession::join(&port.to_string(), "driver", world(), Duration::from_secs(1)).unwrap();
        let mut radio = RadioServer::new();
        let (console, rx) = dispatch::register_console();
        let pose = |x: f64| Pose { name: "p".into(), bus: "Vehicles/x.bus".into(), flags: omsi_net::FLAG_VEHICLE, x, ..Default::default() };
        let mut told: Vec<String> = Vec::new();
        let mut to_console: Vec<ConsoleOut> = Vec::new();
        let run = |host: &mut LanSession, driver: &mut LanSession, radio: &mut RadioServer, rounds: usize, told: &mut Vec<String>, out: &mut Vec<ConsoleOut>| {
            for _ in 0..rounds {
                host.tick(0.05, &pose(0.0));
                driver.tick(0.05, &pose(50.0));
                for (from, text) in host.take_commands() {
                    if let Some(arg) = text.strip_prefix("radio ") {
                        radio.command(from, arg);
                    }
                }
                radio.tick(host, 0.05);
                told.extend(driver.take_commands().into_iter().filter_map(|(_, t)| t.strip_prefix("radio ").map(str::to_string)));
                out.extend(rx.try_iter());
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        for _ in 0..60 {
            run(&mut host, &mut driver, &mut radio, 1, &mut told, &mut to_console);
            if driver.connected && host.peers().any(|p| p.has_pose) {
                break;
            }
        }
        let me = driver.my_id;
        let last_state = |told: &[String]| told.iter().rev().find(|t| t.starts_with("state")).cloned().unwrap_or_default();
        let console_state = |out: &[ConsoleOut]| {
            out.iter()
                .rev()
                .find_map(|m| match m {
                    ConsoleOut::Text(t) => serde_json::from_str::<Value>(t).ok(),
                    _ => None,
                })
                .unwrap_or(Value::Null)
        };
        // the driver's button: a request, which the console sees
        driver.command(1, "radio request");
        run(&mut host, &mut driver, &mut radio, 20, &mut told, &mut to_console);
        assert_eq!(last_state(&told), "state none - pending on");
        let st = console_state(&to_console);
        assert_eq!(st["requests"][0]["player"], me);
        assert_eq!(st["requests"][0]["taken"], false);
        // taken
        dispatch::console_said(ConsoleIn::Text(console, json!({"t": "take", "player": me}).to_string()));
        run(&mut host, &mut driver, &mut radio, 10, &mut told, &mut to_console);
        assert_eq!(last_state(&told), "state none - taken on");
        // called back: the request is answered
        dispatch::console_said(ConsoleIn::Text(console, json!({"t": "call", "kind": "individual", "players": [me]}).to_string()));
        run(&mut host, &mut driver, &mut radio, 10, &mut told, &mut to_console);
        assert_eq!(last_state(&told), "state individual - none on");
        assert_eq!(console_state(&to_console)["call"]["kind"], "individual");
        // the driver talks: the console hears it, the driver's id first
        let frame = dispatch::Encoder::default().encode(&[0.2; dispatch::FRAME_SAMPLES]).to_bytes();
        to_console.clear();
        driver.send_radio(1, me, &frame);
        run(&mut host, &mut driver, &mut radio, 10, &mut told, &mut to_console);
        let heard: Vec<Vec<u8>> = to_console.iter().filter_map(|m| if let ConsoleOut::Binary(b) = m { Some(b.clone()) } else { None }).collect();
        assert_eq!(heard.len(), 1);
        assert_eq!(heard[0][..4], me.to_le_bytes());
        assert_eq!(heard[0][4..], frame[..]);
        // the dispatcher's key down: the driver hears the dispatcher, the console no longer
        // hears the driver
        dispatch::console_said(ConsoleIn::Text(console, json!({"t": "ptt", "on": true}).to_string()));
        dispatch::console_said(ConsoleIn::Voice(console, frame.clone()));
        run(&mut host, &mut driver, &mut radio, 5, &mut told, &mut to_console);
        to_console.clear();
        driver.send_radio(1, me, &frame);
        run(&mut host, &mut driver, &mut radio, 10, &mut told, &mut to_console);
        assert!(!to_console.iter().any(|m| matches!(m, ConsoleOut::Binary(_))), "the driver came through over the dispatcher");
        assert_eq!(driver.take_radio().iter().filter(|(who, _)| *who == dispatch::DISPATCHER).count(), 1);
        assert_eq!(last_state(&told), "state individual dispatcher none on");
        // the dispatcher ends the call (the driver cannot)
        dispatch::console_said(ConsoleIn::Text(console, json!({"t": "ptt", "on": false}).to_string()));
        dispatch::console_said(ConsoleIn::Text(console, json!({"t": "end"}).to_string()));
        run(&mut host, &mut driver, &mut radio, 10, &mut told, &mut to_console);
        assert_eq!(last_state(&told), "state none - none on");
        dispatch::unregister_console(console);
        // no console left: no dispatcher on duty
        run(&mut host, &mut driver, &mut radio, 5, &mut told, &mut to_console);
        assert_eq!(last_state(&told), "state none - none off");
    }

    #[test]
    fn a_tone_is_enough_for_the_buffer_to_start() {
        // (the release tone alone is shorter than the buffer waits for before it plays)
        assert!((omsi_audio::twoway::tone(omsi_audio::twoway::Tone::PttRelease).len() as f32) < PREBUFFER * dispatch::RATE as f32);
        let mut r = Radio::new();
        r.play_tone(omsi_audio::twoway::Tone::PttRelease);
        assert!(r.out.buffered() >= PREBUFFER);
    }

    /// A bus whose varlist declares some of the radio's variables.
    struct Vars(std::collections::HashMap<String, f32>);

    impl BusVars for Vars {
        fn var(&self, name: &str) -> Option<f32> {
            self.0.get(name).copied()
        }

        fn set_var(&mut self, name: &str, v: f32) -> bool {
            match self.0.get_mut(name) {
                Some(x) => {
                    *x = v;
                    true
                }
                None => false,
            }
        }
    }

    #[test]
    fn the_bus_scripts_see_the_radio_and_work_it() {
        use omsi_net::WorldInfo;
        let world = || WorldInfo { map: "m".into(), date: "2026-10-06".into(), time: 36000.0, weather: String::new(), season: String::new() };
        let mut lan = LanSession::host(27960, "Server", world(), true).unwrap();
        let mut bus = Vars(["Phonie_State", "Phonie_Call", "Phonie_Talk", "Phonie_Request", "Phonie_PTT", "Phonie_Cmd_Request", "Phonie_Cmd_PTT"].iter().map(|n| (n.to_string(), 0.0)).collect());
        let mut r = Radio::new();
        // no radio on the server: unavailable; a variable the bus does not declare stays away
        r.bus_link(&mut bus, &mut lan);
        assert_eq!(bus.0["Phonie_State"], 3.0);
        assert!(!bus.0.contains_key("Phonie_Sending"));
        r.on_command("state none - none on");
        r.bus_link(&mut bus, &mut lan);
        assert_eq!(bus.0["Phonie_State"], 0.0);
        // the cab's call button: a request goes out, the command is taken back
        bus.0.insert("Phonie_Cmd_Request".into(), 1.0);
        r.bus_link(&mut bus, &mut lan);
        assert_eq!(bus.0["Phonie_Cmd_Request"], 0.0);
        assert!(r.requesting());
        r.bus_link(&mut bus, &mut lan);
        assert_eq!((bus.0["Phonie_State"], bus.0["Phonie_Request"]), (1.0, 1.0));
        // called back, the dispatcher talking; the cab's key held
        r.on_command("state individual dispatcher none on");
        bus.0.insert("Phonie_Cmd_PTT".into(), 1.0);
        r.bus_link(&mut bus, &mut lan);
        assert_eq!((bus.0["Phonie_State"], bus.0["Phonie_Call"], bus.0["Phonie_Talk"], bus.0["Phonie_PTT"]), (2.0, 1.0, 1.0, 1.0));
        assert!(r.hud().unwrap().blocked, "the dispatcher's key wins over the cab's");
        r.on_command("state general - none on");
        r.bus_link(&mut bus, &mut lan);
        assert_eq!(bus.0["Phonie_Call"], 3.0);
    }

    /// A call reaches the players driving a bus of their own: one on foot is in no call.
    #[test]
    fn a_player_on_foot_is_not_called() {
        let _hub = hub_test();
        use omsi_net::{Pose, WorldInfo};
        let world = || WorldInfo { map: "m".into(), date: "2026-10-06".into(), time: 36000.0, weather: String::new(), season: String::new() };
        let mut host = LanSession::host(27970, "Server", world(), true).unwrap();
        let port = host.local_addr().unwrap().port();
        let mut driver = LanSession::join(&port.to_string(), "driver", world(), Duration::from_secs(1)).unwrap();
        let mut walker = LanSession::join(&port.to_string(), "walker", world(), Duration::from_secs(1)).unwrap();
        let bus = Pose { name: "driver".into(), bus: "Vehicles/x.bus".into(), flags: omsi_net::FLAG_VEHICLE, x: 50.0, ..Default::default() };
        let afoot = Pose { name: "walker".into(), x: 80.0, ..Default::default() };
        let mut radio = RadioServer::new();
        let (console, rx) = dispatch::register_console();
        let mut last = Value::Null;
        let mut run = |host: &mut LanSession, driver: &mut LanSession, walker: &mut LanSession, radio: &mut RadioServer, rounds: usize| {
            for _ in 0..rounds {
                host.tick(0.05, &Pose::default());
                driver.tick(0.05, &bus);
                walker.tick(0.05, &afoot);
                radio.tick(host, 0.05);
                for m in rx.try_iter() {
                    if let ConsoleOut::Text(t) = m {
                        last = serde_json::from_str(&t).unwrap_or(Value::Null);
                    }
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            last.clone()
        };
        let mut st = Value::Null;
        for _ in 0..80 {
            st = run(&mut host, &mut driver, &mut walker, &mut radio, 1);
            if st["players"].as_array().is_some_and(|a| a.len() == 2) && st["players"][0]["bus"] != st["players"][1]["bus"] {
                break;
            }
        }
        let (d, w) = (driver.my_id, walker.my_id);
        let bus_of = |st: &Value, id: u32| st["players"].as_array().unwrap().iter().find(|p| p["id"] == id).map(|p| p["bus"].clone());
        assert_eq!(bus_of(&st, d), Some(json!(true)));
        assert_eq!(bus_of(&st, w), Some(json!(false)));
        // the general call: the driver alone
        dispatch::console_said(ConsoleIn::Text(console, json!({"t": "call", "kind": "general"}).to_string()));
        let st = run(&mut host, &mut driver, &mut walker, &mut radio, 6);
        assert_eq!(st["call"]["players"], json!([d]));
        // the walker called on its own: no call
        dispatch::console_said(ConsoleIn::Text(console, json!({"t": "end"}).to_string()));
        dispatch::console_said(ConsoleIn::Text(console, json!({"t": "call", "kind": "individual", "players": [w]}).to_string()));
        let st = run(&mut host, &mut driver, &mut walker, &mut radio, 6);
        assert!(st["call"].is_null(), "{st}");
        dispatch::unregister_console(console);
    }

    #[test]
    fn kinds_and_words_go_both_ways() {
        for k in [CallKind::Individual, CallKind::Selective, CallKind::General] {
            assert_eq!(CallKind::parse(k.word()), Some(k));
        }
        for t in [Talk::Nobody, Talk::Dispatcher, Talk::Driver] {
            assert_eq!(Talk::parse(t.word()), t);
        }
        for q in [Request::None, Request::Pending, Request::Taken] {
            assert_eq!(Request::parse(q.word()), q);
        }
        assert_eq!(CallKind::parse("none"), None);
    }
}
