# CANdows

Linux can-utils for Windows. `candump` and `cansend` work with CANable (candleLight firmware) adapters on Windows 10/11, using the same commands and output as SocketCAN. No WSL or driver install needed.

## Install (one time, about 10 minutes)

### 1. Install Rust
1. Download and run **rustup-init.exe** from https://rustup.rs.
2. If it says you need the Visual Studio C++ build tools, accept. In the Visual Studio Installer, tick **"Desktop development with C++"** and install, then continue the Rust setup with the default options.
3. Close and reopen your terminal so the `cargo` command is found.

### 2. Get the code
Either:
- **With git:** `git clone <this repo's URL>`, then `cd` into the new folder, or
- **Without git:** on this GitHub page click **Code → Download ZIP**, unzip it, and open a terminal in the unzipped folder.

### 3. Build and install
In that folder, run:

```
cargo install --path .
```

This builds `canup`, `candown`, `candump` and `cansend` and puts them on your PATH. They work from any terminal, in any folder.

To update later, get the new code again and rerun the same command.

To check that it installed, run `candump can0`. It should say `can0: interface is down, run: canup can0 <bitrate>`.

## Use

Plug in the CANable, then:

| Linux | Windows |
|---|---|
| `sudo ip link set can0 up type can bitrate 500000` | `canup can0 500000` |
| `sudo ip link set can0 down` | `candown can0` |
| `candump can0` | `candump can0` |
| `cansend can0 123#DEADBEEF` | `cansend can0 123#DEADBEEF` |

You can run `candump` and `cansend` in as many terminals as you like at the same time. `canup` starts a hidden background process that owns the CANable. It keeps running after you close the terminal, until you run `candown` or unplug the adapter.

Supported:
- `candump`:
  - `-t a|d|z` timestamps
  - `-L` log format
  - `-l` writes `candump-<date>.log`
- `cansend`:
  - 11-bit `123#..` and 29-bit `12345678#..` IDs
  - `.` separators
  - RTR `123#R` / `123#R4`

Not supported:
- CAN FD (classic CAN only)
- more than one adapter (only `can0`)
- candump filters and error frames

## Troubleshooting
- **`cargo` is not recognized**: reopen the terminal after installing Rust.
- **Build error mentioning `link.exe` or MSVC**: the C++ build tools are missing. Rerun the Visual Studio Installer and add "Desktop development with C++".
- **"No CANable found"**: check that it's plugged in. In Device Manager it should appear under *Universal Serial Bus devices*.
- **"cannot claim CANable"**: another program (Cangaroo, a python-can script) has the CANable open. Close it.
- **Other tools see the CANable but CANdows doesn't**: someone may have used Zadig to give it a different driver. Use Zadig to set it back to **WinUSB**.
- **`cansend` says "No buffer space available"**: 10 frames are waiting for an ACK because nothing on the bus is acknowledging them. Check wiring, termination and bitrate. Linux behaves the same way.
- **candump exits with "read: Network is down"**: the adapter was unplugged or `candown` was run. Run `canup` again.

## Development
Run `cargo test` (works on any OS). `.cargo/config.toml` links the C runtime statically, so a release build (`cargo build --release`, output in `target\release\`) can be copied to PCs without Rust.
