//! `cansend can0 123#DEADBEEF`, same syntax and messages as can-utils (classic CAN only).

use std::process::exit;
use candows::client::{E_NOBUFS, K_ERR, K_OK, K_SEND, Record, connect};
use candows::frame::parse_cansend;

fn usage(prg: &str) {
    eprint!(
        "{prg} - send CAN-frames.\n\
         \n\
         Usage: {prg} <device> <can_frame>.\n\
         \n\
         <can_frame>:\n \
         <can_id>#{{data}}          for CAN CC (Classical CAN 2.0B) data frames\n \
         <can_id>#R{{len}}          for CAN CC (Classical CAN 2.0B) remote frames\n\
         \n\
         <can_id>:\n \
         3 (SFF) or 8 (EFF) hex chars\n\
         {{data}}:\n \
         0..8 ASCII hex-values (optionally separated by '.')\n\
         {{len}}:\n \
         an optional 0..8 value as RTR frames can contain a valid dlc field\n\
         \n\
         Examples:\n  \
         5A1#11.2233.44556677.88 / 123#DEADBEEF / 5AA# /\n  \
         1F334455#1122334455667788 / 123#R / 00000123#R3\n\
         \n"
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let prg = "cansend";
    if args.len() != 3 {
        usage(prg);
        exit(1);
    }
    let Some(frame) = parse_cansend(&args[2]) else {
        eprint!("\nWrong CAN-frame format!\n\n");
        usage(prg);
        exit(1);
    };
    let mut s = connect(&args[1]).unwrap_or_else(|e| {
        eprintln!("{e}");
        exit(1)
    });
    if (Record { kind: K_SEND, frame, ..Default::default() }).write_to(&mut s).is_err() {
        eprintln!("write: Network is down");
        exit(1);
    }
    // Skip bus traffic the daemon fans out to us until our reply arrives.
    loop {
        match Record::read_from(&mut s) {
            Ok(r) if r.kind == K_OK => return,
            Ok(r) if r.kind == K_ERR => {
                eprintln!("write: {}", if r.code == E_NOBUFS { "No buffer space available" } else { "Input/output error" });
                exit(1);
            }
            Ok(_) => {}
            Err(_) => {
                eprintln!("write: Network is down");
                exit(1);
            }
        }
    }
}
