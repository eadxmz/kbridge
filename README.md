# kbridge

Interactive menu-driven Rust driver patcher, INF modifier, native catalog generator, and low-overhead background optimization daemon for Intel HD Graphics (15.22.58.2993) on Windows 10 and Windows 11.

## Features
- **Zero-CLI Interactive Menu**: Double-click `kbridge.exe` (automatically requests Administrator privileges) and pick numbered options `[1]` through `[8]`—no command-line flags to memorize.
- **No Test Mode Required (WHQL + CatRoot Dual-Catalog Signing)**: Preserves Intel's genuine Microsoft WHQL kernel signature on `igdkmd64.sys` by registering the original WHQL catalog in Windows `CatRoot`, while natively generating and Root-CA signing a new `igdlh.cat` via `wintrust.dll` for the modified `igdlh64.inf`. Installs and runs on Windows 10/11 in **Normal Boot** (no Test Mode watermark, no WDK/Inf2Cat required).
- **Driver Package Stripping**: Strips `igfxpers.exe`, `hkcmd.exe`, `igfxsrvc.exe`, OpenCL, and DisplayAudio bloatware.
- **Windows 10/11 INF Unlock & Hardcoded Flags**: Clones device descriptors into `[IntelGfx.NTamd64.10.0]` and bakes 11 power, clock, and GTT memory DWORDs directly into `[igfx_SoftwareDeviceSettings]`.
- **Persistent Color & Gamma Guard**: Fixes the Intel Media Control Panel boot/fullscreen color reset bug on Windows 10/11 without relying on `igfxpers.exe`.
- **Ultra-Lean Background Daemon**: Locks the Windows kernel timer to 0.5 ms, purges the Standby RAM list under 500 MB free memory, and pins foreground games to physical cores.
