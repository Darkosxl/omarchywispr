//! Microphone capture.
//!
//! ponytail: spawn `pw-record` instead of linking cpal — PipeWire resamples to
//! 16 kHz mono f32 for us and picks the default source. Swap for cpal if a
//! machine without PipeWire ever matters.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const SAMPLE_RATE: usize = 16_000;

pub struct Recorder {
    child: Child,
    buf: Arc<Mutex<Vec<f32>>>,
    level: Arc<AtomicU32>,
    started: Instant,
    reader: Option<JoinHandle<()>>,
}

impl Recorder {
    pub fn start(device: Option<&str>) -> std::io::Result<Recorder> {
        let mut cmd = Command::new("pw-record");
        cmd.args(["--raw", "--rate", "16000", "--channels", "1", "--format", "f32"]);
        if let Some(d) = device {
            cmd.args(["--target", d]);
        }
        cmd.arg("-")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd.spawn()?;
        let mut out = child.stdout.take().expect("piped stdout");

        let buf = Arc::new(Mutex::new(Vec::with_capacity(SAMPLE_RATE * 30)));
        let level = Arc::new(AtomicU32::new(0));
        let (b, l) = (Arc::clone(&buf), Arc::clone(&level));
        let reader = std::thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            let mut pending: Vec<u8> = Vec::new();
            loop {
                let n = match out.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                pending.extend_from_slice(&chunk[..n]);
                let whole = pending.len() / 4 * 4;
                let samples: Vec<f32> = pending[..whole]
                    .chunks_exact(4)
                    .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .collect();
                pending.drain(..whole);
                if samples.is_empty() {
                    continue;
                }
                let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
                l.store(rms.to_bits(), Relaxed);
                b.lock().unwrap().extend_from_slice(&samples);
            }
        });

        Ok(Recorder { child, buf, level, started: Instant::now(), reader: Some(reader) })
    }

    /// Latest chunk RMS (0..1, speech is usually 0.02–0.2).
    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Relaxed))
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Stop capture and return all samples.
    pub fn finish(mut self) -> Vec<f32> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(r) = self.reader.take() {
            let _ = r.join();
        }
        std::mem::take(&mut *self.buf.lock().unwrap())
    }
}
