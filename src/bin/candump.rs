//! `candump [-t a|d|z] [-L] [-l] can0`, output identical to can-utils candump.

use std::fs::File;
use std::io::{LineWriter, Write};
use std::process::exit;
use candows::client::{K_FRAME, Record, connect};
use candows::frame::{CAN_EFF_FLAG, format_compact, format_long, format_timestamp};

fn usage() -> ! {
    eprintln!(
        "candump - dump CAN bus traffic.\n\
         \n\
         Usage: candump [options] <CAN interface>\n  \
         (use CTRL-C to terminate candump)\n\
         \n\
         Options:\n         \
         -t <type>   (timestamp: (a)bsolute/(d)elta/(z)ero)\n         \
         -l          (log CAN-frames into file. Sets '-s 2' by default)\n         \
         -L          (use log file format on stdout)\n"
    );
    exit(1);
}

fn main() {
    let (mut timestamp, mut logtimestamp, mut log, mut logfrmt) = (0u8, b'a', false, false);
    let mut ifname = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-l" => log = true,
            "-L" => logfrmt = true,
            "-h" | "-?" => usage(),
            t if t.starts_with("-t") => {
                let v = if t.len() > 2 { t[2..].to_string() } else { args.next().unwrap_or_else(|| usage()) };
                let c = v.bytes().next().unwrap_or(0);
                timestamp = c;
                logtimestamp = if c == b'z' { b'z' } else { b'a' };
                if !matches!(c, b'a' | b'd' | b'z') {
                    eprintln!("candump: unknown timestamp mode '{}' - ignored", c as char);
                    timestamp = 0;
                }
            }
            o if o.starts_with('-') => {
                eprintln!("candump: option {o} is not supported");
                usage();
            }
            i if ifname.is_none() => ifname = Some(i.to_string()),
            _ => {
                eprintln!("candump: only one interface is supported");
                exit(1);
            }
        }
    }
    let Some(ifname) = ifname else { usage() };
    if ifname.contains(',') {
        eprintln!("candump: filters are not supported");
        exit(1);
    }

    let mut s = connect(&ifname).unwrap_or_else(|e| {
        eprintln!("{e}");
        exit(1)
    });

    let mut logfile = if log {
        eprintln!("Disabled standard output while logging.");
        let name = chrono::Local::now().format("candump-%Y-%m-%d_%H%M%S.log").to_string();
        eprintln!("Enabling Logfile '{name}'");
        // Line-buffered so Ctrl+C never loses logged frames.
        Some(LineWriter::new(File::create(&name).unwrap_or_else(|e| {
            eprintln!("logfile: {e}");
            exit(1)
        })))
    } else {
        None
    };

    let mut out = std::io::stdout().lock();
    let (mut last_ts, mut last_logts, mut indent_sff) = (0u64, 0u64, false);
    loop {
        let r = match Record::read_from(&mut s) {
            Ok(r) if r.kind == K_FRAME => r,
            Ok(_) => continue,
            Err(_) => {
                eprintln!("read: Network is down");
                exit(1);
            }
        };
        let f = r.frame;
        if f.id & CAN_EFF_FLAG != 0 {
            indent_sff = true;
        }
        if log || logfrmt {
            let line = format!("{}{} {}", format_timestamp(logtimestamp, r.ts_us, &mut last_logts), ifname, format_compact(&f));
            if let Some(lf) = &mut logfile {
                if writeln!(lf, "{line}").is_err() {
                    eprintln!("logfile: write failed");
                    exit(1);
                }
                continue; // logging silences stdout
            }
            let _ = writeln!(out, "{line}");
            continue;
        }
        let ts = format_timestamp(timestamp, r.ts_us, &mut last_ts);
        let _ = writeln!(out, " {ts} {ifname}  {}", format_long(&f, indent_sff));
    }
}
