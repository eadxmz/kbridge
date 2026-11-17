# kbridge

Interactive menu-driven Rust driver patcher, INF modifier, native catalog generator, and anti-stutter background optimization daemon for Intel HD Graphics (15.22.58.2993) on Windows 10 and Windows 11.

## Anti-Stutter Architecture (`v0.4.0`)
- **Zero-Polling WinEvent Foreground Hook**: Uses `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` so priority and core-affinity boosts trigger strictly on window focus changes instead of waking up on a timer.
- **Conditional Gamma LUT Guard**: Reads the current display ramp via `GetDeviceGammaRamp` and only calls `SetDeviceGammaRamp` if Windows actually overwrote the LUT—eliminating the 3-second GDI pipeline micro-stutter during video playback.
- **Standby RAM Purge Cooldown**: Enforces a 60-second cooldown and lower 350 MB threshold on `NtSetSystemInformation(MemoryPurgeStandbyList)` so cached video/browser buffers aren't repeatedly evicted.
- **WDDM 1.1 & Power-State Anti-Stutter (`Option [3]`)**: Disables PCIe ASPM (`5012eeed-3c32-40de-8c5e-835a2d65777b`), locks Minimum Processor State to `100%` (`893dee8e-2bef-41e0-89c6-b55d0929964c`), disables DWM MPO overlay glitches on WDDM 1.1 (`OverlayTestMode = 5`), sets MMCSS GPU Priority to `8`, and pins GPU IRQ policy to High Priority.
