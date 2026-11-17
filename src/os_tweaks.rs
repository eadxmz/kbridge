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

    let (mm_key, _) = hklm.create_subkey(
        r"SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management",
    )?;
    mm_key.set_value("FeatureSettingsOverride", &3u32)?;
    mm_key.set_value("FeatureSettingsOverrideMask", &3u32)?;

    let (game_cfg, _) = hkcu.create_subkey(r"System\GameConfigStore")?;
    game_cfg.set_value("GameDVR_Enabled", &0u32)?;
    game_cfg.set_value("GameDVR_FSEBehaviorMode", &2u32)?;
    game_cfg.set_value("GameDVR_FSEBehavior", &2u32)?;

    let (personalize, _) = hkcu.create_subkey(
        r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
    )?;
    personalize.set_value("EnableTransparency", &0u32)?;

    enable_msi_on_storage_and_usb(&hklm)?;

    let _ = Command::new("sc").args(["stop", "SysMain"]).output();
    let _ = Command::new("sc").args(["config", "SysMain", "start=", "disabled"]).output();

    println!("[+] Applied Windows 10/11 OS, Mitigation, FSE, DWM, and MSI tweaks.");
    Ok(())
}

fn enable_msi_on_storage_and_usb(hklm: &RegKey) -> Result<()> {
    let pci_enum = hklm.open_subkey(r"SYSTEM\CurrentControlSet\Enum\PCI")?;
    for dev_id in pci_enum.enum_keys().flatten() {
        if let Ok(dev_key) = pci_enum.open_subkey(&dev_id) {
            for inst_id in dev_key.enum_keys().flatten() {
                if let Ok(inst_key) = dev_key.open_subkey(&inst_id) {
                    let service: String = inst_key.get_value("Service").unwrap_or_default();
                    let svc_low = service.to_lowercase();
                    if svc_low.contains("storahci")
                        || svc_low.contains("iastor")
                        || svc_low.contains("usb")
                    {
                        let msi_path = format!(
                            r"SYSTEM\CurrentControlSet\Enum\PCI\{}\{}\Device Parameters\Interrupt Management\MessageSignaledInterruptProperties",
                            dev_id, inst_id
                        );
                        let prio_path = format!(
                            r"SYSTEM\CurrentControlSet\Enum\PCI\{}\{}\Device Parameters\Interrupt Management\Affinity Policy",
                            dev_id, inst_id
                        );
                        if let Ok((msi_key, _)) = hklm.create_subkey(&msi_path) {
                            let _ = msi_key.set_value("MSISupported", &1u32);
                        }
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
