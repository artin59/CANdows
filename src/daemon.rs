//! Background process that owns the CANable, doing the kernel's job: it fans
//! bus traffic out to every connected candump/cansend, like SocketCAN sockets.

use crate::client::*;
use crate::frame::{CAN_ERR_FLAG, CanFrame};
use crate::gs_usb::*;
use nusb::descriptors::TransferType;
use nusb::transfer::{Buffer, Bulk, ControlIn, ControlOut, ControlType, Direction, In, Out, Recipient, TransferError};
use nusb::{Endpoint, Interface, MaybeFuture};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const TIMEOUT: Duration = Duration::from_secs(1);
/// Per-client backlog; a stalled client (e.g. paused console) drops frames instead of stalling everyone, like a full socket buffer on Linux.
const CLIENT_QUEUE: usize = 4096;

pub struct Daemon {
    intf: Interface,
    ep_in: Endpoint<Bulk, In>,
    ep_out: Endpoint<Bulk, Out>,
    listener: TcpListener,
}

struct Shared {
    intf: Interface,
    ep_out: Mutex<Endpoint<Bulk, Out>>,
    clients: Mutex<Vec<(u64, SyncSender<Record>)>>,
    /// echo_id -> owning client, freed when the device echoes the frame back (= sent on the bus).
    slots: Mutex<[Option<u64>; MAX_TX]>,
}

fn ctrl_out(intf: &Interface, request: u8, data: &[u8]) -> Result<(), TransferError> {
    let c = ControlOut { control_type: ControlType::Vendor, recipient: Recipient::Interface, request, value: 0, index: 0, data };
    intf.control_out(c, TIMEOUT).wait()
}

fn mode(m: u32) -> [u8; 8] {
    let mut b = [0u8; 8]; // flags = 0: normal mode, no hw timestamps
    b[..4].copy_from_slice(&m.to_le_bytes());
    b
}

fn now_us() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_micros() as u64)
}

/// Open the CANable and start it at `bitrate`. Errors here are shown to the user by canup.
pub fn setup(bitrate: u32) -> Result<Daemon, String> {
    let info = nusb::list_devices()
        .wait()
        .map_err(|e| format!("cannot list USB devices: {e}"))?
        .find(|d| d.vendor_id() == VID && d.product_id() == PID)
        .ok_or("No CANable found (USB 1d50:606f with candleLight firmware). Is it plugged in?")?;
    let dev = info.open().wait().map_err(|e| format!("cannot open CANable: {e}"))?;
    let intf = dev
        .claim_interface(0)
        .wait()
        .map_err(|e| format!("cannot claim CANable, is another program using it? ({e})"))?;

    // candleLight ignores or rejects HOST_FORMAT; a reset clears state left by a crashed daemon.
    let _ = ctrl_out(&intf, BREQ_HOST_FORMAT, &0x0000_beef_u32.to_le_bytes());
    let _ = ctrl_out(&intf, BREQ_MODE, &mode(MODE_RESET));

    let c = ControlIn { control_type: ControlType::Vendor, recipient: Recipient::Interface, request: BREQ_BT_CONST, value: 0, index: 0, length: 40 };
    let btc = intf
        .control_in(c, TIMEOUT)
        .wait()
        .ok()
        .and_then(|b| BtConst::parse(&b))
        .ok_or("cannot read bit-timing constants from the CANable")?;
    let bt = calc_bittiming(bitrate, &btc)?;
    ctrl_out(&intf, BREQ_BITTIMING, &bt.to_bytes()).map_err(|e| format!("cannot set bitrate: {e}"))?;

    let desc = intf.descriptor().ok_or("CANable has no interface descriptor")?;
    let ep = |dir| {
        desc.endpoints()
            .find(|e| e.transfer_type() == TransferType::Bulk && e.direction() == dir)
            .map(|e| e.address())
            .ok_or("CANable bulk endpoints not found")
    };
    let (addr_in, addr_out) = (ep(Direction::In)?, ep(Direction::Out)?);
    let ep_in = intf.endpoint::<Bulk, In>(addr_in).map_err(|e| e.to_string())?;
    let ep_out = intf.endpoint::<Bulk, Out>(addr_out).map_err(|e| e.to_string())?;

    ctrl_out(&intf, BREQ_MODE, &mode(MODE_START)).map_err(|e| format!("cannot start CANable: {e}"))?;
    // Binding the port is the "ready" signal canup waits for, so it comes last.
    let listener = TcpListener::bind(ADDR).map_err(|e| format!("cannot open local port {ADDR}: {e}"))?;
    Ok(Daemon { intf, ep_in, ep_out, listener })
}

impl Daemon {
    /// Run until candown or USB unplug. Must never print: canup's stderr pipe is gone by now.
    pub fn serve(self) -> ! {
        let shared = Arc::new(Shared {
            intf: self.intf,
            ep_out: Mutex::new(self.ep_out),
            clients: Mutex::new(Vec::new()),
            slots: Mutex::new([None; MAX_TX]),
        });

        let s = shared.clone();
        let listener = self.listener;
        thread::spawn(move || {
            for (id, conn) in listener.incoming().flatten().enumerate() {
                client(s.clone(), id as u64, conn);
            }
        });

        let mut ep = self.ep_in;
        let mps = ep.max_packet_size();
        for _ in 0..8 {
            ep.submit(Buffer::new(mps));
        }
        let mut errors = 0;
        loop {
            let Some(c) = ep.wait_next_complete(TIMEOUT) else { continue };
            match c.status {
                Err(TransferError::Disconnected) => std::process::exit(1), // unplugged: can0 goes away
                Err(_) => {
                    errors += 1;
                    if errors > 100 {
                        std::process::exit(1);
                    }
                }
                Ok(()) => {
                    errors = 0;
                    if let Some((echo_id, frame)) = decode_frame(&c.buffer[..c.actual_len]) {
                        on_usb_frame(&shared, echo_id, frame);
                    }
                }
            }
            let mut b = c.buffer;
            b.clear();
            b.set_requested_len(mps);
            ep.submit(b);
        }
    }
}

fn on_usb_frame(shared: &Shared, echo_id: u32, frame: CanFrame) {
    let rec = Record { kind: K_FRAME, frame, ts_us: now_us(), ..Default::default() };
    if echo_id == ECHO_ID_RX {
        if frame.id & CAN_ERR_FLAG == 0 {
            broadcast(shared, None, rec);
        }
    } else if let Some(owner) = shared.slots.lock().unwrap().get_mut(echo_id as usize).and_then(Option::take) {
        // Own frame made it onto the bus: everyone but the sender sees it (Linux recv_own_msgs=0).
        broadcast(shared, Some(owner), rec);
    }
}

fn broadcast(shared: &Shared, except: Option<u64>, rec: Record) {
    shared.clients.lock().unwrap().retain(|(id, tx)| {
        Some(*id) == except || !matches!(tx.try_send(rec), Err(TrySendError::Disconnected(_)))
    });
}

fn client(shared: Arc<Shared>, id: u64, mut conn: TcpStream) {
    conn.set_nodelay(true).ok();
    let Ok(mut w) = conn.try_clone() else { return };
    let (tx, rx) = sync_channel::<Record>(CLIENT_QUEUE);
    thread::spawn(move || {
        for r in rx {
            if r.write_to(&mut w).is_err() {
                break;
            }
        }
    });
    shared.clients.lock().unwrap().push((id, tx.clone()));

    thread::spawn(move || {
        while let Ok(r) = Record::read_from(&mut conn) {
            match r.kind {
                K_SEND => {
                    let code = send(&shared, id, &r.frame);
                    let kind = if code == 0 { K_OK } else { K_ERR };
                    let _ = tx.send(Record { kind, code, ..Default::default() });
                }
                K_SHUTDOWN => {
                    let _ = ctrl_out(&shared.intf, BREQ_MODE, &mode(MODE_RESET));
                    std::process::exit(0);
                }
                _ => {}
            }
        }
        shared.clients.lock().unwrap().retain(|(i, _)| *i != id);
    });
}

/// Returns 0 or an E_* code. Like Linux, success means "queued to the adapter", not "ACKed".
fn send(shared: &Shared, id: u64, frame: &CanFrame) -> u8 {
    let slot = {
        let mut slots = shared.slots.lock().unwrap();
        let Some(i) = slots.iter().position(Option::is_none) else { return E_NOBUFS };
        slots[i] = Some(id);
        i
    };
    let buf: Buffer = encode_frame(slot as u32, frame).to_vec().into();
    let c = shared.ep_out.lock().unwrap().transfer_blocking(buf, TIMEOUT);
    if c.status.is_err() {
        shared.slots.lock().unwrap()[slot] = None;
        return E_IO;
    }
    0
}
