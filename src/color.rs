use std::ffi::c_void;
use std::process::Command;
use windows_sys::Win32::Graphics::Gdi::{GetDC, ReleaseDC, HDC};
use windows_sys::Win32::UI::ColorSystem::SetDeviceGammaRamp;
use winreg::enums::*;
use winreg::RegKey;

#[repr(C)]
pub struct GammaRamp {
    pub red: [u16; 256],
    pub green: [u16; 256],
    pub blue: [u16; 256],
}

#[derive(Clone, Copy, Debug)]
pub struct ColorConfig {
    pub gamma: f32,
    pub brightness: f32,
    pub contrast: f32,
    pub r_gain: f32,
    pub g_gain: f32,
    pub b_gain: f32,
}

impl Default for ColorConfig {
    fn default() -> Self {
        Self {
            gamma: 1.0,
            brightness: 0.0,
            contrast: 1.0,
            r_gain: 1.0,
            g_gain: 1.0,
            b_gain: 1.0,
        }
    }
}

pub fn disable_windows_color_conflicts() {
    let _ = Command::new("schtasks")
        .args([
            "/Change",
            "/TN",
            r"\Microsoft\Windows\WindowsColorSystem\Calibration Loader",
            "/DISABLE",
        ])
        .output();
}

pub fn read_intel_cui_color_from_registry() -> Option<ColorConfig> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let cui = hkcu.open_subkey(r"Software\Intel\Display\igfxcui\Color").ok()?;
    let gamma_raw: u32 = cui.get_value("Gamma").unwrap_or(100);
    let bright_raw: i32 = cui.get_value("Brightness").unwrap_or(0);
    let contrast_raw: u32 = cui.get_value("Contrast").unwrap_or(100);

    Some(ColorConfig {
        gamma: (gamma_raw as f32 / 100.0).clamp(0.5, 2.5),
        brightness: (bright_raw as f32 / 200.0).clamp(-0.4, 0.4),
        contrast: (contrast_raw as f32 / 100.0).clamp(0.5, 1.8),
        ..Default::default()
    })
}

pub fn build_gamma_ramp(cfg: &ColorConfig) -> GammaRamp {
    let mut ramp = GammaRamp {
        red: [0; 256],
        green: [0; 256],
        blue: [0; 256],
    };

    let inv_gamma = 1.0 / cfg.gamma.max(0.1);
    for i in 0..256 {
        let norm = i as f32 / 255.0;
        let base = ((norm.powf(inv_gamma) - 0.5) * cfg.contrast + 0.5 + cfg.brightness)
            .clamp(0.0, 1.0);

        let r = (base * cfg.r_gain * 65535.0).clamp(0.0, 65535.0) as u16;
        let g = (base * cfg.g_gain * 65535.0).clamp(0.0, 65535.0) as u16;
        let b = (base * cfg.b_gain * 65535.0).clamp(0.0, 65535.0) as u16;

        ramp.red[i] = r;
        ramp.green[i] = g;
        ramp.blue[i] = b;
    }
    ramp
}

pub fn apply_gamma_ramp(cfg: &ColorConfig) -> bool {
    let ramp = build_gamma_ramp(cfg);
    unsafe {
        let hdc: HDC = GetDC(std::ptr::null_mut());
        if hdc.is_null() {
            return false;
        }
        let ok = SetDeviceGammaRamp(hdc, &ramp as *const _ as *const c_void) != 0;
        ReleaseDC(std::ptr::null_mut(), hdc);
        ok
    }
}
