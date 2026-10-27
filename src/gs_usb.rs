//! gs_usb protocol spoken by candleLight firmware (mirrors Linux drivers/net/can/usb/gs_usb.c).

use crate::frame::CanFrame;

pub const VID: u16 = 0x1d50;
pub const PID: u16 = 0x606f;

pub const BREQ_HOST_FORMAT: u8 = 0;
pub const BREQ_BITTIMING: u8 = 1;
pub const BREQ_MODE: u8 = 2;
pub const BREQ_BT_CONST: u8 = 4;

pub const MODE_RESET: u32 = 0;
pub const MODE_START: u32 = 1;

pub const ECHO_ID_RX: u32 = 0xFFFF_FFFF;
/// Same in-flight limit as Linux GS_MAX_TX_URBS.
pub const MAX_TX: usize = 10;
/// Classic gs_host_frame without hardware timestamp.
pub const HOST_FRAME_LEN: usize = 20;

fn le(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap())
}

#[derive(Clone, Copy, Debug)]
pub struct BtConst {
    pub fclk: u32,
    pub tseg1_min: u32,
    pub tseg1_max: u32,
    pub tseg2_min: u32,
    pub tseg2_max: u32,
    pub sjw_max: u32,
    pub brp_min: u32,
    pub brp_max: u32,
    pub brp_inc: u32,
}

impl BtConst {
    pub fn parse(b: &[u8]) -> Option<Self> {
        if b.len() < 40 {
            return None;
        }
        // word 0 is the feature mask; we use no optional features.
        Some(BtConst {
            fclk: le(b, 1),
            tseg1_min: le(b, 2),
            tseg1_max: le(b, 3),
            tseg2_min: le(b, 4),
            tseg2_max: le(b, 5),
            sjw_max: le(b, 6),
            brp_min: le(b, 7),
            brp_max: le(b, 8),
            brp_inc: le(b, 9).max(1),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bittiming {
    pub prop_seg: u32,
    pub phase_seg1: u32,
    pub phase_seg2: u32,
    pub sjw: u32,
    pub brp: u32,
}

impl Bittiming {
    pub fn to_bytes(&self) -> Vec<u8> {
        [self.prop_seg, self.phase_seg1, self.phase_seg2, self.sjw, self.brp]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect()
    }

    pub fn bitrate(&self, fclk: u32) -> u32 {
        fclk / (self.brp * (1 + self.prop_seg + self.phase_seg1 + self.phase_seg2))
    }
}

/// Returns (tseg1, tseg2, sample_point, sample_point_error) — Linux can_update_sample_point.
fn update_sample_point(btc: &BtConst, spr: u32, tseg: u32) -> (u32, u32, u32, u32) {
    let (mut best_err, mut best_sp, mut t1, mut t2) = (u32::MAX, 0, 0, 0);
    for i in 0..=1 {
        let mut tseg2 = (tseg + 1).wrapping_sub(spr * (tseg + 1) / 1000).wrapping_sub(i);
        tseg2 = tseg2.clamp(btc.tseg2_min, btc.tseg2_max);
        let mut tseg1 = tseg.wrapping_sub(tseg2);
        if tseg1 > btc.tseg1_max {
            tseg1 = btc.tseg1_max;
            tseg2 = tseg - tseg1;
        }
        let sp = 1000 * (tseg + 1 - tseg2) / (tseg + 1);
        let err = spr.abs_diff(sp);
        if sp <= spr && err < best_err {
            (best_err, best_sp, t1, t2) = (err, sp, tseg1, tseg2);
        }
    }
    (t1, t2, best_sp, best_err)
}

/// Port of Linux can_calc_bittiming() with its default sample points, so a
/// bitrate gives the same register values as `ip link set ... bitrate N`.
pub fn calc_bittiming(bitrate: u32, btc: &BtConst) -> Result<Bittiming, String> {
    if bitrate == 0 {
        return Err("bitrate must be > 0".into());
    }
    let spr = if bitrate > 800_000 { 750 } else if bitrate > 500_000 { 800 } else { 875 };
    let (mut best_br_err, mut best_sp_err, mut best_tseg, mut best_brp) = (u32::MAX, u32::MAX, 0, 0);

    let mut tseg = (btc.tseg1_max + btc.tseg2_max) * 2 + 1;
    while tseg >= (btc.tseg1_min + btc.tseg2_min) * 2 {
        let tsegall = 1 + tseg / 2;
        let mut brp = (btc.fclk as u64 / (tsegall as u64 * bitrate as u64)) as u32 + tseg % 2;
        brp = brp / btc.brp_inc * btc.brp_inc;
        if (btc.brp_min..=btc.brp_max).contains(&brp) && brp > 0 {
            let br_err = bitrate.abs_diff(btc.fclk / (brp * tsegall));
            if br_err <= best_br_err {
                if br_err < best_br_err {
                    best_sp_err = u32::MAX;
                }
                let (_, _, _, sp_err) = update_sample_point(btc, spr, tseg / 2);
                if sp_err < best_sp_err {
                    (best_sp_err, best_br_err, best_tseg, best_brp) = (sp_err, br_err, tseg / 2, brp);
                    if br_err == 0 && sp_err == 0 {
                        break;
                    }
                }
            }
        }
        if tseg == 0 {
            break;
        }
        tseg -= 1;
    }

    if best_brp == 0 {
        return Err(format!("bitrate {bitrate} not possible with this adapter"));
    }
    if best_br_err != 0 {
        let e = (best_br_err as u64 * 10000 / bitrate as u64).max(1);
        if e > 500 {
            return Err(format!("bitrate error: {}.{:02}% too high", e / 100, e % 100));
        }
    }
    let (tseg1, tseg2, _, _) = update_sample_point(btc, spr, best_tseg);
    Ok(Bittiming {
        prop_seg: tseg1 / 2,
        phase_seg1: tseg1 - tseg1 / 2,
        phase_seg2: tseg2,
        sjw: 1, // Linux default when the user gives no sjw
        brp: best_brp,
    })
}

pub fn encode_frame(echo_id: u32, f: &CanFrame) -> [u8; HOST_FRAME_LEN] {
    let mut b = [0u8; HOST_FRAME_LEN];
    b[0..4].copy_from_slice(&echo_id.to_le_bytes());
    b[4..8].copy_from_slice(&f.id.to_le_bytes());
    b[8] = f.len; // can_dlc; channel/flags/reserved stay 0
    b[12..20].copy_from_slice(&f.data);
    b
}

/// Returns (echo_id, frame). Accepts padded packets longer than 20 bytes.
pub fn decode_frame(b: &[u8]) -> Option<(u32, CanFrame)> {
    if b.len() < HOST_FRAME_LEN {
        return None;
    }
    let mut f = CanFrame { id: le(b, 1), len: b[8].min(8), data: [0; 8] };
    f.data.copy_from_slice(&b[12..20]);
    Some((le(b, 0), f))
}

#[cfg(test)]
mod tests {
    use super::*;

    // bt_const reported by candleLight on STM32F0/F4-class boards (bxCAN, 48 MHz)
    const BXCAN_48: BtConst = BtConst { fclk: 48_000_000, tseg1_min: 1, tseg1_max: 16, tseg2_min: 1, tseg2_max: 8, sjw_max: 4, brp_min: 1, brp_max: 1024, brp_inc: 1 };

    #[test]
    fn linux_result_48mhz_500k() {
        // `ip -d link show can0` on Linux for candleLight @ 500k:
        // tq 125 prop-seg 6 phase-seg1 7 phase-seg2 2 sjw 1 brp 6
        let bt = calc_bittiming(500_000, &BXCAN_48).unwrap();
        assert_eq!(bt, Bittiming { prop_seg: 6, phase_seg1: 7, phase_seg2: 2, sjw: 1, brp: 6 });
    }

    #[test]
    fn standard_rates_are_exact() {
        let fdcan = |fclk| BtConst { fclk, tseg1_min: 1, tseg1_max: 256, tseg2_min: 1, tseg2_max: 128, sjw_max: 128, brp_min: 1, brp_max: 512, brp_inc: 1 };
        for btc in [BXCAN_48, fdcan(80_000_000), fdcan(160_000_000)] {
            for rate in [10_000, 20_000, 50_000, 100_000, 125_000, 250_000, 500_000, 800_000, 1_000_000] {
                let bt = calc_bittiming(rate, &btc).unwrap();
                assert_eq!(bt.bitrate(btc.fclk), rate, "fclk {} rate {rate}", btc.fclk);
                let tq = 1 + bt.prop_seg + bt.phase_seg1 + bt.phase_seg2;
                let sp = 1000 * (tq - bt.phase_seg2) / tq;
                let want = if rate > 800_000 { 750 } else if rate > 500_000 { 800 } else { 875 };
                assert!(sp <= want && want - sp <= 25, "fclk {} rate {rate} sp {sp}", btc.fclk);
            }
        }
    }

    #[test]
    fn impossible_rate_rejected() {
        assert!(calc_bittiming(333, &BXCAN_48).is_err());
    }

    #[test]
    fn frame_round_trip() {
        let f = CanFrame { id: 0x8000_1234, len: 3, data: [1, 2, 3, 0, 0, 0, 0, 0] };
        let b = encode_frame(7, &f);
        assert_eq!(decode_frame(&b), Some((7, f)));
        let mut padded = b.to_vec();
        padded.resize(64, 0);
        assert_eq!(decode_frame(&padded), Some((7, f)));
        assert_eq!(decode_frame(&b[..19]), None);
    }
}
