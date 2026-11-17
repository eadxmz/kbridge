# kbridge

Low-overhead Rust driver patcher, INF modifier, and background optimization daemon for Intel HD Graphics (15.22.58.2993) on Windows 10 and Windows 11.

## Features
- **Driver Package Stripping & Binary Patching**: Strips telemetry/bloatware, patches `igdkmd64.sys` OS version checks and `igdumd64.dll` swap-chain delays, and recalculates PE header checksums.
- **Windows 10/11 INF Unlock & Hardcoded Flags**: Clones device descriptors into `[IntelGfx.NTamd64.10.0]` and bakes 11 power, clock, and GTT memory DWORDs directly into `[igfx_SoftwareDeviceSettings]`.
- **Automated Self-Signing**: Generates a local Trusted Root certificate and signs `igdkmd64.sys` and `igdlh.cat`.
- **Persistent Color & Gamma Guard**: Fixes the Intel Media Control Panel boot/fullscreen color reset bug on Windows 10/11 without relying on `igfxpers.exe`.
- **Background Daemon**: Locks the Windows kernel timer to 0.5 ms, purges the Standby RAM list under 500 MB free memory, and pins foreground games to physical cores.

## Usage (Run as Administrator)

1. Build release binary:
   `cargo build --release`

2. Patch, sign, and install the original Intel driver:
   `.\target\release\kbridge.exe patch-driver --input "C:\win64_152258" --output ".\patched_driver" --keep-ui --install`

3. Apply Windows 10/11 OS, MSI mode, FSE, and color calibration fixes:
   `.\target\release\kbridge.exe os-tune`

4. Run the persistent color & performance daemon and register at logon:
   `.\target\release\kbridge.exe daemon --gamma 1.12 --contrast 1.08 --register-startup`
