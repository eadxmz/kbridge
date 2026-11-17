# kbridge

Interactive menu-driven Rust driver patcher, INF modifier, native catalog generator, and low-overhead background optimization daemon for Intel HD Graphics (15.22.58.2993) on Windows 10 and Windows 11.

## Quick Start (2 Steps)
1. **Run `kbridge.exe` -> Option `[1]`**: Point it to your extracted `win7vista_64_152258` folder. It automatically detects `kit51681.inf` / `igdlh64.inf`, strips bloatware, unlocks Windows 10/11 support, bakes in the 11 performance DWORDs, signs a fresh catalog in Normal Mode (no Test Mode required), and installs the driver.
2. **Run `kbridge.exe` -> Option `[3]`**: Applies the Windows 10/11 OS tweaks (MSI interrupts, Spectre/Meltdown mitigation bypass, Exclusive Fullscreen, DWM/SysMain cuts), disables Windows' conflicting Calibration Loader task, and registers the background Color LUT + 0.5 ms Timer + Standby RAM guard to run automatically at login.
