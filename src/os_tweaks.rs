use anyhow::Result;
use std::process::Command;
use winreg::enums::*;
use winreg::RegKey;

const GPU_CLASS_PATH: &str =
    r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}\0000";

pub fn apply_live_gpu_registry_flags(overdrive: bool) -> Result<()> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let (gpu_key, _) = hklm.create_subkey(GPU_CLASS_PATH)?;

    let (dyn_clock, rc6, pwr_gate) = if overdrive { (1u32, 0u32, 0u32) } else { (0u32, 1u32, 1u32) };

    gpu_key.set_value("DisableDynamicClocking", &dyn_clock)?;
    gpu_key.set_value("ForceRenderPState", &0u32)?;
    gpu_key.set_value("RC6Enable", &rc6)?;
    gpu_key.set_value("PowerGatingEnabled", &pwr_gate)?;
    gpu_key.set_value("IncreaseFixedSegment", &1u32)?;
    gpu_key.set_value("ForceGTTCacheable", &1u32)?;
    gpu_key.set_value("DisableTiledMemory", &0u32)?;
    gpu_key.set_value("TotalStaticVRAM", &536870912u32)?;
    gpu_key.set_value("Disable_OverlayDSQualityEnhancement", &1u32)?;
    gpu_key.set_value("AdaptiveVsyncEnable", &0u32)?;
    gpu_key.set_value("AsyncFlips", &1u32)?;

    Ok(())
}

pub fn apply_win10_11_os_tweaks() -> Result<()> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    // 1. Disable Spectre & Meltdown syscall mitigations for 1st-Gen Core I/O recovery
    let (mm_key, _) = hklm.create_subkey(
        r"SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management",
    )?;
    mm_key.set_value("FeatureSettingsOverride", &3u32)?;
    mm_key.set_value("FeatureSettingsOverrideMask", &3u32)?;

    // 2. Disable DWM Multi-Plane Overlay (MPO) which causes frame stutter on WDDM 1.1 drivers in Win10/11
    let (dwm_key, _) = hklm.create_subkey(r"SOFTWARE\Microsoft\Windows\Dwm")?;
    dwm_key.set_value("OverlayTestMode", &5u32)?;

    // 3. Boost MMCSS Multimedia & GPU thread scheduling priority for video/games
    let (mmcss_games, _) = hklm.create_subkey(
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks\Games",
    )?;
    mmcss_games.set_value("GPU Priority", &8u32)?;
    mmcss_games.set_value("Priority", &6u32)?;
    mmcss_games.set_value("Scheduling Category", &"High")?;

    let (sys_prof, _) = hklm.create_subkey(
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile",
    )?;
    sys_prof.set_value("SystemResponsiveness", &0u32)?;
    sys_prof.set_value("NetworkThrottlingIndex", &0xFFFF_FFFFu32)?;

    // 4. Force Exclusive Fullscreen (FSE) & disable GameDVR background capture
    let (game_cfg, _) = hkcu.create_subkey(r"System\GameConfigStore")?;
    game_cfg.set_value("GameDVR_Enabled", &0u32)?;
    game_cfg.set_value("GameDVR_FSEBehaviorMode", &2u32)?;
    game_cfg.set_value("GameDVR_FSEBehavior", &2u32)?;

    // 5. Cut DWM Transparency overhead
    let (personalize, _) = hkcu.create_subkey(
        r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
    )?;
    personalize.set_value("EnableTransparency", &0u32)?;

    // 6. Set High Interrupt Priority on GPU, SATA AHCI, and USB controllers
    enable_msi_and_high_irq_priority(&hklm)?;

    // 7. Lock active Windows Power Plan: PCIe ASPM = Off, Minimum Processor State = 100%, USB Suspend = Off
    apply_anti_stutter_power_plan();

    // 8. Stop & disable SysMain (Superfetch) disk thrashing
    let _ = Command::new("sc").args(["stop", "SysMain"]).output();
    let _ = Command::new("sc").args(["config", "SysMain", "start=", "disabled"]).output();

    println!("[+] Applied anti-stutter OS, DWM OverlayTestMode, MMCSS GPU Priority, Power Plan, and IRQ tweaks.");
    Ok(())
}

fn apply_anti_stutter_power_plan() {
    let _ = Command::new("powercfg")
        .args([
            "/SETACVALUEINDEX",
            "SCHEME_CURRENT",
            "5012eeed-3c32-40de-8c5e-835a2d65777b",
            "ee12f906-d277-404b-b6da-e5fa1a576df5",
            "0",
        ])
        .output();

    let _ = Command::new("powercfg")
        .args([
            "/SETACVALUEINDEX",
            "SCHEME_CURRENT",
            "54533251-82be-4824-96c1-47b60b740d00",
            "893dee8e-2bef-41e0-89c6-b55d0929964c",
            "100",
        ])
        .output();

    let _ = Command::new("powercfg")
        .args([
            "/SETACVALUEINDEX",
            "SCHEME_CURRENT",
            "2a737441-1930-4402-8d77-b2bebba308a3",
            "48e6b7a6-50f5-4782-a5d4-53bb8f07e226",
            "0",
        ])
        .output();

    let _ = Command::new("powercfg").args(["/SETACTIVE", "SCHEME_CURRENT"]).output();
}

fn enable_msi_and_high_irq_priority(hklm: &RegKey) -> Result<()> {
    let pci_enum = hklm.open_subkey(r"SYSTEM\CurrentControlSet\Enum\PCI")?;
    for dev_id in pci_enum.enum_keys().flatten() {
        let dev_low = dev_id.to_lowercase();
        let is_intel_gpu = dev_low.starts_with("ven_8086&dev_004");

        if let Ok(dev_key) = pci_enum.open_subkey(&dev_id) {
            for inst_id in dev_key.enum_keys().flatten() {
                if let Ok(inst_key) = dev_key.open_subkey(&inst_id) {
                    let service: String = inst_key.get_value("Service").unwrap_or_default();
                    let svc_low = service.to_lowercase();
                    let is_io_ctrl = svc_low.contains("storahci")
                        || svc_low.contains("iastor")
                        || svc_low.contains("usb");

                    if is_io_ctrl || is_intel_gpu {
                        if is_io_ctrl {
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
