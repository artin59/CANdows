//! Local IPC between the can0 daemon (owner of the USB device) and the CLI tools.
//! Every message is one fixed 24-byte record over a 127.0.0.1 TCP connection.

use crate::frame::CanFrame;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};

pub const IFNAME: &str = "can0";
pub const ADDR: SocketAddr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), 29536);
pub const REC_LEN: usize = 24;

pub const K_FRAME: u8 = 0; // daemon -> client: frame seen on the bus
pub const K_SEND: u8 = 1; // client -> daemon: transmit frame
pub const K_SHUTDOWN: u8 = 2; // client -> daemon: candown
pub const K_OK: u8 = 3; // daemon -> client: send accepted
pub const K_ERR: u8 = 4; // daemon -> client: send failed, `code` = E_*

pub const E_NOBUFS: u8 = 1;
pub const E_IO: u8 = 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Record {
    pub kind: u8,
    pub code: u8,
    pub frame: CanFrame,
    pub ts_us: u64,
}

impl Record {
    pub fn encode(&self) -> [u8; REC_LEN] {
        let mut b = [0u8; REC_LEN];
        b[0] = self.kind;
        b[1] = self.code;
        b[2] = self.frame.len;
        b[4..8].copy_from_slice(&self.frame.id.to_le_bytes());
        b[8..16].copy_from_slice(&self.frame.data);
        b[16..24].copy_from_slice(&self.ts_us.to_le_bytes());
        b
    }

    pub fn decode(b: &[u8; REC_LEN]) -> Self {
        let mut frame = CanFrame { id: u32::from_le_bytes(b[4..8].try_into().unwrap()), len: b[2].min(8), data: [0; 8] };
        frame.data.copy_from_slice(&b[8..16]);
        Record { kind: b[0], code: b[1], frame, ts_us: u64::from_le_bytes(b[16..24].try_into().unwrap()) }
    }

    pub fn write_to(&self, w: &mut impl Write) -> std::io::Result<()> {
        w.write_all(&self.encode())
    }

    pub fn read_from(r: &mut impl Read) -> std::io::Result<Self> {
        let mut b = [0u8; REC_LEN];
        r.read_exact(&mut b)?;
        Ok(Self::decode(&b))
    }
}

/// Connect to a running can0, or explain why not.
pub fn connect(ifname: &str) -> Result<TcpStream, String> {
    if ifname != IFNAME {
        return Err(format!("{ifname}: No such device (only {IFNAME} is supported)"));
    }
    let s = TcpStream::connect(ADDR)
        .map_err(|_| format!("{ifname}: interface is down, run: canup {ifname} <bitrate>"))?;
    s.set_nodelay(true).ok();
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_round_trip() {
        let r = Record { kind: K_FRAME, code: 0, frame: CanFrame { id: 0xC000_0001, len: 8, data: [9; 8] }, ts_us: 1_698_000_000_123_456 };
        assert_eq!(Record::decode(&r.encode()), r);
    }
}
