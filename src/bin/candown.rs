//! `candown can0`: the Windows stand-in for `ip link set can0 down`.

use std::io::Read;
use std::process::exit;
use std::time::Duration;
use candows::client::{IFNAME, K_SHUTDOWN, Record, connect};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, ifname] = &args[..] else {
        eprintln!("Usage: candown {IFNAME}");
        exit(1);
    };
    if ifname != IFNAME {
        eprintln!("Cannot find device \"{ifname}\" (only {IFNAME} is supported)");
        exit(1);
    }
    // Already down is not an error, same as `ip link set down`.
    let Ok(mut s) = connect(ifname) else { return };
    if (Record { kind: K_SHUTDOWN, ..Default::default() }).write_to(&mut s).is_err() {
        return;
    }
    // Wait for the daemon to exit (connection closes) so a following canup works.
    s.set_read_timeout(Some(Duration::from_secs(3))).ok();
    let mut sink = [0u8; 256];
    while matches!(s.read(&mut sink), Ok(n) if n > 0) {}
}
