//! Daemon: unix-socket server + Idle → Recording → Transcribing state machine.
//!
//! Protocol (one line per command): `toggle` | `start` | `stop` | `status` →
//! reply `ok <phase>`; `follow` → stream `{"phase":..,"level":..}` lines
//! until the client disconnects.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::audio::{Recorder, SAMPLE_RATE};
use crate::stt::Stt;

const MIN_RECORDING: Duration = Duration::from_millis(300);
const MAX_RECORDING: Duration = Duration::from_secs(120);
const LEVEL_TICK: Duration = Duration::from_millis(66);
const SILENCE_PEAK: f32 = 0.01;
// Hold-to-talk releases the hotkey a beat before Super; typing while Super is
// still down would fire Hyprland binds, so never type sooner than this.
const OUTPUT_GUARD: Duration = Duration::from_millis(400);

// ponytail: no config file — these flags live on the autostart line.
pub struct Opts {
    pub model: PathBuf,
    pub lang: String,
    pub prompt: Option<String>,
    pub device: Option<String>,
}

impl Opts {
    pub fn parse(args: &[String]) -> Opts {
        let mut o = Opts { model: default_model(), lang: "tr".into(), prompt: None, device: None };
        let mut i = 0;
        while i < args.len() {
            let Some(v) = args.get(i + 1) else {
                eprintln!("option {} needs a value", args[i]);
                std::process::exit(2);
            };
            match args[i].as_str() {
                "--model" => o.model = PathBuf::from(v),
                "--lang" => o.lang = v.clone(),
                "--prompt" => o.prompt = Some(v.clone()),
                "--device" => o.device = Some(v.clone()),
                other => {
                    eprintln!("unknown option {other}");
                    std::process::exit(2);
                }
            }
            i += 2;
        }
        o
    }
}

fn default_model() -> PathBuf {
    let data = std::env::var("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|_| {
        PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share")
    });
    data.join("omarchywispr/ggml-small-q8_0.bin")
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Phase {
    #[default]
    Idle,
    Recording,
    Transcribing,
}

impl Phase {
    fn as_str(self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Recording => "recording",
            Phase::Transcribing => "transcribing",
        }
    }
}

// ponytail: one global mutex; there is exactly one mic and one user.
#[derive(Default)]
struct Shared {
    phase: Phase,
    rec: Option<Recorder>,
    followers: Vec<UnixStream>,
}

struct Daemon {
    state: Mutex<Shared>,
    stt: Stt,
    opts: Opts,
}

type D = Arc<Daemon>;

pub fn run(opts: Opts) {
    whisper_rs::install_logging_hooks();
    let path = crate::sock_path();
    if UnixStream::connect(&path).is_ok() {
        eprintln!("omarchywispr: daemon already running at {}", path.display());
        std::process::exit(1);
    }
    let _ = std::fs::remove_file(&path);

    let t0 = Instant::now();
    let stt = match Stt::load(&opts.model, &opts.lang, opts.prompt.clone()) {
        Ok(s) => s,
        Err(e) => {
            notify("critical", &format!("Model load failed: {e}"));
            std::process::exit(1);
        }
    };
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("omarchywispr: bind {}: {e}", path.display());
            std::process::exit(1);
        }
    };
    eprintln!(
        "omarchywispr: {} loaded in {:?} (lang {}), listening on {}",
        opts.model.display(),
        t0.elapsed(),
        opts.lang,
        path.display()
    );

    let d: D = Arc::new(Daemon { state: Mutex::new(Shared::default()), stt, opts });
    for conn in listener.incoming() {
        let Ok(stream) = conn else { continue };
        let d = Arc::clone(&d);
        thread::spawn(move || handle(&d, stream));
    }
}

fn handle(d: &D, mut stream: UnixStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    let Ok(reader) = stream.try_clone() else { return };
    let mut line = String::new();
    if BufReader::new(reader).read_line(&mut line).is_err() {
        return;
    }
    let reply = match line.trim() {
        "status" => format!("ok {}", phase(d)),
        "start" => {
            start(d);
            format!("ok {}", phase(d))
        }
        "stop" => {
            stop(d);
            format!("ok {}", phase(d))
        }
        "toggle" => {
            // Copy the phase out: holding the guard across start()/stop()
            // would deadlock on their own lock().
            let current = d.state.lock().unwrap().phase;
            match current {
                Phase::Idle => start(d),
                Phase::Recording => stop(d),
                Phase::Transcribing => {}
            }
            format!("ok {}", phase(d))
        }
        "follow" => {
            let mut sh = d.state.lock().unwrap();
            let rms = sh.rec.as_ref().map(Recorder::level).unwrap_or(0.0);
            if stream.write_all(event(sh.phase, rms).as_bytes()).is_ok() {
                sh.followers.push(stream);
            }
            return;
        }
        other => format!("err unknown command {other:?}"),
    };
    let _ = writeln!(stream, "{reply}");
}

fn phase(d: &D) -> &'static str {
    d.state.lock().unwrap().phase.as_str()
}

fn event(phase: Phase, rms: f32) -> String {
    // Speech RMS sits around 0.02–0.2; scale so normal talking fills the bars.
    let level = (rms * 6.0).clamp(0.0, 1.0);
    format!("{{\"phase\":\"{}\",\"level\":{level:.3}}}\n", phase.as_str())
}

fn broadcast(sh: &mut Shared, rms: f32) {
    let msg = event(sh.phase, rms);
    sh.followers.retain_mut(|f| f.write_all(msg.as_bytes()).is_ok());
}

fn start(d: &D) {
    let mut sh = d.state.lock().unwrap();
    if sh.phase != Phase::Idle {
        return;
    }
    match Recorder::start(d.opts.device.as_deref()) {
        Ok(r) => {
            sh.rec = Some(r);
            sh.phase = Phase::Recording;
            broadcast(&mut sh, 0.0);
            let d = Arc::clone(d);
            thread::spawn(move || ticker(&d));
        }
        Err(e) => notify("critical", &format!("Mic capture failed (pw-record): {e}")),
    }
}

/// Streams mic level to followers while recording; enforces MAX_RECORDING.
fn ticker(d: &D) {
    loop {
        thread::sleep(LEVEL_TICK);
        let mut sh = d.state.lock().unwrap();
        if sh.phase != Phase::Recording {
            return;
        }
        let (rms, over) = match sh.rec.as_ref() {
            Some(r) => (r.level(), r.elapsed() > MAX_RECORDING),
            None => return,
        };
        if over {
            drop(sh);
            stop(d);
            return;
        }
        broadcast(&mut sh, rms);
    }
}

fn stop(d: &D) {
    let mut sh = d.state.lock().unwrap();
    if sh.phase != Phase::Recording {
        return;
    }
    let Some(rec) = sh.rec.take() else {
        sh.phase = Phase::Idle;
        return;
    };
    if rec.elapsed() < MIN_RECORDING {
        rec.finish();
        sh.phase = Phase::Idle;
        broadcast(&mut sh, 0.0);
        return;
    }
    sh.phase = Phase::Transcribing;
    broadcast(&mut sh, 0.0);
    drop(sh);

    let d = Arc::clone(d);
    thread::spawn(move || {
        let stopped = Instant::now();
        let samples = rec.finish();
        let t0 = Instant::now();
        // Whisper hallucinates on silence ("Tamam.", "Altyazı M.K."); skip
        // clips whose peak never rises above roughly -40 dBFS.
        let silent = samples.iter().all(|s| s.abs() < SILENCE_PEAK);
        let result = if silent { Ok(String::new()) } else { d.stt.transcribe(&samples) };
        match result {
            Ok(text) if !text.is_empty() => {
                eprintln!(
                    "omarchywispr: {:.1}s audio → {text:?} in {:?}",
                    samples.len() as f32 / SAMPLE_RATE as f32,
                    t0.elapsed()
                );
                if let Some(wait) = OUTPUT_GUARD.checked_sub(stopped.elapsed()) {
                    thread::sleep(wait);
                }
                output(&text);
            }
            Ok(_) => eprintln!("omarchywispr: nothing recognised{}", if silent { " (silence)" } else { "" }),
            Err(e) => notify("critical", &format!("Transcription failed: {e}")),
        }
        let mut sh = d.state.lock().unwrap();
        sh.phase = Phase::Idle;
        broadcast(&mut sh, 0.0);
    });
}

/// Type the text into the focused window; fall back to the clipboard.
fn output(text: &str) {
    if pipe_to(&["wtype", "-"], text) {
        return;
    }
    if pipe_to(&["wl-copy"], text) {
        notify("normal", "Typing failed — text copied to clipboard");
    } else {
        notify("critical", "Typing and clipboard both failed");
    }
}

fn pipe_to(cmd: &[&str], input: &str) -> bool {
    let Ok(mut child) = Command::new(cmd[0])
        .args(&cmd[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
    }
    child.wait().map(|s| s.success()).unwrap_or(false)
}

fn notify(urgency: &str, msg: &str) {
    eprintln!("omarchywispr: {msg}");
    for bin in ["omarchy-notification-send", "notify-send"] {
        let ok = Command::new(bin)
            .args(["-u", urgency, "omarchywispr", msg])
            .stdin(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            return;
        }
    }
}
