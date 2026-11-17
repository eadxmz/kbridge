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

const FILE_EXTENSIONS: &[&str] = &[
    ".exe", ".dll", ".sys", ".lrc", ".bin", ".cpl", ".chm", ".hlp", ".cab", ".xml", ".config", ".dat",
];

fn is_file_entry_for_missing_file(trimmed: &str, existing_files: &HashSet<String>) -> bool {
    if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('[') {
        return false;
    }
    // Check `filename = diskid,...` lines inside [SourceDisksFiles*]
    if let Some((lhs, _)) = trimmed.split_once('=') {
        let candidate = lhs.trim().trim_matches('"').to_lowercase();
        if FILE_EXTENSIONS.iter().any(|ext| candidate.ends_with(ext)) {
            return !existing_files.contains(&candidate);
        }
        return false;
    }

    // Check `dest_file[,src_file][,unused][,flags]` lines inside any CopyFiles / CoInstallers section
    let first_token = trimmed
        .split(',')
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches('"')
        .to_lowercase();

    if FILE_EXTENSIONS.iter().any(|ext| first_token.ends_with(ext)) {
        return !existing_files.contains(&first_token);
    }
    false
}

pub fn patch_inf_for_win10_11(inf_path: &Path, out_dir: &Path, keep_ui: bool) -> Result<()> {
    let raw = fs::read_to_string(inf_path)?;

    // Build exact set of lowercase filenames present in `out_dir`
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
    let mut capturing_win7 = false;
    let mut emitted_nt10_section = false;
    let mut injected_dwords = false;
    let mut pruned_count = 0usize;

    for line in raw.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();

        // 1. Point CatalogFile= to our freshly built igdlh.cat
        if lower.starts_with("catalogfile") {
            out_lines.push("CatalogFile=igdlh.cat".to_string());
            continue;
        }

        // 2. If user disabled UI, strip UI files from INF even if present
        if !keep_ui && (lower.starts_with("gfxui.exe") || lower.starts_with("igfxrenu.lrc")) {
            pruned_count += 1;
            continue;
        }

        // 3. Prune EVERY SourceDisksFiles or CopyFiles entry whose target file was stripped from out_dir
        //    This prevents pnputil / Device Manager from failing with ERROR_FILE_NOT_FOUND.
        if is_file_entry_for_missing_file(trimmed, &existing_files) {
            pruned_count += 1;
            continue;
        }

        // 4. Add NTamd64.10.0 to [Manufacturer] for Windows 10 & Windows 11
        if lower.starts_with("%intel%") && lower.contains("ntamd64.6.1") && !lower.contains("ntamd64.10.0") {
            out_lines.push(format!("{}, NTamd64.10.0", line));
            continue;
        }

        // 5. Capture [IntelGfx.NTamd64.6.1] once and emit [IntelGfx.NTamd64.10.0] right after it
        if trimmed.starts_with('[') {
            if capturing_win7 && !emitted_nt10_section {
                capturing_win7 = false;
                emitted_nt10_section = true;
                out_lines.push(String::new());
                out_lines.push("[IntelGfx.NTamd64.10.0]".to_string());
                out_lines.extend(win7_device_lines.drain(..));
                out_lines.push(String::new());
            }
            if lower.starts_with("[intelgfx.ntamd64.6.1]") && !emitted_nt10_section {
                capturing_win7 = true;
            }
        } else if capturing_win7 && !trimmed.is_empty() && !trimmed.starts_with(';') {
            win7_device_lines.push(line.to_string());
        }

        // 6. Bake 11 Hardcoded Registry Flags into [igfx_SoftwareDeviceSettings]
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
    println!(
        "[+] Patched igdlh64.inf: added [IntelGfx.NTamd64.10.0], baked 11 DWORDs, and pruned {} stripped file references.",
        pruned_count
    );
    Ok(())
}
