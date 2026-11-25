use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub struct StrippedPackage {
    pub inf_path: PathBuf,
    pub orig_whql_cat: Option<PathBuf>,
    pub is_haswell: bool,
}

const CORE_WDDM_FILES: &[&str] = &[
    "igdkmd64.sys",
    "igdumd64.dll",
    "igdumd32.dll",
    "ig4icd64.dll",
    "ig4icd32.dll",
    "igd10umd64.dll",
    "igd10umd32.dll",
    "igdde64.dll",
    "igdde32.dll",
    "igdumdx32.dll",
    "igfxress.dll",
    "igdumdim64.dll",
    "igdumdim32.dll",
    "igd10iumd64.dll",
    "igd10iumd32.dll",
    "igd11dxva64.dll",
    "igd11dxva32.dll",
    "igd12umd64.dll",
    "igd12umd32.dll",
    "igdusc64.dll",
    "igdusc32.dll",
    "igc64.dll",
    "igc32.dll",
    "ig75icd64.dll",
    "ig75icd32.dll",
    "ig8icd64.dll",
    "ig8icd32.dll",
    "igdail64.dll",
    "igdail32.dll",
    "igdmd64.dll",
    "igdmd32.dll",
    "igdfcl64.dll",
    "igdfcl32.dll",
    "igfxcmrt64.dll",
    "igfxcmrt32.dll",
    "igfx11cmrt64.dll",
    "igfx11cmrt32.dll",
    "igfxcmjit64.dll",
    "igfxcmjit32.dll",
    "libmfxhw64.dll",
    "libmfxhw32.dll",
    "mfxplugin64_hw.dll",
    "mfxplugin32_hw.dll",
    "mfx_mft_h264ve_64.dll",
    "mfx_mft_h264ve_32.dll",
    "mfx_mft_vp8vd_64.dll",
    "mfx_mft_vp8vd_32.dll",
    "mfx_mft_vp9vd_64.dll",
    "mfx_mft_vp9vd_32.dll",
    "mfx_mft_mjpgvd_64.dll",
    "mfx_mft_mjpgvd_32.dll",
    "iglhcp64.dll",
    "iglhcp32.dll",
    "iglhsip64.dll",
    "iglhsip32.dll",
    "igfxexps.dll",
    "igfxexps32.dll",
    "igfxosp.dll",
];

const OPTIONAL_UI_FILES: &[&str] = &[
    "gfxui.exe",
    "gfxui.exe.config",
    "igfxrenu.lrc",
    "igfxcfg.dll",
    "igfxdev.dll",
    "igfxdevlib.dll",
    "hccutils.dll",
    "icclibdll_x64.dll",
    "igfxcpl.cpl",
    "gfxuiex.exe",
    "gfxv4_0.exe",
    "gfxv2_0.exe",
    "gfxv4_0.exe.config",
    "igfxdh.dll",
    "igfxdi.dll",
    "igfxdtcm.dll",
    "igfxlhm.dll",
    "igfxres.dll",
    "gfxresources.dll",
];

pub fn read_inf_text(path: &Path) -> Result<String> {
    let bytes = fs::read(path)?;
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let u16s: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return Ok(String::from_utf16_lossy(&u16s));
    }
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let u16s: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return Ok(String::from_utf16_lossy(&u16s));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn find_display_inf(dir: &Path) -> Option<PathBuf> {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(found) = find_display_inf(&path) {
                    return Some(found);
                }
            } else if path.is_file() {
                let fname = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
                if fname.ends_with(".inf")
                    && !fname.contains("intcdaud")
                    && !fname.contains("autorun")
                    && !fname.contains("heci")
                {
                    if let Ok(text) = read_inf_text(&path) {
                        if text.contains("igdkmd64.sys") || text.contains("IntelGfx") {
                            return Some(path);
                        }
                    }
                }
            }
        }
    }
    None
}

pub fn strip_driver_package(
    src_dir: &Path,
    out_dir: &Path,
    keep_ui: bool,
) -> Result<StrippedPackage> {
    if out_dir.exists() {
        let _ = fs::remove_dir_all(out_dir);
    }
    fs::create_dir_all(out_dir)?;

    let src_inf = find_display_inf(src_dir).with_context(|| {
        format!(
            "Could not find an Intel Display .inf file (igdlh64.inf or kit*.inf) inside '{}'. Make sure you extracted the zip/exe first.",
            src_dir.display()
        )
    })?;

    let inf_text = read_inf_text(&src_inf)?;
    let inf_low = inf_text.to_lowercase();
    let is_haswell = inf_low.contains("igd10iumd64.dll")
        || inf_low.contains("igdumdim64.dll")
        || inf_low.contains("dev_0416")
        || inf_low.contains("dev_0a16")
        || inf_low.contains("dev_041e");

    println!(
        "[+] Detected Driver Architecture: {}",
        if is_haswell {
            "Haswell / HD Graphics 4400 Family (WDDM 1.3 / 2.0)"
        } else {
            "Ironlake / 1st-Gen Intel HD Graphics (WDDM 1.1)"
        }
    );

    let graphics_dir = src_inf.parent().unwrap_or(src_dir);

    for entry in fs::read_dir(graphics_dir)?.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let fname = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();

        if fname.starts_with("igfxpers")
            || fname.starts_with("hkcmd")
            || fname.starts_with("igfxsrvc")
            || fname.starts_with("igfxtray")
            || fname.starts_with("igfxem")
            || fname.starts_with("igfxhk")
            || fname.starts_with("igfxmtc")
            || fname.contains("opencl")
        {
            continue;
        }

        let is_coinstaller = fname.starts_with("igfxcoin") && fname.ends_with(".dll");
        let is_microcode_blob = fname.ends_with(".bin")
            || fname.ends_with(".vp")
            || fname.ends_with(".cpa")
            || fname.ends_with(".hex")
            || fname.ends_with(".dat")
            || fname.ends_with(".icm");
        let is_mfx_media = is_haswell && fname.starts_with("mfx") && fname.ends_with(".dll");

        let should_keep = CORE_WDDM_FILES.contains(&fname.as_str())
            || is_coinstaller
            || is_microcode_blob
            || is_mfx_media
            || (keep_ui && OPTIONAL_UI_FILES.contains(&fname.as_str()));

        if should_keep {
            fs::copy(&path, out_dir.join(path.file_name().unwrap()))?;
            println!("[+] Kept core file: {}", fname);
        }
    }

    let inf_stem = src_inf
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    let mut found_cat: Option<PathBuf> = None;
    for entry in fs::read_dir(graphics_dir)?.flatten() {
        let path = entry.path();
        if path.is_file() {
            let fname = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
            if fname == format!("{}.cat", inf_stem) || fname == "igdlh.cat" {
                found_cat = Some(path);
                break;
            }
        }
    }

    let dst_inf = out_dir.join("igdlh64.inf");
    fs::write(&dst_inf, &inf_text)?;
    println!(
        "[+] Imported display INF: {} -> igdlh64.inf",
        src_inf.file_name().unwrap().to_string_lossy()
    );

    if let Some(ref cat) = found_cat {
        println!(
            "[+] Matched original WHQL catalog: {}",
            cat.file_name().unwrap().to_string_lossy()
        );
    }

    Ok(StrippedPackage {
        inf_path: dst_inf,
        orig_whql_cat: found_cat,
        is_haswell,
    })
}
