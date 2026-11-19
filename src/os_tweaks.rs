use anyhow::Result;
use std::process::Command;
use winreg::enums::*;
use winreg::RegKey;

const DISPLAY_CLASS_ROOT: &str =
    r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";

/// Dynamically finds the exact Display Adapter subkey (`0000`, `0001`, etc.) belonging to Intel (`VEN_8086`)
/// and detects whether it is Haswell (HD 4400 family) or Ironlake (1st-Gen).
pub fn detect_active_intel_gpu_class_key() -> (String, bool) {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    if let Ok(class_root) = hklm.open_subkey(DISPLAY_CLASS_ROOT) {
        for sub in ["0000", "0001", "0002", "0003"] {
            if let Ok(k) = class_root.open_subkey(sub) {
                let match_id: String = k.get_value("MatchingDeviceId").unwrap_or_default();
                let desc: String = k.get_value("DriverDesc").unwrap_or_default();
                let low_id = match_id.to_lowercase();
                let low_desc = desc.to_lowercase();

                if low_id.contains("ven_8086") || low_desc.contains("intel") {
                    let is_haswell = low_id.contains("dev_04")
                        || low_id.contains("dev_0a")
                        || low_id.contains("dev_0d")
                        || low_desc.contains("4400")
                        || low_desc.contains("4600")
                        || low_desc.contains("5000")
                        || low_desc.contains("4200");
                    return (format!(r"{}\{}", DISPLAY_CLASS_ROOT, sub), is_haswell);
                }
            }
        }
    }
    (format!(r"{}\0000", DISPLAY_CLASS_ROOT), false)
}

pub fn apply_live_gpu_registry_flags(overdrive: bool) -> Result<()> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let (gpu_path, is_haswell) = detect_active_intel_gpu_class_key();
    let (gpu_key, _) = hklm.create_subkey(&gpu_path)?;

    let (dyn_clock, rc6, pwr_gate, mid_pwr, policy) = if overdrive {
        (1u32, 0u32, 0u32, 0u32, 0u32)
    } else {
        (0u32, 1u32, 1u32, 1u32, 1u32)
    };

    // Clocks, Render P-States, DPST & PSR
    gpu_key.set_value("DisableDynamicClocking", &dyn_clock)?;
    gpu_key.set_value("ForceRenderPState", &0u32)?;
    gpu_key.set_value("RC6Enable", &rc6)?;
    gpu_key.set_value("PowerGatingEnabled", &pwr_gate)?;
    gpu_key.set_value("EnableMidGfxPowerSaving", &mid_pwr)?;
    gpu_key.set_value("ACUserPreferencePolicy", &policy)?;
    gpu_key.set_value("DCUserPreferencePolicy", &policy)?;
    gpu_key.set_value("TurboBoostEnable", &1u32)?;
    gpu_key.set_value("DisplayPowerSavingTechnology", &0u32)?;
    gpu_key.set_value("Disable_PSR", &1u32)?;
    gpu_key.set_value("PsrDisable", &1u32)?;
    gpu_key.set_value("FeatureTestControl", &0x0000_9240u32)?;

    // Memory, GTT & VRAM
    gpu_key.set_value("IncreaseFixedSegment", &1u32)?;
    gpu_key.set_value("ForceGTTCacheable", &1u32)?;
    gpu_key.set_value("DisableTiledMemory", &0u32)?;
    gpu_key.set_value("EnableDynamicMemoryAllocation", &1u32)?;
    gpu_key.set_value("GttSize", &512u32)?;
    let static_vram: u32 = if is_haswell { 1_073_741_824 } else { 536_870_912 };
    gpu_key.set_value("TotalStaticVRAM", &static_vram)?;

    // 1440p60 Media & Low-Latency Flip Pipeline
    gpu_key.set_value("Disable_OverlayDSQualityEnhancement", &1u32)?;
    gpu_key.set_value("DisableVideoPostProcessing", &1u32)?;
    gpu_key.set_value("SmoothVideo", &0u32)?;
    gpu_key.set_value("TotalColorCorrection", &0u32)?;
    gpu_key.set_value("MediaDecodePriority", &1u32)?;
    gpu_key.set_value("ForceDXVA2", &1u32)?;
    gpu_key.set_value("AdaptiveVsyncEnable", &0u32)?;
    gpu_key.set_value("AsyncFlips", &1u32)?;
    gpu_key.set_value("FlipQueueSize", &1u32)?;
    gpu_key.set_value("D3D_FlipInterval", &0u32)?;

    if is_haswell {
        gpu_key.set_value("EnableVP9HybridDecode", &1u32)?;
        gpu_key.set_value("ShaderCache", &1u32)?;
        gpu_key.set_value("EnableHardwareScheduling", &1u32)?;
    }

    // Unlock 512 MB Dedicated GMM Segment for Intel integrated graphics
    if let Ok((gmm_key, _)) = hklm.create_subkey(r"SOFTWARE\Intel\GMM") {
        let _ = gmm_key.set_value("DedicatedSegmentSize", &512u32);
    }

    println!(
        "[+] Wrote 25+ GPU/Media DWORDs to {} and unlocked 512 MB Intel GMM.",
        gpu_path
    );
    Ok(())
}

pub fn apply_win10_11_os_tweaks() -> Result<()> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    // 1. Spectre & Meltdown bypass + Lock kernel drivers (igdkmd64.sys) in physical RAM
    let (mm_key, _) = hklm.create_subkey(
        r"SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management",
    )?;
    mm_key.set_value("FeatureSettingsOverride", &3u32)?;
    mm_key.set_value("FeatureSettingsOverrideMask", &3u32)?;
    mm_key.set_value("DisablePagingExecutive", &1u32)?;
    mm_key.set_value("LargeSystemCache", &0u32)?;

    // 2. Low-latency foreground quantum scheduling (Short, Variable, High Foreground Boost)
    let (prio_key, _) = hklm.create_subkey(r"SYSTEM\CurrentControlSet\Control\PriorityControl")?;
    prio_key.set_value("Win32PrioritySeparation", &0x26u32)?;

    // 3. DXGKrnl TDR safety margin (prevents false GPU resets under heavy 1440p60/game spikes)
    let (gfx_drv, _) = hklm.create_subkey(r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers")?;
    gfx_drv.set_value("TdrDelay", &10u32)?;
    gfx_drv.set_value("TdrDdiDelay", &10u32)?;

    // 4. Disable DWM Multi-Plane Overlay (MPO) glitches on Win10/11
    let (dwm_key, _) = hklm.create_subkey(r"SOFTWARE\Microsoft\Windows\Dwm")?;
    dwm_key.set_value("OverlayTestMode", &5u32)?;
    dwm_key.set_value("OverlayMinFPS", &0u32)?;

    // 5. MMCSS Maximum GPU Priority for Video Playback, Display Post-Processing & Games
    let (sys_prof, _) = hklm.create_subkey(
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile",
    )?;
    sys_prof.set_value("SystemResponsiveness", &0u32)?;
    sys_prof.set_value("NetworkThrottlingIndex", &0xFFFF_FFFFu32)?;

    for task in ["Games", "Playback", "DisplayPostProcessing", "Pro Audio"] {
        let path = format!(
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks\{}",
            task
        );
        if let Ok((tkey, _)) = hklm.create_subkey(&path) {
            let _ = tkey.set_value("GPU Priority", &8u32);
            let _ = tkey.set_value("Priority", &6u32);
            let _ = tkey.set_value("Scheduling Category", &"High");
            let _ = tkey.set_value("SFIO Priority", &"High");
            let _ = tkey.set_value("Latency Sensitive", &"True");
            let _ = tkey.set_value("Background Only", &"False");
            let _ = tkey.set_value("Clock Rate", &10000u32);
        }
    }

    // 6. Force Exclusive Fullscreen (FSE) & disable GameDVR capture overhead
    let (game_cfg, _) = hkcu.create_subkey(r"System\GameConfigStore")?;
    game_cfg.set_value("GameDVR_Enabled", &0u32)?;
    game_cfg.set_value("GameDVR_FSEBehaviorMode", &2u32)?;
    game_cfg.set_value("GameDVR_FSEBehavior", &2u32)?;

    // 7. Cut DWM Transparency & UI animation overhead
    let (personalize, _) = hkcu.create_subkey(
        r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
    )?;
    personalize.set_value("EnableTransparency", &0u32)?;

    // 8. Enable MSI Mode & High IRQ Priority across GPU, Storage, USB & Audio
    enable_msi_and_high_irq_priority(&hklm)?;

    // 9. Power Plan: Unpark 100% cores, Aggressive Turbo, PCIe ASPM Off, Intel Graphics Max Perf
    apply_ultimate_power_plan();

    // 10. Kernel Timer Coalescing: Disable Dynamic Tick & ensure fast TSC (no slow HPET)
    let _ = Command::new("bcdedit").args(["/set", "disabledynamictick", "yes"]).output();
    let _ = Command::new("bcdedit").args(["/deletevalue", "useplatformclock"]).output();

    // 11. Stop & disable SysMain (Superfetch) disk thrashing
    let _ = Command::new("sc").args(["stop", "SysMain"]).output();
    let _ = Command::new("sc").args(["config", "SysMain", "start=", "disabled"]).output();

    println!("[+] Applied ALL Max OS, Kernel, MMCSS Playback/Games, Power Plan, Core-Unpark & IRQ tweaks.");
    Ok(())
}

fn set_power_val(sub: &str, setting: &str, val: &str) {
    let _ = Command::new("powercfg")
        .args(["/SETACVALUEINDEX", "SCHEME_CURRENT", sub, setting, val])
        .output();
    let _ = Command::new("powercfg")
        .args(["/SETDCVALUEINDEX", "SCHEME_CURRENT", sub, setting, val])
        .output();
}

fn apply_ultimate_power_plan() {
    // PCI Express ASPM -> Off (0)
    set_power_val(
        "5012eeed-3c32-40de-8c5e-835a2d65777b",
        "ee12f906-d277-404b-b6da-e5fa1a576df5",
        "0",
    );
    // Processor Min & Max State -> 100%
    set_power_val(
        "54533251-82be-4824-96c1-47b60b740d00",
        "893dee8e-2bef-41e0-89c6-b55d0929964c",
        "100",
    );
    set_power_val(
        "54533251-82be-4824-96c1-47b60b740d00",
        "bc5038f7-23e0-4960-96da-33abaf5935ec",
        "100",
    );
    // Unpark 100% of CPU Cores (CPMINCORES = 100)
    set_power_val(
        "54533251-82be-4824-96c1-47b60b740d00",
        "0cc5b647-c1df-4637-891a-dec35c318583",
        "100",
    );
    // Processor Performance Boost Mode -> Aggressive (2)
    set_power_val(
        "54533251-82be-4824-96c1-47b60b740d00",
        "be337238-0d82-4146-a960-4f3749d470c7",
        "2",
    );
    // Energy Performance Preference -> 0 (Max Performance)
    set_power_val(
        "54533251-82be-4824-96c1-47b60b740d00",
        "36687f9e-e3a5-4dbf-b1dc-15eb381c50ef",
        "0",
    );
    // Intel(R) Graphics Power Plan -> Maximum Performance (2)
    set_power_val(
        "44f3beca-a7c0-460e-9df2-bb8b99e0cba6",
        "3619c3f2-afb2-4afc-b0e9-e7fef372de36",
        "2",
    );
    // USB Selective Suspend -> Disabled (0)
    set_power_val(
        "2a737441-1930-4402-8d77-b2bebba308a3",
        "48e6b7a6-50f5-4782-a5d4-53bb8f07e226",
        "0",
    );
    let _ = Command::new("powercfg").args(["/SETACTIVE", "SCHEME_CURRENT"]).output();
}

fn enable_msi_and_high_irq_priority(hklm: &RegKey) -> Result<()> {
    let pci_enum = hklm.open_subkey(r"SYSTEM\CurrentControlSet\Enum\PCI")?;
    for dev_id in pci_enum.enum_keys().flatten() {
        let dev_low = dev_id.to_lowercase();
        let is_ironlake_gpu = dev_low.starts_with("ven_8086&dev_004");
        let is_haswell_gpu = dev_low.starts_with("ven_8086&dev_04")
            || dev_low.starts_with("ven_8086&dev_0a")
            || dev_low.starts_with("ven_8086&dev_0d");

        if let Ok(dev_key) = pci_enum.open_subkey(&dev_id) {
            for inst_id in dev_key.enum_keys().flatten() {
                if let Ok(inst_key) = dev_key.open_subkey(&inst_id) {
                    let service: String = inst_key.get_value("Service").unwrap_or_default();
                    let svc_low = service.to_lowercase();
                    let is_io_ctrl = svc_low.contains("storahci")
                        || svc_low.contains("stornvme")
                        || svc_low.contains("iastor")
                        || svc_low.contains("usb")
                        || svc_low.contains("hdaudbus");

                    if is_io_ctrl || is_ironlake_gpu || is_haswell_gpu {
                        // Enable MSI on Haswell GPU + Storage/USB/Audio (keep Ironlake on line-based IRQ)
                        if is_io_ctrl || is_haswell_gpu {
                            let msi_path = format!(
                                r"SYSTEM\CurrentControlSet\Enum\PCI\{}\{}\Device Parameters\Interrupt Management\MessageSignaledInterruptProperties",
                                dev_id, inst_id
                            );
                            if let Ok((msi_key, _)) = hklm.create_subkey(&msi_path) {
                                let _ = msi_key.set_value("MSISupported", &1u32);
                            }
                        }

                        let prio_path = format!(
                            r"SYSTEM\CurrentControlSet\Enum\PCI\{}\{}\Device Parameters\Interrupt Management\Affinity Policy",
                            dev_id, inst_id
                        );
                        if let Ok((prio_key, _)) = hklm.create_subkey(&prio_path) {
                            let _ = prio_key.set_value("DevicePriority", &3u32);
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
