# kbridge

Interactive menu-driven Rust driver patcher, INF modifier, native catalog generator, and invisible anti-stutter background daemon for Intel HD Graphics (15.22.58.2993) on Windows 10 and Windows 11.

## Key Features (`v0.4.1`)
- **Invisible & Instant Boot Daemon**: Detaches from the console via `FreeConsole()` and `CREATE_NO_WINDOW` (zero Command Prompt windows on screen) and registers a `<Priority>0</Priority>` Windows Logon task that executes immediately at boot before the desktop finishes loading.
- **Settings -> Display LUT Lock (`win32k` Cache-Buster)**: Toggles 1 LSB (`1/65535`) in the 16-bit GDI gamma ramp so `win32kfull.sys` never skips updating the hardware DAC when Windows 11 **Settings -> Display** resets the LUT via DXGI.
- **Anti-Stutter Frame Pacing**: Event-driven `SetWinEventHook`, 60-second Standby RAM purge cooldown, DWM `OverlayTestMode = 5`, MMCSS `GPU Priority = 8`, and `powercfg` PCIe ASPM off + 100% min CPU state.
