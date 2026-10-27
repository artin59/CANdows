//! `canup can0 <bitrate>`: the Windows stand-in for `ip link set can0 up type can bitrate N`.

use std::io::Read;
use std::net::TcpStream;
use std::process::{Command, Stdio, exit};
use std::time::{Duration, Instant};
use candows::client::{ADDR, IFNAME};

fn usage() -> ! {
    eprintln!("Usage: canup {IFNAME} <bitrate>    e.g. canup {IFNAME} 500000");
    exit(1);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let daemon = args.get(1).is_some_and(|a| a == "--daemon");
    let rest = &args[1 + daemon as usize..];
    let [ifname, bitrate] = rest else { usage() };
    let Ok(bitrate) = bitrate.parse::<u32>() else { usage() };
    if ifname != IFNAME {
        eprintln!("Cannot find device \"{ifname}\" (only {IFNAME} is supported)");
        exit(1);
    }
    if !(1..=1_000_000).contains(&bitrate) {
        eprintln!("bitrate must be between 1 and 1000000");
        exit(1);
    }

    if daemon {
        match candows::daemon::setup(bitrate) {
            Ok(d) => d.serve(),
            Err(e) => {
                eprintln!("{e}");
                exit(1);
            }
        }
    }

    if TcpStream::connect(ADDR).is_ok() {
        eprintln!("{IFNAME} is already up. Run `candown {IFNAME}` first to change the bitrate.");
        exit(1);
    }

    let mut cmd = Command::new(std::env::current_exe().expect("own path"));
    cmd.args(["--daemon", ifname, &bitrate.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200; // Ctrl+C in this terminal must not kill can0
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    let mut child = cmd.spawn().unwrap_or_else(|e| {
        eprintln!("cannot start background process: {e}");
        exit(1)
    });

    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        if let Ok(Some(_)) = child.try_wait() {
            let mut err = String::new();
            child.stderr.take().unwrap().read_to_string(&mut err).ok();
            eprint!("{err}");
            exit(1);
        }
        if TcpStream::connect(ADDR).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    eprintln!("timed out waiting for the CANable to start");
    exit(1);
}
