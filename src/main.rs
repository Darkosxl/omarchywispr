//! omarchywispr — local push-to-talk dictation for Omarchy.
//!
//! `omarchywispr daemon [--model P] [--lang tr|auto] [--prompt S] [--device NAME]`
//! `omarchywispr toggle|start|stop|status [--follow]`

mod audio;
mod daemon;
mod stt;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

pub fn sock_path() -> PathBuf {
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(dir).join("omarchywispr.sock")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("");
    match cmd {
        "daemon" => daemon::run(daemon::Opts::parse(&args[1..])),
        "toggle" | "start" | "stop" | "status" => {
            let follow = args.iter().any(|a| a == "--follow");
            client(if follow { "follow" } else { cmd });
        }
        _ => {
            eprintln!("usage: omarchywispr daemon [--model P] [--lang L] [--prompt S] [--device D]");
            eprintln!("       omarchywispr toggle|start|stop|status [--follow]");
            std::process::exit(2);
        }
    }
}

/// Send one command to the daemon and print its reply line(s).
fn client(cmd: &str) {
    let mut s = match UnixStream::connect(sock_path()) {
        Ok(s) => s,
        Err(_) => {
            println!("err daemon not running");
            std::process::exit(1);
        }
    };
    if writeln!(s, "{cmd}").is_err() {
        println!("err write failed");
        std::process::exit(1);
    }
    let mut code = 0;
    for line in BufReader::new(s).lines() {
        let Ok(line) = line else { break };
        println!("{line}");
        if line.starts_with("err") {
            code = 1;
        }
        if cmd != "follow" {
            break;
        }
    }
    std::process::exit(code);
}
