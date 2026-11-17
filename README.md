# kbridge

Interactive menu-driven Rust driver patcher, INF modifier, native catalog generator, and low-overhead background optimization daemon for Intel HD Graphics (15.22.58.2993) on Windows 10 and Windows 11.

## Quick Start (2 Steps)
1. **Run `kbridge.exe` -> Option `[1]`**: Point it to your extracted `win7vista_64_152258` folder. It locates `kit51428.inf` / `igdlh64.inf` inside the `Graphics` directory, strips bloatware while keeping core WDDM + `.vp`/`.cpa`/`.bin` microcode files, unlocks Windows 10/11 support, bakes in the 11 performance DWORDs, builds and signs `igdlh.cat` via `wintrust.dll` (No Test Mode required), stages the package in DriverStore, and force-binds it onto your Intel HD Graphics device via `DiInstallDriverW`.
2. **Run `kbridge.exe` -> Option `[3]`**: Applies the Windows 10/11 OS tweaks (MSI interrupts, Spectre/Meltdown mitigation bypass, Exclusive Fullscreen, DWM/SysMain cuts), disables Windows' conflicting Calibration Loader task, and registers the background Color LUT + 0.5 ms Timer + Standby RAM guard to run automatically at login.
