use anyhow::Result;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

const HARDCODED_INF_DWORDS: &[(&str, u32)] = &[
    ("DisableDynamicClocking", 1),
    ("ForceRenderPState", 0),
    ("RC6Enable", 0),
    ("PowerGatingEnabled", 0),
    ("IncreaseFixedSegment", 1),
    ("ForceGTTCacheable", 1),
    ("DisableTiledMemory", 0),
    ("TotalStaticVRAM", 536870912),
    ("Disable_OverlayDSQualityEnhancement", 1),
    ("AdaptiveVsyncEnable", 0),
    ("AsyncFlips", 1),
];

const BLOAT_FILE_PREFIXES: &[&str] = &[
    "igfxpers.exe",
    "hkcmd.exe",
    "igfxsrvc.exe",
    "igfxtray.exe",
    "igfxext.exe",
    "intcdaud",
    "igfxdo",
    "opencl",
    "igfxcm",
    "gfxres.",
    "igfxr",
];

pub fn patch_inf_for_win10_11(inf_path: &Path, out_dir: &Path, keep_ui: bool) -> Result<()> {
    let raw = fs::read_to_string(inf_path)?;

    let mut existing_files: HashSet<String> = HashSet::new();
    if let Ok(entries) = fs::read_dir(out_dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                existing_files.insert(name.to_lowercase());
            }
        }
    }

    let mut out_lines: Vec<String> = Vec::new();
    let mut win7_device_lines: Vec<String> = Vec::new();
    let mut in_win7_section = false;
    let mut in_source_disks_files = false;
    let mut injected_dwords = false;

    for line in raw.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();

        if lower.starts_with("catalogfile") {
            out_lines.push("CatalogFile=igdlh.cat".to_string());
            continue;
        }

        if trimmed.starts_with('[') {
            in_source_disks_files = lower.starts_with("[sourcedisksfiles");
        }

        if !keep_ui && (lower.starts_with("gfxui.exe") || lower.starts_with("igfxrenu.lrc")) {
            continue;
        }
        if BLOAT_FILE_PREFIXES.iter().any(|p| lower.starts_with(p)) {
            if !(keep_ui && lower.starts_with("igfxrenu.lrc")) {
                continue;
            }
        }
        if in_source_disks_files && !trimmed.starts_with('[') && !trimmed.starts_with(';') && !trimmed.is_empty() {
            if let Some((lhs, _)) = trimmed.split_once('=') {
                let fname = lhs.trim().to_lowercase();
                if !existing_files.contains(&fname) {
                    continue;
                }
            }
        }

        if lower.contains("cui.copy") || lower.contains("cuir.copy") || lower.contains("hotkey.copy") {
            if !keep_ui {
                out_lines.push(format!("; {}", line));
                continue;
            }
        }
        if lower.contains("opencl.copy") || lower.contains("audio.copy") || lower.contains("hdmi.copy") {
            out_lines.push(format!("; {}", line));
            continue;
        }

        if lower.starts_with("%intel%") && lower.contains("ntamd64.6.1") && !lower.contains("ntamd64.10.0") {
            out_lines.push(format!("{}, NTamd64.10.0", line));
            continue;
        }

        if lower.starts_with("[intelgfx.ntamd64.6.1]") || lower.starts_with("[intelgfx.ntamd64.6.2]") {
            in_win7_section = true;
            out_lines.push(line.to_string());
            continue;
        } else if in_win7_section && trimmed.starts_with('[') {
            in_win7_section = false;
            out_lines.push(String::new());
            out_lines.push("[IntelGfx.NTamd64.10.0]".to_string());
            out_lines.extend(win7_device_lines.drain(..));
            out_lines.push(String::new());
        } else if in_win7_section && !trimmed.is_empty() && !trimmed.starts_with(';') {
            win7_device_lines.push(line.to_string());
        }

        out_lines.push(line.to_string());
        if lower == "[igfx_softwaredevicesettings]" && !injected_dwords {
            out_lines.push("; --- KBridge Hardcoded Performance & Memory Flags ---".to_string());
            for (key, val) in HARDCODED_INF_DWORDS {
                out_lines.push(format!("HKR,,\"{}\",%REG_DWORD%,0x{:08X}", key, val));
            }
            injected_dwords = true;
        }
    }

    fs::write(inf_path, out_lines.join("\r\n"))?;
    println!("[+] Patched igdlh64.inf with [IntelGfx.NTamd64.10.0] and 11 performance DWORDs.");
    Ok(())
}
