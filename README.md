# kbridge v0.5.0

Ultimate multi-generation Rust driver patcher, INF modifier, native WinTrust catalog signer, and invisible anti-stutter daemon for **1st-Gen Intel HD Graphics (Ironlake `15.22.58.2993`)** and **4th-Gen Intel HD Graphics 4400 Family (Haswell `15.36` / `15.40`)** on Windows 10 and Windows 11.

## Quick Start
1. **Option [1]**: Point to your extracted Ironlake or Haswell HD 4400 driver folder. Auto-detects generation, preserves all WDDM + DXVA2/QuickSync media DLLs, strips telemetry/bloatware, bakes 25+ performance/media DWORDs, builds and signs `igdlh.cat` in Normal Mode (No Test Mode), and binds onto your Intel GPU.
2. **Option [3]**: Applies maximum OS, DXGKrnl, MMCSS Playback/Games, 512MB GMM, core-unparking, dynamic-tick, and IRQ tweaks, and launches the invisible Priority-0 startup daemon.
3. **Option [7]** *(Optional)*: Tunes your Firefox profile (`user.js`) for smooth hardware-accelerated 1440p60 playback.
