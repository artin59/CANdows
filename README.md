# CANdows

`candump` / `cansend` for Windows 10/11 with a CANable (candleLight firmware). Same commands and output as Linux can-utils, no WSL, no drivers, nothing to install.

## Use

Unzip `canup.exe`, `candown.exe`, `candump.exe`, `cansend.exe` into a folder and open a terminal there (or add the folder to PATH).

| Linux | Windows |
|---|---|
| `sudo ip link set can0 up type can bitrate 500000` | `canup can0 500000` |
| `sudo ip link set can0 down` | `candown can0` |
| `candump can0` | `candump can0` |
| `cansend can0 123#DEADBEEF` | `cansend can0 123#DEADBEEF` |

Run `candump` and `cansend` in as many terminals as you like at the same time. `canup` starts a hidden background process that owns the CANable. It keeps running after you close the terminal, until `candown` or until you unplug the adapter.

Supported:
- `candump`:
  - `-t a|d|z` timestamps
  - `-L` log format
  - `-l` writes `candump-<date>.log`
- `cansend`:
  - 11-bit `123#..` and 29-bit `12345678#..` IDs
  - `.` separators
  - RTR `123#R` / `123#R4`

Not supported: classic CAN only (no FD), one adapter (`can0`), and no candump filters or error frames.

### Troubleshooting
- **"No CANable found"**: check that it's plugged in. In Device Manager it should appear under *Universal Serial Bus devices* as a candleLight / gs_usb device.
- **"cannot claim CANable"**: another program (Cangaroo, a python-can script, another `canup`) has it open. Close it.
- **CANable found by other tools but not here**: someone may have used Zadig to give it a different driver. Use Zadig to put it back on **WinUSB**.
- **`cansend` says "No buffer space available"**: 10 frames are waiting for an ACK. Nothing else on the bus is ACKing: check wiring, termination and bitrate. Same as Linux.
- **candump exits with "read: Network is down"**: the adapter was unplugged or `candown` was run. Run `canup` again.

## Build (only whoever makes the release needs this)

On Windows, install Rust from https://rustup.rs (it also asks you to install the Visual Studio C++ build tools), then:

```
cargo build --release
```

The four `.exe` files are in `target\release\`. Zip them for members. `.cargo/config.toml` links the C runtime statically, so they run on any 64-bit Windows 10/11 PC.

Run tests with `cargo test` on any OS.
