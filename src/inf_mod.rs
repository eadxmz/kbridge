use crate::patcher::read_inf_text;
use anyhow::Result;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

const HARDCODED_INF_DWORDS_COMMON: &[(&str, u32)] = &[
    // Clock, Render Boost & Power Gating
    ("DisableDynamicClocking", 1),
    ("ForceRenderPState", 0),
    ("RC6Enable", 0),
    ("PowerGatingEnabled", 0),
    ("EnableMidGfxPowerSaving", 0),
    ("ACUserPreferencePolicy", 0),
    ("DCUserPreferencePolicy", 0),
    ("TurboBoostEnable", 1),
    ("DisplayPowerSavingTechnology", 0),
    ("Disable_PSR", 1),
    ("PsrDisable", 1),
    ("FeatureTestControl", 0x0000_9240),
    // Memory, GTT & VRAM Allocation
    ("IncreaseFixedSegment", 1),
    ("ForceGTTCacheable", 1),
    ("DisableTiledMemory", 0),
    ("EnableDynamicMemoryAllocation", 1),
    ("GttSize", 512),
    // 1440p60 Media Pipeline & Low-Latency Render
    ("Disable_OverlayDSQualityEnhancement", 1),
    ("DisableVideoPostProcessing", 1),
    ("SmoothVideo", 0),
    ("TotalColorCorrection", 0),
    ("MediaDecodePriority", 1),
    ("ForceDXVA2", 1),
    ("AdaptiveVsyncEnable", 0),
    ("AsyncFlips", 1),
    ("FlipQueueSize", 1),
    ("D3D_FlipInterval", 0),
];

const HASWELL_EXTRA_DWORDS: &[(&str, u32)] = &[
    ("TotalStaticVRAM", 1_073_741_824), // 1 GB virtual dedicated report for Haswell HD 4400
    ("EnableVP9HybridDecode", 1),       // Hardware-assisted VP9 decode on Haswell 15.40
    ("ShaderCache", 1),                 // Eliminates D3D11/OpenGL shader compile stutter
    ("EnableHardwareScheduling", 1),
];

const IRONLAKE_EXTRA_DWORDS: &[(&str, u32)] = &[
    ("TotalStaticVRAM", 536_870_912), // 512 MB static report for Ironlake
];

pub fn patch_inf_for_win10_11(
    inf_path: &Path,
    out_dir: &Path,
    keep_ui: bool,
    is_haswell: bool,
) -> Result<()> {
    let raw = read_inf_text(inf_path)?;
    let raw_lower = raw.to_lowercase();
    let already_has_nt10 = raw_lower.contains("[intelgfx.ntamd64.10.0]");

    let mut existing_files: HashSet<String> = HashSet::new();
    if let Ok(entries) = fs::read_dir(out_dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                existing_files.insert(name.to_lowercase());
            }
        }
    }

    // PASS 1: Collect CopyFiles sections & all PCI\VEN_8086 device models from any [IntelGfx.NTamd64*] section
    let mut copy_sections: HashSet<String> = HashSet::new();
    let mut captured_device_lines: Vec<String> = Vec::new();
    let mut seen_models: HashSet<String> = HashSet::new();
    let mut scan_section = String::new();

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(';') {
            continue;
        }
        let lower = trimmed.to_lowercase();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            scan_section = trimmed[1..trimmed.len() - 1].trim().to_lowercase();
            continue;
        }

        if lower.starts_with("copyfiles") {
            if let Some((_, rhs)) = trimmed.split_once('=') {
                for item in rhs.split(',') {
                    let sec = item.trim().trim_start_matches('@').to_lowercase();
                    if !sec.is_empty() {
                        copy_sections.insert(sec);
                    }
                }
            }
        }

        if scan_section.starts_with("intelgfx.ntamd64") && lower.contains("pci\\ven_8086") {
            if seen_models.insert(lower) {
                captured_device_lines.push(line.to_string());
            }
        }
    }

    if !is_haswell {
        if !captured_device_lines.iter().any(|l| l.to_lowercase().contains("dev_0046")) {
            captured_device_lines.push(r"%iILKGM0% = iILKM0, PCI\VEN_8086&DEV_0046".to_string());
        }
        if !captured_device_lines.iter().any(|l| l.to_lowercase().contains("dev_0042")) {
            captured_device_lines.push(r"%iILKG0% = iILKD0, PCI\VEN_8086&DEV_0042".to_string());
        }
    }

    let model_count = captured_device_lines.len();

    // PASS 2: Rewrite INF cleanly
    let mut out_lines: Vec<String> = Vec::new();
    let mut current_section = String::new();
    let mut emitted_nt10_section = already_has_nt10;
    let mut injected_dwords = false;
    let mut pruned_count = 0usize;

    for line in raw.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();

        if lower.starts_with("catalogfile") {
            out_lines.push("CatalogFile=igdlh.cat".to_string());
            continue;
        }

        if lower.starts_with("excludefromselect") {
            out_lines.push(format!("; {}", line));
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if !emitted_nt10_section && current_section.starts_with("intelgfx.ntamd64") {
                let next_sec = trimmed[1..trimmed.len() - 1].trim().to_lowercase();
                if !next_sec.starts_with("intelgfx.ntamd64") {
                    out_lines.push(String::new());
                    out_lines.push("[IntelGfx.NTamd64.10.0]".to_string());
                    out_lines.extend(captured_device_lines.iter().cloned());
                    out_lines.push(String::new());
                    emitted_nt10_section = true;
                }
            }

            current_section = trimmed[1..trimmed.len() - 1].trim().to_lowercase();
            out_lines.push(line.to_string());
            continue;
        }

        if lower.starts_with("copyfiles") && lower.contains('@') {
            if let Some((_, rhs)) = trimmed.split_once('=') {
                let mut kept_items = Vec::new();
                for item in rhs.split(',') {
                    let it = item.trim();
                    if let Some(inline_file) = it.strip_prefix('@') {
                        let fname = inline_file.trim().to_lowercase();
                        if !existing_files.contains(&fname) {
                            pruned_count += 1;
                            continue;
                        }
                    }
                    kept_items.push(it.to_string());
                }
                if kept_items.is_empty() {
                    out_lines.push(format!("; {}", line));
                } else {
                    out_lines.push(format!("CopyFiles={}", kept_items.join(", ")));
                }
                continue;
            }
        }

        let in_source_disks = current_section.starts_with("sourcedisksfiles");
        if in_source_disks && !trimmed.is_empty() && !trimmed.starts_with(';') {
            if let Some((lhs, _)) = trimmed.split_once('=') {
                let fname = lhs.trim().trim_matches('"').to_lowercase();
                if !existing_files.contains(&fname) {
                    pruned_count += 1;
                    continue;
                }
            }
        }

        let in_copy_section = copy_sections.contains(&current_section);
        if in_copy_section && !trimmed.is_empty() && !trimmed.starts_with(';') {
            let parts: Vec<&str> = trimmed.split(',').collect();
            let dest_file = parts[0].trim().trim_matches('"').to_lowercase();
            let src_file = if parts.len() > 1 && !parts[1].trim().is_empty() {
                parts[1].trim().trim_matches('"').to_lowercase()
            } else {
                dest_file.clone()
            };

            let ui_blocked = !keep_ui
                && (dest_file == "gfxui.exe"
                    || dest_file == "gfxuiex.exe"
                    || dest_file == "gfxv4_0.exe"
                    || dest_file == "igfxrenu.lrc");
            if ui_blocked || !existing_files.contains(&src_file) {
                pruned_count += 1;
                continue;
            }
        }

        if !already_has_nt10
            && current_section == "manufacturer"
            && lower.contains("intelgfx")
            && !lower.contains("ntamd64.10.0")
        {
            if let Some((code_part, comment_part)) = line.split_once(';') {
                out_lines.push(format!("{}, NTamd64.10.0 ;{}", code_part.trim_end(), comment_part));
            } else {
                out_lines.push(format!("{}, NTamd64.10.0", line));
            }
            continue;
        }

        out_lines.push(line.to_string());

        // Inject DWORDs into [igfx_SoftwareDeviceSettings] or [SoftBIOS_AddReg] / Haswell software device settings
        let is_sw_settings = current_section == "igfx_softwaredevicesettings"
            || current_section.ends_with("_softwaredevicesettings");
        if is_sw_settings && !injected_dwords {
            out_lines.push("; --- KBridge v0.5.0 Ultimate Performance & 1440p60 Media Flags ---".to_string());
            for (key, val) in HARDCODED_INF_DWORDS_COMMON {
                out_lines.push(format!("HKR,,\"{}\",%REG_DWORD%,0x{:08X}", key, val));
            }
            let extra = if is_haswell {
                HASWELL_EXTRA_DWORDS
            } else {
                IRONLAKE_EXTRA_DWORDS
            };
            for (key, val) in extra {
                out_lines.push(format!("HKR,,\"{}\",%REG_DWORD%,0x{:08X}", key, val));
            }
            injected_dwords = true;
        }
    }

    if !emitted_nt10_section {
        out_lines.push(String::new());
        out_lines.push("[IntelGfx.NTamd64.10.0]".to_string());
        out_lines.extend(captured_device_lines);
    }

    let total_flags = HARDCODED_INF_DWORDS_COMMON.len()
        + if is_haswell {
            HASWELL_EXTRA_DWORDS.len()
        } else {
            IRONLAKE_EXTRA_DWORDS.len()
        };

    fs::write(inf_path, out_lines.join("\r\n"))?;
    println!(
        "[+] Patched igdlh64.inf: Win10/11 target ready ({} models), baked {} DWORDs, pruned {} bloat refs.",
        model_count, total_flags, pruned_count
    );
    Ok(())
}
