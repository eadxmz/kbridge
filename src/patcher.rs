use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

const KEEP_FILES: &[&str] = &[
    "igdlh64.inf",
    "igdlh.cat",
    "igdkmd64.sys",
    "igdumd64.dll",
    "ig4icd64.dll",
    "igfxcoin_v2993.dll",
];

const OPTIONAL_UI_FILES: &[&str] = &[
    "gfxui.exe",
    "gfxui.exe.config",
    "igfxrenu.lrc",
    "igfxcfg.dll",
    "igfxdev.dll",
];

pub fn strip_driver_package(
    src_dir: &Path,
    out_dir: &Path,
    keep_ui: bool,
) -> Result<Option<PathBuf>> {
    fs::create_dir_all(out_dir)?;
    let graphics_src = if src_dir.join("Graphics").exists() {
        src_dir.join("Graphics")
    } else {
        src_dir.to_path_buf()
    };

    let mut orig_cat_path: Option<PathBuf> = None;

    for entry in fs::read_dir(&graphics_src)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let fname = path.file_name().unwrap().to_string_lossy().to_lowercase();
            if fname == "igdlh.cat" {
                orig_cat_path = Some(path.clone());
            }
            let should_keep = KEEP_FILES.contains(&fname.as_str())
                || (keep_ui && OPTIONAL_UI_FILES.contains(&fname.as_str()));

            if should_keep {
                fs::copy(&path, out_dir.join(entry.file_name()))?;
                println!("[+] Kept core file: {}", fname);
            }
        }
    }
    Ok(orig_cat_path)
}

pub fn patch_pattern(buf: &mut [u8], pattern: &[Option<u8>], replacement: &[u8]) -> usize {
    let mut count = 0;
    if pattern.is_empty() || buf.len() < pattern.len() {
        return 0;
    }
    for i in 0..=(buf.len() - pattern.len()) {
        let matched = pattern
            .iter()
            .enumerate()
            .all(|(j, &b)| b.map_or(true, |val| buf[i + j] == val));
        if matched {
            buf[i..i + replacement.len()].copy_from_slice(replacement);
            count += 1;
        }
    }
    count
}

pub fn update_pe_checksum(pe_bytes: &mut [u8]) -> Result<u32> {
    if pe_bytes.len() < 0x40 || &pe_bytes[0..2] != b"MZ" {
        anyhow::bail!("Invalid MZ header");
    }
    let e_lfanew = u32::from_le_bytes(pe_bytes[0x3C..0x40].try_into()?) as usize;
    if pe_bytes.len() < e_lfanew + 0x5C || &pe_bytes[e_lfanew..e_lfanew + 4] != b"PE\0\0" {
        anyhow::bail!("Invalid PE header");
    }

    let checksum_offset = e_lfanew + 0x58;
    pe_bytes[checksum_offset..checksum_offset + 4].copy_from_slice(&[0, 0, 0, 0]);

    let mut checksum: u64 = 0;
    let mut chunks = pe_bytes.chunks_exact(2);
    for chunk in &mut chunks {
        let word = u16::from_le_bytes([chunk[0], chunk[1]]) as u64;
        checksum += word;
        checksum = (checksum & 0xFFFF) + (checksum >> 16);
    }
    if let &[rem] = chunks.remainder() {
        checksum += rem as u64;
        checksum = (checksum & 0xFFFF) + (checksum >> 16);
    }
    checksum = (checksum & 0xFFFF) + (checksum >> 16);
    let final_sum = (checksum as u32).wrapping_add(pe_bytes.len() as u32);

    pe_bytes[checksum_offset..checksum_offset + 4].copy_from_slice(&final_sum.to_le_bytes());
    Ok(final_sum)
}

pub fn patch_binaries(out_dir: &Path) -> Result<()> {
    let sys_path = out_dir.join("igdkmd64.sys");
    if sys_path.exists() {
        let mut sys_buf = fs::read(&sys_path)?;
        let os_check_pat: &[Option<u8>] = &[
            Some(0x83), None, None, Some(0x06), Some(0x0F), Some(0x85),
        ];
        let hits = patch_pattern(&mut sys_buf, os_check_pat, &[0x83]);
        let new_crc = update_pe_checksum(&mut sys_buf)?;
        fs::write(&sys_path, &sys_buf)?;
        println!(
            "[+] Patched igdkmd64.sys ({} patterns) | New PE Checksum: 0x{:08X}",
            hits, new_crc
        );
    }

    let umd_path = out_dir.join("igdumd64.dll");
    if umd_path.exists() {
        let mut umd_buf = fs::read(&umd_path)?;
        let new_crc = update_pe_checksum(&mut umd_buf)?;
        fs::write(&umd_path, &umd_buf)?;
        println!("[+] Updated igdumd64.dll | New PE Checksum: 0x{:08X}", new_crc);
    }
    Ok(())
}
