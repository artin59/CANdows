//! Linux `struct can_frame` semantics plus the can-utils text formats
//! (ported from can-utils lib.c / candump.c so output matches byte for byte).

use std::fmt::Write;

pub const CAN_EFF_FLAG: u32 = 0x8000_0000;
pub const CAN_RTR_FLAG: u32 = 0x4000_0000;
pub const CAN_ERR_FLAG: u32 = 0x2000_0000;
pub const CAN_SFF_MASK: u32 = 0x0000_07FF;
pub const CAN_EFF_MASK: u32 = 0x1FFF_FFFF;

/// `id` carries the EFF/RTR/ERR flag bits exactly like Linux `can_id`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CanFrame {
    pub id: u32,
    pub len: u8,
    pub data: [u8; 8],
}

fn nib(c: Option<&u8>) -> Option<u8> {
    (*c? as char).to_digit(16).map(|d| d as u8)
}

/// Parse a cansend frame string (`123#DEADBEEF`, `12345678#R`, `5A1#11.22`).
/// Classic CAN only: CAN FD (`##`), CAN XL and error frames are rejected.
pub fn parse_cansend(s: &str) -> Option<CanFrame> {
    let b = s.as_bytes();
    let mut f = CanFrame::default();
    let mut idx;
    let id_digits = |n: usize| (0..n).try_fold(0u32, |id, i| Some(id << 4 | nib(b.get(i))? as u32));

    if b.len() >= 4 && b[3] == b'#' {
        f.id = id_digits(3)?;
        idx = 4;
    } else if b.len() >= 21 && b[5] == b'#' && b[20] == b'#' {
        return None; // CAN XL
    } else if b.len() >= 9 && b[8] == b'#' {
        f.id = id_digits(8)?;
        if f.id & CAN_ERR_FLAG != 0 {
            return None;
        }
        f.id |= CAN_EFF_FLAG;
        idx = 9;
    } else {
        return None;
    }

    if matches!(b.get(idx), Some(b'R' | b'r')) {
        f.id |= CAN_RTR_FLAG;
        if let Some(n) = nib(b.get(idx + 1)).filter(|&n| n <= 8) {
            f.len = n;
        }
        return Some(f);
    }
    if b.get(idx) == Some(&b'#') {
        return None; // CAN FD
    }

    // Like can-utils: up to 8 bytes, '.' separators allowed, anything after byte 8 ignored.
    for i in 0..8 {
        if b.get(idx) == Some(&b'.') {
            idx += 1;
        }
        if idx >= b.len() {
            break;
        }
        f.data[i] = nib(b.get(idx))? << 4 | nib(b.get(idx + 1))?;
        idx += 2;
        f.len += 1;
    }
    Some(f)
}

fn put_id(s: &mut String, f: &CanFrame, indent_sff: bool) {
    if f.id & CAN_EFF_FLAG != 0 {
        write!(s, "{:08X}", f.id & CAN_EFF_MASK).unwrap();
    } else {
        if indent_sff {
            s.push_str("     ");
        }
        write!(s, "{:03X}", f.id & CAN_SFF_MASK).unwrap();
    }
}

/// candump default view: `123   [4]  DE AD BE EF` (snprintf_long_canframe).
pub fn format_long(f: &CanFrame, indent_sff: bool) -> String {
    let mut s = String::new();
    put_id(&mut s, f, indent_sff);
    write!(s, "   [{}] ", f.len).unwrap();
    if f.id & CAN_RTR_FLAG != 0 {
        s.push_str(" remote request");
    } else {
        for b in &f.data[..f.len as usize] {
            write!(s, " {b:02X}").unwrap();
        }
    }
    s
}

/// Log / -L view: `123#DEADBEEF`, `123#R`, `123#R4` (snprintf_canframe).
pub fn format_compact(f: &CanFrame) -> String {
    let mut s = String::new();
    put_id(&mut s, f, false);
    s.push('#');
    if f.id & CAN_RTR_FLAG != 0 {
        s.push('R');
        if f.len > 0 {
            write!(s, "{:X}", f.len).unwrap();
        }
    } else {
        for b in &f.data[..f.len as usize] {
            write!(s, "{b:02X}").unwrap();
        }
    }
    s
}

/// candump sprint_timestamp for modes 'a', 'd', 'z' (anything else: empty).
/// `last_us == 0` means "not initialised yet", as in candump.
pub fn format_timestamp(mode: u8, ts_us: u64, last_us: &mut u64) -> String {
    match mode {
        b'a' => format!("({:010}.{:06}) ", ts_us / 1_000_000, ts_us % 1_000_000),
        b'd' | b'z' => {
            if *last_us == 0 {
                *last_us = ts_us;
            }
            let diff = ts_us.saturating_sub(*last_us);
            if mode == b'd' {
                *last_us = ts_us;
            }
            format!("({:03}.{:06}) ", diff / 1_000_000, diff % 1_000_000)
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fr(id: u32, data: &[u8]) -> CanFrame {
        let mut f = CanFrame { id, len: data.len() as u8, ..Default::default() };
        f.data[..data.len()].copy_from_slice(data);
        f
    }

    #[test]
    fn parse_ok() {
        assert_eq!(parse_cansend("123#DEADBEEF"), Some(fr(0x123, &[0xDE, 0xAD, 0xBE, 0xEF])));
        assert_eq!(parse_cansend("5A1#11.2233.44556677.88"), Some(fr(0x5A1, &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88])));
        assert_eq!(parse_cansend("5AA#"), Some(fr(0x5AA, &[])));
        assert_eq!(parse_cansend("1F334455#11"), Some(fr(0x9F33_4455, &[0x11])));
        assert_eq!(parse_cansend("123#R"), Some(fr(0x4000_0123, &[])));
        let r3 = parse_cansend("00000123#R3").unwrap();
        assert_eq!((r3.id, r3.len), (CAN_EFF_FLAG | CAN_RTR_FLAG | 0x123, 3));
        assert_eq!(parse_cansend("123#deadbeef"), Some(fr(0x123, &[0xDE, 0xAD, 0xBE, 0xEF])));
    }

    #[test]
    fn parse_reject() {
        for s in ["", "12#11", "123", "12G#11", "123#1", "123#XY", "123##1", "45123#81:00:12345678#11", "20000123#11"] {
            assert_eq!(parse_cansend(s), None, "{s}");
        }
    }

    #[test]
    fn long_format_matches_candump() {
        assert_eq!(format_long(&fr(0x123, &[0xDE, 0xAD, 0xBE, 0xEF]), false), "123   [4]  DE AD BE EF");
        assert_eq!(format_long(&fr(0x123, &[0xDE]), true), "     123   [1]  DE");
        assert_eq!(format_long(&fr(0x8000_1234, &[0x11, 0x22]), false), "00001234   [2]  11 22");
        assert_eq!(format_long(&fr(0x7FF, &[]), false), "7FF   [0] ");
        let mut rtr = fr(0x4000_0123, &[]);
        rtr.len = 2;
        assert_eq!(format_long(&rtr, false), "123   [2]  remote request");
    }

    #[test]
    fn compact_format_matches_candump() {
        assert_eq!(format_compact(&fr(0x123, &[0xDE, 0xAD])), "123#DEAD");
        assert_eq!(format_compact(&fr(0x8000_0123, &[])), "00000123#");
        let mut rtr = fr(0x4000_0123, &[]);
        assert_eq!(format_compact(&rtr), "123#R");
        rtr.len = 8;
        assert_eq!(format_compact(&rtr), "123#R8");
    }

    #[test]
    fn timestamps() {
        let mut last = 0;
        assert_eq!(format_timestamp(b'a', 1_698_000_000_123_456, &mut last), "(1698000000.123456) ");
        assert_eq!(format_timestamp(b'z', 5_000_000, &mut last), "(000.000000) ");
        assert_eq!(format_timestamp(b'z', 6_500_001, &mut last), "(001.500001) ");
        let mut last = 0;
        format_timestamp(b'd', 5_000_000, &mut last);
        assert_eq!(format_timestamp(b'd', 5_000_250, &mut last), "(000.000250) ");
        assert_eq!(format_timestamp(b'd', 5_000_300, &mut last), "(000.000050) ");
        assert_eq!(format_timestamp(0, 1, &mut last), "");
    }
}
