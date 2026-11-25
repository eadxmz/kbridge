# kbridge v0.6.0 — Ultimate Driver Patcher + HD 4400 Dual-GPU `ZeroLag-X` Frame Gen

All-in-one Rust driver patcher, native WinTrust catalog signer, invisible anti-stutter daemon, and **custom Dual-GPU Offload 2x Frame Generation engine** for Intel HD Graphics (`Ironlake 15.22.58.2993` & `Haswell HD 4400 Family 15.36/15.40`).

## Custom Frame Gen Engine (`Option [9]` — Intel HD 4400 Only)
Designed specifically for **Dual-GPU Offload** (your discrete GPU renders the game at 100% speed while the **Intel HD 4400** runs zero-copy DXGI Desktop Duplication + HLSL Optical Flow + overlay presentation with **0% load on the dGPU**):

1. **Pyramid Optical Flow (`2x`)**: Two-level coarse-to-fine (`1/8` → `1/4` res) block matching + 3×3 vector smoothing + disocclusion masking + static HUD lock.
2. **Ultra-Lite Lucas-Kanade (`2x`)**: Single-pass `PS 4.0` gradient optical flow at `1/4` res (~50% lower shader cost).
3. **Adaptive Video Smoother (`24/30 → 60 fps`)**: Scene-cut detection + temporal smoothing tuned for anime, movies, and YouTube.

### Custom Ultra-Low-Latency Technique: `ZeroLag-X`
- **0-Frame Hold Predictive Extrapolation**: Displays `Frame[N]` the exact microsecond DXGI delivers it (eliminating the ~33 ms 1-frame hold of standard interpolation), then warps forward (`t = +0.5`) with disocclusion inpainting and static-HUD protection.
- **Live Mouse-Delta Camera-Vector Injection**: Samples real-time mouse delta (`dx, dy`) between `Frame[N]` capture and synthetic midpoint presentation to steer camera-pan extrapolation before the dGPU finishes rendering `Frame[N+1]`.
- **1-Frame Waitable Flip Queue (`IDXGISwapChain2::SetMaximumFrameLatency(1)`) + 0.5 ms Kernel Timer JIT Pacing + `WDA_EXCLUDEFROMCAPTURE`**.
- **In-Game Hotkeys**: `Ctrl + Alt + F` toggles the overlay on/off; `Ctrl + Alt + Q` exits Frame Gen.
